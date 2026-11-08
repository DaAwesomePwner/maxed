use crate::{
    project::{micros, tag},
    StoreError, StoreReader,
};
use maxed_types::*;
use rusqlite::{params, params_from_iter, types::Value, Connection, Params, Transaction};
use serde::{de::DeserializeOwned, Serialize};

const MAX_RUN_ID_BYTES: usize = 256;
const MAX_RUN_PAGE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_RUN_STORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_FIGHT_BYTES: usize = 8 * 1024 * 1024;
const MAX_PLAYER_BYTES: usize = 1024 * 1024;
const MAX_STORY_FIGHTS: usize = 12_000;
const MAX_STORY_TIMELINE: usize = 20_000;
const MAX_STORY_DROPS: usize = 2_000;
const DURATION: &str = "CASE WHEN ended>=started THEN ended-started END";

/// The same summary cap, ordinal ordering and party rule for run and visit scopes.
pub(crate) fn fight_summaries(
    tx: &Transaction<'_>,
    sql: &str,
    args: impl Params,
    bytes: &mut usize,
) -> Result<Vec<RunFightSummary>, StoreError> {
    let mut fights: Vec<RunFightSummary> =
        rows(tx, sql, args, MAX_STORY_FIGHTS, bytes, MAX_RUN_STORY_BYTES)?;
    let fight_ids = fights
        .iter()
        .map(|fight| fight.id.as_str())
        .collect::<Vec<_>>();
    let mut parties = crate::party_average::party_averages(tx, &fight_ids)?;
    for (index, fight) in fights.iter_mut().enumerate() {
        fight.ordinal = index as u32 + 1;
        fight.party = parties.remove(&fight.id);
    }
    Ok(fights)
}

pub fn clear_comparison(current_micros: f64, previous_micros: &[f64]) -> Option<ClearComparison> {
    if !current_micros.is_finite()
        || current_micros <= 0.0
        || previous_micros.len() < 3
        || previous_micros.iter().any(|n| !n.is_finite() || *n <= 0.0)
    {
        return None;
    }
    // Scale before summing so finite samples need not overflow their mean.
    let n = previous_micros.len() as f64;
    let mean = previous_micros.iter().map(|v| v / n).sum::<f64>();
    let change = (current_micros - mean) / mean;
    if !mean.is_finite() || mean <= 0.0 || !change.is_finite() {
        return None;
    }
    Some(ClearComparison {
        previous_clears: u32::try_from(previous_micros.len()).ok()?,
        mean_micros: mean,
        change_fraction: change,
    })
}

pub(crate) fn range(range: &TimeRange) -> Result<(i64, i64), StoreError> {
    let start = micros(range.start)?;
    let end = micros(range.end)?;
    if start >= end {
        return Err(StoreError::InvalidEvent);
    }
    Ok((start, end))
}

pub(crate) fn identifier(id: &str) -> Result<(), StoreError> {
    if id.is_empty() || id.len() > MAX_RUN_ID_BYTES || id.chars().any(char::is_control) {
        return Err(StoreError::InvalidEvent);
    }
    Ok(())
}

pub(crate) fn add_bytes(bytes: &mut usize, amount: usize, limit: usize) -> Result<(), StoreError> {
    *bytes = bytes
        .checked_add(amount)
        .filter(|n| *n <= limit)
        .ok_or(StoreError::Limit)?;
    Ok(())
}

pub(crate) fn row_json<T: DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    column: usize,
    bytes: &mut usize,
    limit: usize,
) -> Result<T, StoreError> {
    let size = usize::try_from(row.get::<_, i64>(column)?).map_err(|_| StoreError::InvalidEvent)?;
    add_bytes(bytes, size, limit)?;
    Ok(serde_json::from_str(&row.get::<_, String>(column + 1)?)?)
}

struct CappedBytes {
    bytes: usize,
    limit: usize,
}
impl std::io::Write for CappedBytes {
    fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(value.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| std::io::Error::other("history size limit"))?;
        Ok(value.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) fn bounded<T: Serialize>(value: T, limit: usize) -> Result<T, StoreError> {
    let mut sink = CappedBytes { bytes: 0, limit };
    serde_json::to_writer(&mut sink, &value).map_err(|error| {
        if error.is_io() {
            StoreError::Limit
        } else {
            StoreError::InvalidEvent
        }
    })?;
    Ok(value)
}

fn one<T: DeserializeOwned + Serialize>(
    conn: &Connection,
    sql: &str,
    args: impl Params,
    limit: usize,
) -> Result<Option<T>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let mut rows = statement.query(args)?;
    match rows.next()? {
        Some(row) => bounded(Some(row_json(row, 0, &mut 0, limit)?), limit),
        None => Ok(None),
    }
}

pub(crate) fn rows<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    args: impl Params,
    limit: usize,
    bytes: &mut usize,
    byte_limit: usize,
) -> Result<Vec<T>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let mut query = statement.query(args)?;
    let mut result = Vec::new();
    while let Some(row) = query.next()? {
        if result.len() == limit {
            return Err(StoreError::Limit);
        }
        result.push(row_json(row, 0, bytes, byte_limit)?);
    }
    Ok(result)
}

impl StoreReader {
    pub fn dungeon_bests(&self, account: Option<&str>) -> Result<Vec<DungeonBest>, StoreError> {
        let json: Option<String> = self.conn.query_row(
            &crate::account_sql::with_accounts(include_str!("dungeon_bests.sql")),
            [account],
            |row| row.get(0),
        )?;
        bounded(
            serde_json::from_str::<Vec<DungeonBest>>(&json.ok_or(StoreError::Limit)?)?,
            1024 * 1024,
        )
    }

    pub fn list_runs(&self, filter: RunFilter) -> Result<RunPage, StoreError> {
        if !(1..=100).contains(&filter.limit) {
            return Err(StoreError::InvalidEvent);
        }
        let (key, descending) = match filter.sort {
            RunSort::Newest => ("started".to_owned(), true),
            RunSort::Oldest => ("started".to_owned(), false),
            RunSort::DurationAscending => {
                (format!("coalesce(({DURATION}),9223372036854775807)"), false)
            }
            RunSort::DurationDescending => (format!("coalesce(({DURATION}),-1)"), true),
        };
        let mut predicates = vec!["1=1".to_owned()];
        let mut args = Vec::<Value>::new();
        if let Some(dungeon) = &filter.dungeon {
            identifier(dungeon)?;
            args.push(dungeon.clone().into());
            predicates.push(format!("dungeon=?{}", args.len()));
        }
        if let Some(character) = &filter.character_id {
            identifier(character)?;
            args.push(character.clone().into());
            predicates.push(format!("character_id=?{}", args.len()));
        }
        if let Some(outcome) = &filter.outcome {
            args.push(tag(outcome)?.into());
            predicates.push(format!("outcome=?{}", args.len()));
        }
        if let Some(time) = &filter.range {
            let (start, end) = range(time)?;
            args.push(start.into());
            predicates.push(format!("started>=?{}", args.len()));
            args.push(end.into());
            predicates.push(format!("started<?{}", args.len()));
        }
        if let Some(cursor) = &filter.cursor {
            let value = cursor
                .sort_value
                .parse::<i64>()
                .map_err(|_| StoreError::InvalidEvent)?;
            identifier(&cursor.id)?;
            if value < 0 && !(value == -1 && filter.sort == RunSort::DurationDescending)
                || cursor.sort_value != value.to_string()
            {
                return Err(StoreError::InvalidEvent);
            }
            args.push(value.into());
            let number = args.len();
            args.push(cursor.id.clone().into());
            predicates.push(format!(
                "({key},id){}(?{number},?{})",
                if descending { "<" } else { ">" },
                args.len()
            ));
        }
        args.push((i64::from(filter.limit) + 1).into());
        let direction = if descending { "DESC" } else { "ASC" };
        let sql=format!("SELECT length(CAST(json AS BLOB)),json,{key} FROM runs WHERE {} ORDER BY {key} {direction},id {direction} LIMIT ?{}",predicates.join(" AND "),args.len());
        let tx = self.conn.unchecked_transaction()?;
        let mut statement = tx.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(args))?;
        let mut runs = Vec::new();
        let mut last = None;
        let mut more = false;
        let mut bytes = 0usize;
        while let Some(row) = result.next()? {
            if runs.len() == usize::from(filter.limit) {
                more = true;
                break;
            }
            let run: RunView = row_json(row, 0, &mut bytes, MAX_RUN_PAGE_BYTES)?;
            last = Some(RunCursor {
                sort_value: row.get::<_, i64>(2)?.to_string(),
                id: run.id.clone(),
            });
            runs.push(run);
        }
        bounded(
            RunPage {
                details: Some(crate::run_rows::details(&tx, &runs)?),
                summary: crate::run_rows::summary(&tx, &filter)?,
                rows: runs,
                next: if more { last } else { None },
            },
            MAX_RUN_PAGE_BYTES,
        )
    }

    pub fn get_run(&self, id: &str) -> Result<Option<RunStory>, StoreError> {
        identifier(id)?;
        let tx = self.conn.unchecked_transaction()?;
        let mut bytes = 0;
        let (run, mut character): (RunView, Option<CharacterView>) = {
            let mut stmt = tx.prepare("SELECT length(CAST(json AS BLOB)),json,length(CAST(entry_character_json AS BLOB)),entry_character_json FROM runs WHERE id=?1")?;
            let mut query = stmt.query([id])?;
            let Some(row) = query.next()? else {
                return Ok(None);
            };
            let run = row_json(row, 0, &mut bytes, MAX_RUN_STORY_BYTES)?;
            let character = if row.get::<_, Option<i64>>(2)?.is_some() {
                Some(row_json(row, 2, &mut bytes, MAX_RUN_STORY_BYTES)?)
            } else {
                None
            };
            (run, character)
        };
        if let Some(entry) = character
            .as_mut()
            .filter(|c| c.name.is_none() || c.class_id.is_none())
        {
            let mut stmt = tx.prepare("SELECT length(CAST(identity AS BLOB)),identity FROM
                (SELECT json_array(json_extract(json,'$.name'),json_extract(json,'$.class_id')) AS identity
                 FROM characters WHERE id=?1)")?;
            let mut query = stmt.query([&entry.id])?;
            if let Some(row) = query.next()? {
                let (name, class_id): (Option<String>, Option<u16>) =
                    row_json(row, 0, &mut bytes, MAX_RUN_STORY_BYTES)?;
                entry.name = entry.name.take().or(name);
                entry.class_id = entry.class_id.or(class_id);
            }
        }
        let fights = fight_summaries(&tx,
            "SELECT length(CAST(json AS BLOB)),json FROM fight_summaries WHERE run_id=?1 ORDER BY started,fight_id LIMIT 12001",
            [id], &mut bytes)?;
        let drops = rows(&tx,
            "SELECT length(CAST(json AS BLOB)),json FROM drops WHERE run_id=?1 ORDER BY at,id LIMIT 2001",
            [id], MAX_STORY_DROPS, &mut bytes, MAX_RUN_STORY_BYTES)?;
        // Hide legacy post-clear departures without rewriting saved facts.
        let mut statement = tx.prepare("SELECT length(CAST(id AS BLOB))+coalesce(length(CAST(related_id AS BLOB)),0)+length(CAST(explanation AS BLOB))+length(CAST(kind AS BLOB)),id,at,kind,related_id,explanation FROM timeline WHERE run_id=?1 AND (kind!='player_left' OR ?2 IS NULL OR at<?2) ORDER BY at,id LIMIT 20001")?;
        let mut query = statement.query(params![id, run.completed_at.map(micros).transpose()?])?;
        let mut timeline = Vec::new();
        while let Some(row) = query.next()? {
            if timeline.len() == MAX_STORY_TIMELINE {
                return Err(StoreError::Limit);
            }
            add_bytes(
                &mut bytes,
                usize::try_from(row.get::<_, i64>(0)?).map_err(|_| StoreError::InvalidEvent)?,
                MAX_RUN_STORY_BYTES,
            )?;
            timeline.push(TimelineEntry {
                id: row.get(1)?,
                at: Timestamp(
                    u64::try_from(row.get::<_, i64>(2)?).map_err(|_| StoreError::InvalidEvent)?,
                ),
                kind: serde_json::from_value(serde_json::Value::String(row.get(3)?))?,
                related_id: row.get(4)?,
                explanation: row.get(5)?,
            });
        }
        drop(query);
        drop(statement);
        let comparison = comparison(&tx, &run)?;
        let screen = crate::run_screen::load(&tx, &run, &mut bytes)?;
        let story = crate::run_screen::bound_story(
            RunStory {
                boss_fights: Some(screen.boss_fights),
                boss_fights_limited: Some(screen.boss_fights_limited),
                events: Some(screen.events),
                events_limited: Some(screen.events_limited),
                older_run_id: screen.older_run_id,
                newer_run_id: screen.newer_run_id,
                totals: crate::instance::totals(&tx, &run)?,
                run,
                character,
                fights,
                drops,
                timeline,
                comparison,
            },
            MAX_RUN_STORY_BYTES,
        )?;
        tx.commit()?;
        Ok(Some(story))
    }

    pub fn get_fight(&self, id: &str) -> Result<Option<FightView>, StoreError> {
        identifier(id)?;
        one(
            &self.conn,
            "SELECT length(CAST(json AS BLOB)),json FROM fights WHERE id=?1",
            [id],
            MAX_FIGHT_BYTES,
        )
    }

    pub fn get_player_breakdown(
        &self,
        fight_id: &str,
        object_id: i32,
    ) -> Result<Option<PlayerDamage>, StoreError> {
        identifier(fight_id)?;
        let mut players = rows(
            &self.conn,
            "SELECT length(CAST(payload AS BLOB)),payload FROM (
                SELECT json_set(fight_players.json,'$.share',
                    CASE WHEN json_extract(f.json,'$.total_damage')>0
                    THEN damage/json_extract(f.json,'$.total_damage') END) AS payload
                FROM fight_players JOIN fights f ON f.id=fight_id
                WHERE fight_id=?1 AND object_id=?2 LIMIT 2)",
            params![fight_id, object_id],
            2,
            &mut 0,
            MAX_PLAYER_BYTES,
        )?;
        bounded(
            if players.len() == 1 {
                players.pop()
            } else {
                None
            },
            MAX_PLAYER_BYTES,
        )
    }

    pub fn get_player_by_id(
        &self,
        fight_id: &str,
        player_id: &str,
    ) -> Result<Option<PlayerDamage>, StoreError> {
        identifier(fight_id)?;
        identifier(player_id)?;
        one(
            &self.conn,
            "SELECT length(CAST(payload AS BLOB)),payload FROM (
                SELECT json_set(fight_players.json,'$.share',
                    CASE WHEN json_extract(f.json,'$.total_damage')>0
                    THEN damage/json_extract(f.json,'$.total_damage') END) AS payload
                FROM fight_players JOIN fights f ON f.id=fight_id
                WHERE fight_id=?1 AND player_id=?2)",
            params![fight_id, player_id],
            MAX_PLAYER_BYTES,
        )
    }

    pub fn bag_count_since(&self, bag_type: u16, time: TimeRange) -> Result<u32, StoreError> {
        let (start, end) = range(&time)?;
        let count: i64 = self.conn.query_row(
            "SELECT count(*) FROM drops WHERE bag_type=?1 AND at>=?2 AND at<?3",
            params![bag_type, start, end],
            |r| r.get(0),
        )?;
        u32::try_from(count).map_err(|_| StoreError::InvalidEvent)
    }
}

fn comparison(conn: &Connection, run: &RunView) -> Result<Option<ClearComparison>, StoreError> {
    if run.area_kind != AreaKind::Dungeon
        || run.outcome != Some(Outcome::Cleared)
        || !observed_completion(run)
    {
        return Ok(None);
    }
    let Some(current) = run
        .completed_at
        .and_then(|t| t.0.checked_sub(run.started.0))
        .filter(|n| *n > 0)
    else {
        return Ok(None);
    };
    // Aggregate in SQL instead of fetching every historical run. Counter-based
    // timestamps are fallback/estimated clear durations, even at zero delay.
    let sql = &crate::account_sql::with_accounts(include_str!("run_comparison.sql"));
    let (count, mean): (i64, Option<f64>) = conn.query_row(
        sql,
        params![run.dungeon, micros(run.started)?, run.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let Some(mean) = mean.filter(|n| n.is_finite() && *n > 0.0) else {
        return Ok(None);
    };
    if count < 3 {
        return Ok(None);
    }
    let change = (current as f64 - mean) / mean;
    if !change.is_finite() {
        return Ok(None);
    }
    Ok(Some(ClearComparison {
        previous_clears: u32::try_from(count).map_err(|_| StoreError::InvalidEvent)?,
        mean_micros: mean,
        change_fraction: change,
    }))
}

fn observed_completion(run: &RunView) -> bool {
    run.evidence.iter().any(|e| {
        matches!(
            e.evidence,
            Evidence::Victory | Evidence::CompletionDialogue { .. }
        )
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ScopeLootCursor {
    scope: HomeScope,
    priority: u8,
    item_type: u16,
}
fn loot_cursor(
    scope: &HomeScope,
    cursor: Option<&str>,
) -> Result<Option<ScopeLootCursor>, StoreError> {
    cursor
        .map(|text| {
            if text.len() > 2048 {
                return Err(StoreError::InvalidEvent);
            }
            let value: ScopeLootCursor =
                serde_json::from_str(text).map_err(|_| StoreError::InvalidEvent)?;
            if &value.scope != scope || !(1..=3).contains(&value.priority) {
                return Err(StoreError::InvalidEvent);
            }
            Ok(value)
        })
        .transpose()
}
/// Bound catalog metadata before allocating query JSON or scheduling a read.
pub fn validate_loot_catalog(
    tiers: &[(u16, Option<String>)],
    potions: &[(u16, PotionStat)],
) -> Result<(), StoreError> {
    if tiers.len() > 65_536
        || potions.len() > 65_536
        || tiers
            .iter()
            .any(|(_, tier)| tier.as_ref().is_some_and(|s| s.len() > 16))
    {
        return Err(StoreError::Limit);
    }
    Ok(())
}

pub fn validate_scope_loot(
    scope: &HomeScope,
    cursor: Option<&str>,
    limit: u16,
) -> Result<(), StoreError> {
    if !(1..=100).contains(&limit) {
        return Err(StoreError::InvalidEvent);
    }
    match scope {
        HomeScope::Session { id } => identifier(id)?,
        HomeScope::Today { range: time } => {
            range(time)?;
        }
    }
    loot_cursor(scope, cursor)?;
    Ok(())
}
impl StoreReader {
    pub fn scope_loot(
        &self,
        scope: HomeScope,
        account: Option<&str>,
        cursor: Option<String>,
        limit: u16,
        tiers: &[(u16, Option<String>)],
    ) -> Result<ScopeLootPage, StoreError> {
        validate_scope_loot(&scope, cursor.as_deref(), limit)?;
        let cursor = loot_cursor(&scope, cursor.as_deref())?;
        let (kind, id, lo, hi) = match &scope {
            HomeScope::Session { id } => (0, Some(id.as_str()), None, None),
            HomeScope::Today { range: time } => {
                let (lo, hi) = range(time)?;
                (1, None, Some(lo), Some(hi))
            }
        };
        // One tier per u16 type; reject pathological metadata before allocating SQL JSON.
        validate_loot_catalog(tiers, &[])?;
        let unique: std::collections::BTreeMap<_, _> = tiers.iter().cloned().collect();
        let tiers = serde_json::to_string(&unique.into_iter().collect::<Vec<_>>())?;
        let json: Option<String> = self.conn.query_row(
            &crate::account_sql::with_accounts(include_str!("scope_loot.sql")),
            params![
                kind,
                id,
                lo,
                hi,
                tiers,
                cursor.as_ref().map(|c| c.priority),
                cursor.as_ref().map(|c| c.item_type),
                i64::from(limit) + 1,
                account,
            ],
            |row| row.get(0),
        )?;
        let mut page: ScopeLootPage = serde_json::from_str(&json.ok_or(StoreError::Limit)?)?;
        if page.items.len() > usize::from(limit) {
            page.items.truncate(usize::from(limit));
            let last = page.items.last().ok_or(StoreError::InvalidEvent)?;
            page.next = Some(serde_json::to_string(&ScopeLootCursor {
                scope,
                priority: match last.tier.as_deref() {
                    Some("UT") => 3,
                    Some("ST") => 2,
                    _ => 1,
                },
                item_type: last.item_type,
            })?);
        }
        bounded(page, 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    thread_local! {
        static STATEMENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static REVISE: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
    }
    fn trace(event: rusqlite::trace::TraceEvent<'_>) {
        if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
            let sql = statement.sql();
            STATEMENTS.with(|s| s.borrow_mut().push(sql.to_string()));
            if sql.contains("FROM fight_summaries") {
                if let Some(path) = REVISE.with(|s| s.borrow_mut().take()) {
                    let mut db = Connection::open(path).unwrap();
                    let tx = db.transaction().unwrap();
                    tx.execute(
                        "UPDATE runs SET json=json_set(json,'$.dungeon','revised')",
                        [],
                    )
                    .unwrap();
                    tx.execute(
                        "UPDATE fight_summaries SET json=json_set(json,'$.total_damage',999)",
                        [],
                    )
                    .unwrap();
                    tx.execute("UPDATE timeline SET explanation='revised'", [])
                        .unwrap();
                    tx.commit().unwrap();
                }
            }
        }
    }
    fn count() -> Vec<String> {
        STATEMENTS.with(|s| {
            s.borrow_mut()
                .drain(..)
                .filter(|sql| sql.starts_with("SELECT") || sql.starts_with("WITH"))
                .collect()
        })
    }
    #[test]
    fn query_count_is_constant_and_story_snapshot_is_consistent() {
        for fights in [1, 400, 3000] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("trace");
            let mut db = Connection::open(&path).unwrap();
            crate::migrate(&mut db).unwrap();
            db.pragma_update(None, "journal_mode", "WAL").unwrap();
            let snapshot = demo::demo_snapshot();
            let mut events: Vec<_> = [
                EngineEvent::SessionUpdated(snapshot.session.unwrap()),
                EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
                EngineEvent::RunStarted(snapshot.run.unwrap()),
            ]
            .into_iter()
            .collect();
            for i in 0..fights {
                let mut fight = snapshot.fights[0].clone();
                fight.id = format!("f-{i:04}");
                events.push(EngineEvent::FightStarted(fight));
            }
            let events: Vec<_> = events
                .into_iter()
                .enumerate()
                .map(|(i, event)| EventEnvelope {
                    fight_chart_omitted: false,
                    stream_id: "trace".into(),
                    sequence: Sequence(Timestamp(i as u64 + 1)),
                    at: Timestamp(5_000_000),
                    event,
                })
                .collect();
            crate::apply_batch(&mut db, &events).unwrap();
            let reader = crate::open_reader(&path).unwrap();
            reader.conn.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                Some(trace),
            );
            count();
            let filter = RunFilter {
                character_id: None,
                dungeon: None,
                outcome: None,
                range: None,
                sort: RunSort::Newest,
                cursor: None,
                limit: 50,
            };
            reader.list_runs(filter).unwrap();
            assert_eq!(count().len(), 3);
            let expected = reader.get_run("demo-run").unwrap().unwrap();
            let mut sql = count();
            assert_eq!(sql.len(), 14, "{sql:?}");
            assert!(sql.remove(2).contains("FROM party_avg"));
            assert_eq!(sql.len(), 13, "{sql:?}");
            assert!(sql[..5].iter().all(|q| !q.contains("FROM fights")
                && !q.contains("fight_players")
                && !q.contains("damage_sources")));
            assert_eq!(expected.fights.len(), fights);
            assert_eq!(expected.fights.last().unwrap().ordinal, fights as u32);
            reader.get_fight("f-0000").unwrap();
            assert_eq!(count().len(), 1);
            reader.get_player_breakdown("f-0000", 1).unwrap();
            assert_eq!(count().len(), 1);
            REVISE.with(|s| *s.borrow_mut() = Some(path.clone()));
            assert_eq!(reader.get_run("demo-run").unwrap().unwrap(), expected);
            count();
            let revised = reader.get_run("demo-run").unwrap().unwrap();
            count();
            assert_eq!(revised.run.dungeon, "revised");
            assert!(revised.fights.iter().all(|f| f.total_damage == 999.0));
            assert!(revised.timeline.iter().all(|t| t.explanation == "revised"));
        }
    }

    #[test]
    fn capped_serialization_checks_escaped_utf8_and_checked_addition() {
        let value = "\u{e9}\n";
        let exact = serde_json::to_vec(value).unwrap().len();
        assert!(bounded(value, exact).is_ok());
        assert!(matches!(bounded(value, exact - 1), Err(StoreError::Limit)));
        let mut bytes = usize::MAX;
        assert!(matches!(
            add_bytes(&mut bytes, 1, usize::MAX),
            Err(StoreError::Limit)
        ));
    }

    #[test]
    fn fixed_runs_workloads_use_selective_query_plans() {
        use crate::runs_workload::{build, cases, FIGHT, REALISTIC, RUN, WORST};
        thread_local! {
            static SQL: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }
        fn capture(event: rusqlite::trace::TraceEvent<'_>) {
            if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
                if statement.sql().starts_with("SELECT") || statement.sql().starts_with("WITH") {
                    SQL.with(|s| s.borrow_mut().push(statement.expanded_sql().unwrap()));
                }
            }
        }
        for shape in [REALISTIC, WORST] {
            let fixture = build(shape);
            crate::run_rows::tests::seed_identity_history(&fixture.db);
            let reader = fixture.validate();
            reader.conn.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                Some(capture),
            );
            let check = |label: &str| {
                let mut statements = SQL.with(|s| std::mem::take(&mut *s.borrow_mut()));
                if label == "story" {
                    assert_eq!(statements.len(), 14);
                    let party_sql = statements.remove(2);
                    assert!(party_sql.contains("FROM party_avg"));
                    let plan = fixture
                        .db
                        .prepare(&format!("EXPLAIN QUERY PLAN {party_sql}"))
                        .unwrap()
                        .query_map([], |row| row.get::<_, String>(3))
                        .unwrap()
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap()
                        .join("\n");
                    assert!(plan.contains("SEARCH fight_players"), "{plan}");
                    assert!(!plan.contains("SCAN fight_players"), "{plan}");
                    assert_eq!(statements.len(), 13);
                    crate::run_screen::tests::assert_plans(&fixture.db, &statements[5..9]);
                    crate::instance::tests::assert_aggregate_plans(&fixture.db, &statements[9..]);
                    fixture.assert_plans(label, &statements[..5]);
                } else if matches!(label, "fight" | "player") {
                    fixture.assert_plans(label, &statements);
                } else {
                    assert_eq!(statements.len(), if label == "deep-keyset" { 2 } else { 3 });
                    fixture.assert_plans(label, &statements[..1]);
                    crate::run_rows::tests::assert_plans(&fixture.db, &statements[1..]);
                }
            };
            for (label, filter) in cases() {
                reader.list_runs(filter).unwrap();
                check(label);
            }
            reader.get_run(RUN).unwrap().unwrap();
            check("story");
            reader.get_fight(FIGHT).unwrap().unwrap();
            check("fight");
            reader.get_player_breakdown(FIGHT, 1).unwrap().unwrap();
            check("player");
        }
    }
}

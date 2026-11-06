//! Stats readers: per-map fame rates, sessions and lifetime dungeon records.
use crate::{
    project::micros,
    queries::{identifier, range, row_json},
    StoreError, StoreReader,
};
use maxed_types::*;
use rusqlite::{params, Connection, OptionalExtension};

pub(crate) const DUNGEON_STATS_SLOT: &str = "/*dungeon_stats*/";

/// Splice the shared dungeon-statistics CTE into a host statement exactly once.
pub(crate) fn with_dungeon_stats(sql: &str) -> String {
    debug_assert_eq!(sql.matches(DUNGEON_STATS_SLOT).count(), 1);
    sql.replacen(DUNGEON_STATS_SLOT, include_str!("dungeon_stats.sql"), 1)
}

pub(crate) fn count(value: i64) -> Result<u32, StoreError> {
    u32::try_from(value).map_err(|_| StoreError::InvalidEvent)
}

pub(crate) fn unsigned(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::InvalidEvent)
}

/// Integer median, flooring the even-count mean without overflowing the sum.
pub(crate) fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        values[middle - 1] + (values[middle] - values[middle - 1]) / 2
    } else {
        values[middle]
    })
}

/// A validated filter: range start and end in micros (`None` is all time), and the character.
pub(crate) type Bounds<'a> = (Option<i64>, Option<i64>, Option<&'a str>);

pub(crate) fn bounds(filter: &StatsFilter) -> Result<Bounds<'_>, StoreError> {
    let (lo, hi) = match &filter.range {
        Some(time) => {
            let (lo, hi) = range(time)?;
            (Some(lo), Some(hi))
        }
        None => (None, None),
    };
    if let Some(character) = &filter.character_id {
        identifier(character)?;
    }
    Ok((lo, hi, filter.character_id.as_deref()))
}

/// Characters with recorded fame or visits, most recently seen first. Merged
/// provisional identities are hidden; their history now belongs to the verified id.
pub(crate) fn characters(conn: &Connection) -> Result<Vec<StatsCharacter>, StoreError> {
    let mut statement = conn.prepare(
        "SELECT json FROM characters c
         WHERE c.id NOT IN (SELECT provisional_id FROM character_aliases)
           AND (EXISTS(SELECT 1 FROM fame_samples f WHERE f.character_id=c.id)
             OR EXISTS(SELECT 1 FROM map_visits v WHERE v.character_id=c.id))
         ORDER BY c.observed_at DESC, c.id LIMIT ?1",
    )?;
    let rows = statement.query_map([MAX_STATS_CHARACTERS as i64], |r| r.get::<_, String>(0))?;
    rows.map(|json| {
        let character: CharacterView = serde_json::from_str(&json?)?;
        Ok(StatsCharacter {
            id: character.id,
            name: character.name,
            class_id: character.class_id,
            last_seen: character.observed_at,
        })
    })
    .collect()
}

pub(crate) fn map_rows(
    conn: &Connection,
    lo: Option<i64>,
    hi: Option<i64>,
    character: Option<&str>,
    session: Option<&str>,
) -> Result<(Vec<StatsMapRow>, bool), StoreError> {
    let mut statement = conn.prepare_cached(include_str!("stats_maps.sql"))?;
    let rows = statement.query_map(params![lo, hi, character, session], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            [r.get::<_, i64>(2)?, r.get(3)?, r.get(4)?],
            r.get::<_, bool>(5)?,
            [r.get::<_, i64>(6)?, r.get(7)?],
        ))
    })?;
    let mut out = rows
        .map(|row| {
            let (group, map, [visits, micros, fame], missing, [clears, deaths]) = row?;
            let group = match group.as_str() {
                "dungeon" => StatsMapGroup::Dungeon,
                "realm" => StatsMapGroup::Realm,
                "hubs" => StatsMapGroup::Hubs,
                _ => StatsMapGroup::Other,
            };
            let time_micros = unsigned(micros)?;
            let dungeon = group == StatsMapGroup::Dungeon;
            Ok(StatsMapRow {
                group,
                map,
                visits: count(visits)?,
                time_micros,
                fame,
                missing_baseline: missing,
                fame_per_hour: fame_per_hour(fame, time_micros),
                clears: dungeon.then(|| count(clears)).transpose()?,
                deaths: dungeon.then(|| count(deaths)).transpose()?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let truncated = out.len() > MAX_STATS_ROWS;
    out.truncate(MAX_STATS_ROWS);
    Ok((out, truncated))
}

const CANDIDATE_BATCH: usize = 128;
const MAX_SESSION_RUN_BYTES: usize = 8 * 1024 * 1024;

/// A session's runs with their totals, for the "runs with party" export.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRunParty {
    pub run: RunView,
    pub totals: Option<RunTotals>,
}

struct Candidate {
    id: String,
    started: i64,
    last_traffic: i64,
    ended: Option<i64>,
}

fn candidate(r: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    Ok(Candidate {
        id: r.get(0)?,
        started: r.get(1)?,
        last_traffic: r.get(2)?,
        ended: r.get(3)?,
    })
}

fn candidates(
    conn: &Connection,
    lo: Option<i64>,
    hi: Option<i64>,
    cursor: Option<&(i64, String)>,
) -> Result<Vec<Candidate>, StoreError> {
    let mut statement = conn.prepare_cached(include_str!("stats_sessions.sql"))?;
    let rows = statement.query_map(
        params![
            lo,
            hi,
            cursor.map(|c| c.0),
            cursor.map(|c| c.1.as_str()),
            CANDIDATE_BATCH as i64
        ],
        candidate,
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// `None` when `require_activity` and the character has nothing in the session.
fn session_row(
    conn: &Connection,
    session: &Candidate,
    character: Option<&str>,
    require_activity: bool,
) -> Result<Option<StatsSessionRow>, StoreError> {
    let mut statement = conn.prepare_cached(include_str!("stats_session_totals.sql"))?;
    let (fame, missing, runs, clears, deaths, active, bags, notable_bags) =
        statement.query_row(params![session.id, character], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?;
    if require_activity && !active {
        return Ok(None);
    }
    let started = unsigned(session.started)?;
    let until = unsigned(session.ended.unwrap_or(session.last_traffic))?;
    let duration_micros = until.saturating_sub(started);
    Ok(Some(StatsSessionRow {
        id: session.id.clone(),
        started: Timestamp(started),
        ended: session.ended.map(unsigned).transpose()?.map(Timestamp),
        duration_micros,
        fame,
        missing_baseline: missing > 0,
        fame_per_hour: fame_per_hour(fame, duration_micros),
        bags: count(bags)?,
        notable_bags: count(notable_bags)?,
        bags_per_hour: fame_per_hour(bags, duration_micros),
        notable_bags_per_hour: fame_per_hour(notable_bags, duration_micros),
        runs: count(runs)?,
        clears: count(clears)?,
        deaths: count(deaths)?,
    }))
}

/// Up to `limit` rows after `cursor`, and whether another row qualifies.
fn session_rows(
    conn: &Connection,
    filter: &StatsFilter,
    include_empty: bool,
    mut cursor: Option<(i64, String)>,
    limit: usize,
) -> Result<(Vec<StatsSessionRow>, bool), StoreError> {
    let (lo, hi, character) = bounds(filter)?;
    let mut rows = Vec::new();
    loop {
        let batch = candidates(conn, lo, hi, cursor.as_ref())?;
        let exhausted = batch.len() < CANDIDATE_BATCH;
        for session in batch {
            cursor = Some((session.started, session.id.clone()));
            let Some(row) = session_row(conn, &session, character, true)? else {
                continue;
            };
            if !include_empty && row.fame == 0 && row.runs == 0 && row.deaths == 0 && row.bags == 0
            {
                continue;
            }
            if rows.len() == limit {
                return Ok((rows, true));
            }
            rows.push(row);
        }
        if exhausted {
            return Ok((rows, false));
        }
    }
}

fn session_runs(
    conn: &Connection,
    session: &str,
    character: Option<&str>,
) -> Result<(Vec<RunView>, bool), StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT length(CAST(json AS BLOB)),json FROM runs INDEXED BY runs_session
         WHERE session_id=?1 AND (?2 IS NULL OR character_id=?2) ORDER BY started,id LIMIT ?3",
    )?;
    let mut rows = statement.query(params![
        session,
        character,
        MAX_STATS_SESSION_RUNS as i64 + 1
    ])?;
    let mut bytes = 0;
    let mut runs = Vec::new();
    while let Some(row) = rows.next()? {
        runs.push(row_json(row, 0, &mut bytes, MAX_SESSION_RUN_BYTES)?);
    }
    let truncated = runs.len() > MAX_STATS_SESSION_RUNS;
    runs.truncate(MAX_STATS_SESSION_RUNS);
    Ok((runs, truncated))
}

impl StoreReader {
    pub fn stats_sessions(
        &self,
        filter: StatsSessionFilter,
    ) -> Result<StatsSessionPage, StoreError> {
        if !(1..=MAX_STATS_SESSION_PAGE).contains(&filter.limit) {
            return Err(StoreError::InvalidEvent);
        }
        let cursor = match &filter.cursor {
            Some(c) => {
                identifier(&c.id)?;
                Some((micros(c.started)?, c.id.clone()))
            }
            None => None,
        };
        let scope = StatsFilter {
            range: filter.range.clone(),
            character_id: filter.character_id.clone(),
        };
        let tx = self.conn.unchecked_transaction()?;
        let (rows, more) = session_rows(
            &tx,
            &scope,
            filter.include_empty,
            cursor,
            usize::from(filter.limit),
        )?;
        let next = rows.last().filter(|_| more).map(|row| StatsSessionCursor {
            started: row.started,
            id: row.id.clone(),
        });
        let characters = characters(&tx)?;
        tx.commit()?;
        Ok(StatsSessionPage {
            rows,
            next,
            characters,
        })
    }

    pub fn stats_sessions_export(
        &self,
        filter: StatsFilter,
        include_empty: bool,
    ) -> Result<(Vec<StatsSessionRow>, bool), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let result = session_rows(&tx, &filter, include_empty, None, MAX_STATS_EXPORT_SESSIONS)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn stats_session(
        &self,
        request: StatsSessionRequest,
    ) -> Result<Option<StatsSessionDetail>, StoreError> {
        identifier(&request.id)?;
        let scope = StatsFilter {
            range: None,
            character_id: request.character_id.clone(),
        };
        let (_, _, character) = bounds(&scope)?;
        let tx = self.conn.unchecked_transaction()?;
        let found = tx
            .query_row(
                "SELECT id,started,last_traffic,ended FROM play_sessions WHERE id=?1",
                [&request.id],
                candidate,
            )
            .optional()?;
        let Some(found) = found else {
            return Ok(None);
        };
        let session =
            session_row(&tx, &found, character, false)?.ok_or(StoreError::InvalidEvent)?;
        let (maps, maps_truncated) = map_rows(&tx, None, None, character, Some(&request.id))?;
        let recorded: i64 = maps.iter().map(|m| m.fame).sum();
        let unrecorded_fame = (session.fame > recorded).then(|| session.fame - recorded);
        let (runs, runs_truncated) = session_runs(&tx, &request.id, character)?;
        let characters = characters(&tx)?;
        tx.commit()?;
        Ok(Some(StatsSessionDetail {
            session,
            maps,
            maps_truncated,
            unrecorded_fame,
            runs,
            runs_truncated,
            characters,
        }))
    }

    pub fn stats_session_party(
        &self,
        session_id: &str,
        character: Option<&str>,
    ) -> Result<Vec<SessionRunParty>, StoreError> {
        identifier(session_id)?;
        if let Some(character) = character {
            identifier(character)?;
        }
        let tx = self.conn.unchecked_transaction()?;
        let (runs, _) = session_runs(&tx, session_id, character)?;
        let party = runs
            .into_iter()
            .map(|run| {
                Ok(SessionRunParty {
                    totals: crate::instance::totals(&tx, &run)?,
                    run,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        tx.commit()?;
        Ok(party)
    }

    pub fn stats_maps(&self, filter: StatsFilter) -> Result<StatsMaps, StoreError> {
        let (lo, hi, character) = bounds(&filter)?;
        let tx = self.conn.unchecked_transaction()?;
        let (rows, truncated) = map_rows(&tx, lo, hi, character, None)?;
        let tracking_since = tx
            .query_row(
                "SELECT min(started) FROM map_visits WHERE source='live'",
                [],
                |r| r.get::<_, Option<i64>>(0),
            )?
            .map(unsigned)
            .transpose()?
            .map(Timestamp);
        let characters = characters(&tx)?;
        tx.commit()?;
        Ok(StatsMaps {
            rows,
            truncated,
            tracking_since,
            characters,
        })
    }

    pub fn stats_dungeons(&self, filter: StatsFilter) -> Result<StatsDungeons, StoreError> {
        let (lo, hi, character) = bounds(&filter)?;
        let tx = self.conn.unchecked_transaction()?;
        let mut rows = {
            let sql = with_dungeon_stats(include_str!("stats_dungeons.sql"));
            let mut statement = tx.prepare(&sql)?;
            let rows = statement.query_map(params![lo, hi, character], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    [r.get::<_, i64>(1)?, r.get(2)?, r.get(3)?, r.get(4)?],
                    r.get::<_, Option<f64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                ))
            })?;
            rows.map(|row| {
                let (dungeon, [runs, clears, deaths, observed], mean, best, total, last) = row?;
                Ok(StatsDungeonRow {
                    dungeon,
                    runs: count(runs)?,
                    clears: count(clears)?,
                    deaths: count(deaths)?,
                    observed_clears: count(observed)?,
                    mean_clear_micros: mean,
                    best_clear_micros: best.map(unsigned).transpose()?,
                    total_micros: unsigned(total)?,
                    last_started: Timestamp(unsigned(last)?),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?
        };
        let truncated = rows.len() > MAX_STATS_ROWS;
        rows.truncate(MAX_STATS_ROWS);
        let characters = characters(&tx)?;
        tx.commit()?;
        Ok(StatsDungeons {
            rows,
            truncated,
            characters,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stats_reads_use_their_indexes_and_never_scan_fame() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrate(&mut db).unwrap();
        for (sql, indexes) in [
            (
                include_str!("stats_maps.sql"),
                &["fame_session", "map_visits_session"][..],
            ),
            (
                include_str!("stats_sessions.sql"),
                &["play_sessions_started"][..],
            ),
            (
                include_str!("stats_session_totals.sql"),
                &[
                    "fame_session",
                    "runs_session",
                    "deaths_session",
                    "drops_session",
                ][..],
            ),
        ] {
            let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let parameters = vec![rusqlite::types::Value::Null; statement.parameter_count()];
            let plan = statement
                .query_map(rusqlite::params_from_iter(parameters), |r| {
                    r.get::<_, String>(3)
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            for index in indexes {
                assert!(plan.contains(index), "{index}:\n{plan}");
            }
            // Fame samples are aliased `f` or `b`; plans name the alias, not the table.
            let scans_fame = plan.lines().map(str::trim_start).any(|line| {
                ["SCAN f", "SCAN b", "SCAN fame_samples"]
                    .iter()
                    .any(|scan| line == *scan || line.starts_with(&format!("{scan} ")))
            });
            assert!(!scans_fame, "{plan}");
        }
    }
}

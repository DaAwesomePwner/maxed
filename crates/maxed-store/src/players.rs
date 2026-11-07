use crate::{
    project::{json, micros},
    queries::{add_bytes, bounded, identifier, row_json, rows},
    StoreError, StoreReader,
};
use maxed_types::*;
use rusqlite::{params, Connection, OptionalExtension, Params, Transaction};
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_RUN_PLAYER_BYTES: usize = 2 * 1024 * 1024;
const MAX_PLAYER_CONTEXT_BYTES: usize = 1024 * 1024;

struct ReadBudget {
    used: usize,
    limit: usize,
}

impl ReadBudget {
    fn rows<T: serde::de::DeserializeOwned>(
        &mut self,
        conn: &Connection,
        sql: &str,
        args: impl Params,
    ) -> Result<Vec<T>, StoreError> {
        rows(conn, sql, args, usize::MAX, &mut self.used, self.limit)
    }

    fn one<T: serde::de::DeserializeOwned>(
        &mut self,
        conn: &Connection,
        sql: &str,
        args: impl Params,
    ) -> Result<Option<T>, StoreError> {
        let mut result = rows(conn, sql, args, 1, &mut self.used, self.limit)?;
        Ok(result.pop())
    }
}

impl StoreReader {
    pub fn get_run_player(
        &self,
        run_id: &str,
        player_id: &str,
    ) -> Result<Option<RunPlayerView>, StoreError> {
        identifier(run_id)?;
        identifier(player_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let mut budget = ReadBudget {
            used: 0,
            limit: MAX_RUN_PLAYER_BYTES,
        };
        let Some(run): Option<RunView> = budget.one(
            &tx,
            "SELECT length(CAST(json AS BLOB)),json FROM runs WHERE id=?1",
            [run_id],
        )?
        else {
            return Ok(None);
        };
        let totals = crate::instance::totals(&tx, &run)?;
        bounded(&totals, budget.limit)?;
        add_bytes(
            &mut budget.used,
            serde_json::to_vec(&totals)?.len(),
            budget.limit,
        )?;
        let players = totals.as_ref().map_or(&[][..], |t| t.players.as_slice());
        let player = players
            .iter()
            .find(|p| p.player_id == player_id && p.damage > 0.)
            .cloned();
        let party = party_summary(totals.as_ref());
        let partial = totals.as_ref().map_or(!run.complete_capture, |t| t.partial);
        let sources = run_sources(&tx, run_id, player_id, &mut budget)?;
        let fights = run_fights(&tx, run_id, player_id, &mut budget)?;
        let context = player_context(&tx, run_id, player_id, None, None, &mut budget)?;
        let is_local = player.as_ref().is_some_and(|p| p.is_local)
            || context.profile.as_ref().is_some_and(|p| p.is_local)
            || run.character_id.as_deref() == Some(player_id);
        let deaths = player_deaths(&tx, run_id, player_id, is_local, None, None, &mut budget)?;
        let departed_at = if let Some(profile) = &context.profile {
            tx.query_row(
                "SELECT max(at) FROM instance_events WHERE run_id=?1 AND kind='player_left'
                    AND json_extract(json,'$.kind.object_id')=?2",
                params![run_id, profile.object_id],
                |row| row.get::<_, Option<i64>>(0),
            )?
            .map(timestamp)
            .transpose()?
        } else {
            None
        };
        let roster = run_roster(&tx, run_id, players, &mut budget)?;
        let result = bounded(
            RunPlayerView {
                run,
                scope_id: run_id.into(),
                player_id: player_id.into(),
                player,
                party,
                sources,
                fights,
                profile: context.profile,
                gear_log: context.gear_log,
                timeline: context.timeline,
                deaths,
                departed_at,
                roster,
                partial,
            },
            MAX_RUN_PLAYER_BYTES,
        )?;
        tx.commit()?;
        Ok(Some(result))
    }

    pub fn get_player_context(
        &self,
        scope_id: &str,
        player_id: &str,
        from: Option<Timestamp>,
        until: Option<Timestamp>,
    ) -> Result<Option<PlayerContextView>, StoreError> {
        identifier(scope_id)?;
        identifier(player_id)?;
        let from = from.map(micros).transpose()?;
        let until = until.map(micros).transpose()?;
        if from.zip(until).is_some_and(|(from, until)| from > until) {
            return Err(StoreError::InvalidEvent);
        }
        let tx = self.conn.unchecked_transaction()?;
        let mut budget = ReadBudget {
            used: 0,
            limit: MAX_PLAYER_CONTEXT_BYTES,
        };
        let mut context = player_context(&tx, scope_id, player_id, from, until, &mut budget)?;
        if context.profile.is_none() && context.timeline.is_none() {
            return Ok(None);
        }
        let is_local = context.profile.as_ref().is_some_and(|p| p.is_local)
            || tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1 AND character_id=?2)",
                params![scope_id, player_id],
                |r| r.get::<_, bool>(0),
            )?;
        context.deaths =
            player_deaths(&tx, scope_id, player_id, is_local, from, until, &mut budget)?;
        let context = bounded(context, MAX_PLAYER_CONTEXT_BYTES)?;
        tx.commit()?;
        Ok(Some(context))
    }
}

fn party_summary(totals: Option<&RunTotals>) -> PartySummary {
    let players = totals.map_or(&[][..], |t| t.players.as_slice());
    let damaging: Vec<_> = players.iter().filter(|p| p.damage > 0.).collect();
    let dps: Vec<_> = damaging.iter().filter_map(|p| p.dps).collect();
    let top = damaging.iter().filter(|p| p.dps.is_some()).max_by(|a, b| {
        a.dps
            .unwrap()
            .total_cmp(&b.dps.unwrap())
            .then_with(|| b.player_id.cmp(&a.player_id))
    });
    PartySummary {
        total_damage: totals.map_or(0., |t| t.total_damage),
        average_dps: (!dps.is_empty()).then(|| dps.iter().map(|d| d / dps.len() as f64).sum()),
        top_player_id: top.map(|p| p.player_id.clone()),
        top_name: top.and_then(|p| p.name.clone()),
        top_dps: top.and_then(|p| p.dps),
        players_with_damage: damaging.len() as u32,
    }
}

fn run_sources(
    conn: &Connection,
    run_id: &str,
    player_id: &str,
    budget: &mut ReadBudget,
) -> Result<Vec<SourceDamage>, StoreError> {
    let sources: Vec<SourceDamage> = budget.rows(
        conn,
        "SELECT length(CAST(d.json AS BLOB)),d.json FROM fight_summaries s
            JOIN damage_sources d ON d.fight_id=s.fight_id
            WHERE s.run_id=?1 AND d.player_id=?2 ORDER BY s.started,s.fight_id,d.source_index",
        params![run_id, player_id],
    )?;
    let mut groups = BTreeMap::<String, SourceDamage>::new();
    for mut source in sources {
        source.reasons.sort();
        let key = serde_json::to_string(&(
            source.item_type,
            &source.kind,
            &source.enchant,
            source.estimated,
            &source.reasons,
        ))?;
        if let Some(group) = groups.get_mut(&key) {
            group.damage += source.damage;
            group.hits = group.hits.saturating_add(source.hits);
            group.biggest_hit = match (group.biggest_hit, source.biggest_hit) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        } else {
            groups.insert(key, source);
        }
    }
    let mut result: Vec<_> = groups.into_values().collect();
    result.sort_by(|a, b| b.damage.total_cmp(&a.damage));
    Ok(result)
}

fn run_fights(
    conn: &Connection,
    run_id: &str,
    player_id: &str,
    budget: &mut ReadBudget,
) -> Result<Vec<RunPlayerFight>, StoreError> {
    let mut fights: Vec<RunPlayerFight> = budget.rows(conn, crate::run_strip::FIGHTS, [run_id])?;
    for fight in &mut fights {
        let contribution: Option<FightContribution> = budget.one(conn,
            "SELECT length(CAST(payload AS BLOB)),payload FROM (
                SELECT json_object('object_id',
                    CASE WHEN (SELECT count(*) FROM fight_players other
                        WHERE other.fight_id=p.fight_id AND other.object_id=p.object_id)=1
                        THEN p.object_id END,
                    'damage',p.damage,'dps',json_extract(p.json,'$.dps'),'rank',json_extract(p.json,'$.rank'),
                    'share',json_extract(p.json,'$.share'),'deaths',json_extract(p.json,'$.deaths')) AS payload
                FROM fight_players p WHERE p.fight_id=?1 AND p.player_id=?2)",
            params![fight.fight_id, player_id])?;
        if let Some(contribution) = contribution {
            fight.object_id = contribution.object_id;
            fight.damage = Some(contribution.damage);
            fight.dps = contribution.dps;
            fight.rank = contribution.rank;
            fight.share = contribution.share;
            fight.deaths = contribution.deaths;
        }
    }
    Ok(fights)
}

#[derive(Deserialize)]
struct FightContribution {
    object_id: Option<i32>,
    damage: f64,
    dps: Option<f64>,
    rank: Option<u32>,
    share: Option<f64>,
    deaths: Option<u32>,
}

fn run_roster(
    conn: &Connection,
    run_id: &str,
    players: &[RunPlayerTotal],
    budget: &mut ReadBudget,
) -> Result<Vec<RosterEntry>, StoreError> {
    let mut roster: Vec<RosterEntry> = budget.rows(conn,
        "SELECT length(CAST(payload AS BLOB)),payload FROM (
            SELECT json_object('player_id',player_id,'name',name,'class_id',class_id,
                'is_local',json(CASE WHEN is_local>0 THEN 'true' ELSE 'false' END),
                'damage',damage+damage_correction) AS payload FROM pd_run_players WHERE run_id=?1
            UNION ALL
            SELECT json_object('player_id',p.player_id,'name',json_extract(p.json,'$.name'),
                'class_id',json_extract(p.json,'$.class_id'),'is_local',json(CASE WHEN p.is_local THEN 'true' ELSE 'false' END),
                'damage',NULL) FROM player_profiles p WHERE p.run_id=?1
                AND NOT EXISTS(SELECT 1 FROM pd_run_players r WHERE r.run_id=?1 AND r.player_id=p.player_id))",
        [run_id])?;
    // Older runs may not have the cached projection; totals supplies its legacy fallback.
    for player in players {
        if let Some(entry) = roster.iter_mut().find(|r| r.player_id == player.player_id) {
            if entry.damage.is_none() {
                entry.damage = Some(player.damage);
            }
        } else {
            roster.push(RosterEntry {
                player_id: player.player_id.clone(),
                name: player.name.clone(),
                class_id: player.class_id,
                is_local: player.is_local,
                damage: Some(player.damage),
            });
        }
    }
    roster.sort_by(|a, b| {
        b.damage
            .partial_cmp(&a.damage)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.player_id.cmp(&b.player_id))
    });
    Ok(roster)
}

#[derive(Deserialize)]
struct StoredScopeTimeline {
    buckets: Vec<ScopeBucket>,
    compacted: bool,
    disabled: bool,
}

#[derive(Deserialize)]
struct StoredPlayerTimeline {
    points: Vec<f64>,
}

fn player_context(
    conn: &Connection,
    scope_id: &str,
    player_id: &str,
    from: Option<i64>,
    until: Option<i64>,
    budget: &mut ReadBudget,
) -> Result<PlayerContextView, StoreError> {
    let mut profile: Option<PlayerProfileView> = budget.one(conn,
        "SELECT length(CAST(json AS BLOB)),json FROM player_profiles WHERE scope_id=?1 AND player_id=?2",
        params![scope_id, player_id])?;
    if let Some(profile) = &mut profile {
        profile.normalize_maxed();
    }
    let scope: Option<StoredScopeTimeline> = budget.one(
        conn,
        "SELECT length(CAST(json AS BLOB)),json FROM scope_timelines WHERE scope_id=?1",
        [scope_id],
    )?;
    let timeline = if let Some(mut scope) = scope {
        let player: Option<StoredPlayerTimeline> = budget.one(conn,
            "SELECT length(CAST(json AS BLOB)),json FROM player_timelines WHERE scope_id=?1 AND player_id=?2",
            params![scope_id, player_id])?;
        if scope.disabled {
            scope.buckets.clear();
        }
        let mut points = player.map_or_else(Vec::new, |p| p.points);
        points.resize(scope.buckets.len(), 0.);
        let (buckets, points) = scope
            .buckets
            .into_iter()
            .zip(points)
            .filter(|(bucket, _)| {
                from.is_none_or(|from| bucket.until.0 > from as u64)
                    && until.is_none_or(|until| bucket.from.0 <= until as u64)
            })
            .unzip();
        Some(PlayerTimeline {
            buckets,
            points,
            compacted: scope.compacted,
            disabled: scope.disabled,
        })
    } else {
        None
    };
    let mut statement = conn.prepare(
        "SELECT at,length(CAST(json AS BLOB)),json FROM player_gear_log
            WHERE scope_id=?1 AND player_id=?2 AND (?4 IS NULL OR at<=?4)
                AND (?3 IS NULL OR at>?3 OR at=(SELECT max(at) FROM player_gear_log
                    WHERE scope_id=?1 AND player_id=?2 AND at<=?3)) ORDER BY at",
    )?;
    let mut rows = statement.query(params![scope_id, player_id, from, until])?;
    let mut gear_log = Vec::new();
    while let Some(row) = rows.next()? {
        gear_log.push(GearChange {
            at: timestamp(row.get(0)?)?,
            equipment: row_json(row, 1, &mut budget.used, budget.limit)?,
        });
    }
    Ok(PlayerContextView {
        profile,
        gear_log,
        timeline,
        deaths: Vec::new(),
    })
}

fn timestamp(at: i64) -> Result<Timestamp, StoreError> {
    Ok(Timestamp(
        u64::try_from(at).map_err(|_| StoreError::InvalidEvent)?,
    ))
}

fn player_deaths(
    conn: &Connection,
    scope_id: &str,
    player_id: &str,
    is_local: bool,
    from: Option<i64>,
    until: Option<i64>,
    budget: &mut ReadBudget,
) -> Result<Vec<PlayerDeathAt>, StoreError> {
    // Map scopes and instance IDs both encode the stream and map ordinal.
    let map_prefix = scope_id
        .rsplit_once(":map:")
        .map(|(stream, ordinal)| format!("{stream}:instance:{ordinal}:"));
    let (fight_scope, notice_scope) = if map_prefix.is_some() {
        (
            "f.run_id IS NULL AND json_extract(f.json,'$.scope_id')=?1",
            "run_id IS NULL AND substr(id,1,length(?6))=?6",
        )
    } else {
        ("s.run_id=?1", "NOT ?3 AND run_id=?1")
    };
    let sql = format!("SELECT length(CAST(payload AS BLOB)),payload FROM (
        SELECT json_object('at',CAST(d.at AS TEXT),'fight_id',(
            SELECT s.fight_id FROM fight_summaries s JOIN fights f ON f.id=s.fight_id
            WHERE {fight_scope}
                AND (json_extract(s.json,'$.target_kind') IN ('boss','event_boss') OR f.boss_type IS NOT NULL)
                AND s.started<=d.at AND (json_extract(s.json,'$.ended') IS NULL OR CAST(json_extract(s.json,'$.ended') AS INTEGER)>=d.at)
            ORDER BY s.started,s.fight_id LIMIT 1)) AS payload
        FROM (
            SELECT at FROM deaths WHERE ?3 AND run_id=?1 AND (character_id=?2 OR character_id IS NULL)
            UNION ALL
            SELECT at FROM instance_events WHERE kind='player_death'
                AND json_extract(json,'$.kind.player_id')=?2
                AND {notice_scope}
        ) d WHERE (?4 IS NULL OR d.at>=?4) AND (?5 IS NULL OR d.at<=?5) ORDER BY d.at)");
    if let Some(map_prefix) = map_prefix {
        budget.rows(
            conn,
            &sql,
            params![scope_id, player_id, is_local, from, until, map_prefix],
        )
    } else {
        budget.rows(
            conn,
            &sql,
            params![scope_id, player_id, is_local, from, until],
        )
    }
}

pub(crate) fn write_profile(
    tx: &Transaction<'_>,
    profile: &PlayerProfileView,
) -> Result<(), StoreError> {
    let previous: Option<String> = tx.query_row(
        "SELECT json_extract(json,'$.equipment') FROM player_profiles WHERE scope_id=?1 AND player_id=?2",
        params![profile.scope_id, profile.player_id],
        |row| row.get(0),
    ).optional()?;
    let previous = previous
        .map(|text| serde_json::from_str::<Vec<EquipmentView>>(&text))
        .transpose()?;
    if previous.as_ref() != Some(&profile.equipment) {
        let at = if previous.is_none() {
            profile.first_seen
        } else {
            profile.last_seen
        };
        let rows: i64 = tx.query_row(
            "SELECT count(*) FROM player_gear_log WHERE scope_id=?1 AND player_id=?2",
            params![profile.scope_id, profile.player_id],
            |row| row.get(0),
        )?;
        if rows >= 64 {
            tx.execute(
                "DELETE FROM player_gear_log WHERE scope_id=?1 AND player_id=?2 AND at=(
                    SELECT max(at) FROM player_gear_log WHERE scope_id=?1 AND player_id=?2)",
                params![profile.scope_id, profile.player_id],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO player_gear_log(scope_id,player_id,at,json) VALUES(?1,?2,?3,?4)",
            params![profile.scope_id, profile.player_id, micros(at)?, json(&profile.equipment)?],
        )?;
    }
    tx.execute(
        "INSERT INTO player_profiles(scope_id,player_id,object_id,is_local,run_id,first_seen,last_seen,json)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
         ON CONFLICT(scope_id,player_id) DO UPDATE SET object_id=excluded.object_id,
             is_local=excluded.is_local,run_id=excluded.run_id,first_seen=excluded.first_seen,
             last_seen=excluded.last_seen,json=excluded.json",
        params![profile.scope_id, profile.player_id, profile.object_id, profile.is_local, profile.run_id,
            micros(profile.first_seen)?, micros(profile.last_seen)?, json(profile)?],
    )?;
    Ok(())
}

pub(crate) fn write_timeline(
    tx: &Transaction<'_>,
    update: &ScopeTimelineUpdate,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO scope_timelines(scope_id,run_id,json) VALUES(?1,?2,?3)
         ON CONFLICT(scope_id) DO UPDATE SET run_id=excluded.run_id,json=excluded.json",
        params![update.scope_id, update.run_id, json(&serde_json::json!({
            "buckets": update.buckets, "compacted": update.compacted, "disabled": update.disabled,
        }))?],
    )?;
    if update.full || update.disabled {
        tx.execute(
            "DELETE FROM player_timelines WHERE scope_id=?1",
            [&update.scope_id],
        )?;
    }
    for player in &update.players {
        tx.execute(
            "INSERT INTO player_timelines(scope_id,player_id,json) VALUES(?1,?2,?3)
             ON CONFLICT(scope_id,player_id) DO UPDATE SET json=excluded.json",
            params![update.scope_id, player.player_id, json(&serde_json::json!({
                "object_id": player.object_id, "is_local": player.is_local, "points": player.points,
            }))?],
        )?;
    }
    Ok(())
}

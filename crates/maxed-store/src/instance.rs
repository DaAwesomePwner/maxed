//! Durable instance facts and bounded whole-run aggregates. No raw combat rows.
use crate::{
    project::{json, matching_run, micros, tag, timeline},
    StoreError,
};
use maxed_engine::names::display_name;
use maxed_types::*;
use rusqlite::{params, Connection, Transaction};

/// Normalize historical display fields without rewriting stored facts or local flags.
pub(crate) fn normalize_names(mut event: InstanceEventView) -> InstanceEventView {
    let name = match &mut event.kind {
        InstanceEventKind::PlayerDeath { player_name, .. } => Some(player_name),
        InstanceEventKind::PlayerLeft { player_name, .. } => player_name.as_mut(),
        InstanceEventKind::PortalOpened { opener, .. } => opener.as_mut(),
        InstanceEventKind::RealmEventKilled { killer, .. } => killer.as_mut(),
    };
    if let Some(name) = name {
        name.truncate(display_name(name).len());
    }
    event
}

pub(crate) fn project(tx: &Transaction<'_>, event: &InstanceEventView) -> Result<(), StoreError> {
    matching_run(tx, event.run_id.as_deref(), &event.session_id)?;
    let (kind, timeline_kind, mut explanation) = match &event.kind {
        InstanceEventKind::PlayerDeath {
            player_name,
            level,
            killed_by,
            ..
        } => (
            "player_death",
            TimelineKind::PartyDeath,
            format!(
                "{} died{}{}",
                display_name(player_name),
                killed_by
                    .as_ref()
                    .map(|s| format!(" to {s}"))
                    .unwrap_or_default(),
                level.map(|n| format!(" (level {n})")).unwrap_or_default()
            ),
        ),
        InstanceEventKind::PlayerLeft { player_name, .. } => (
            "player_left",
            TimelineKind::PlayerLeft,
            format!(
                "{} left (cause unconfirmed)",
                player_name
                    .as_deref()
                    .map(display_name)
                    .unwrap_or("Unknown player")
            ),
        ),
        InstanceEventKind::PortalOpened {
            opener, dungeon, ..
        } => (
            "portal_opened",
            TimelineKind::PortalOpened,
            format!(
                "{} opened {}",
                opener
                    .as_deref()
                    .map(display_name)
                    .unwrap_or("Unknown player"),
                dungeon.as_deref().unwrap_or("a portal")
            ),
        ),
        InstanceEventKind::RealmEventKilled {
            boss_key, killer, ..
        } => (
            "realm_event_killed",
            TimelineKind::Kill,
            format!(
                "{boss_key} killed{}",
                killer
                    .as_ref()
                    .map(|s| format!(" by {}", display_name(s)))
                    .unwrap_or_default()
            ),
        ),
    };
    if event.provisional {
        explanation.push_str(" (provisional)");
    }
    tx.execute("INSERT INTO instance_events VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET session_id=excluded.session_id,run_id=excluded.run_id,at=excluded.at,kind=excluded.kind,json=excluded.json",
        params![event.id,event.session_id,event.run_id,micros(event.at)?,kind,json(event)?])?;
    // Upserts may revise run ownership; never leave a stale timeline association.
    tx.execute("DELETE FROM timeline WHERE related_id=?1 AND kind IN ('party_death','player_left','portal_opened','kill')", [&event.id])?;
    if let Some(run) = &event.run_id {
        timeline(
            tx,
            run,
            &format!("instance:{}", event.id),
            event.at,
            timeline_kind,
            Some(&event.id),
            &explanation,
        )?;
    }
    Ok(())
}

pub(crate) fn kill_timeline(tx: &Transaction<'_>, fight: &FightView) -> Result<(), StoreError> {
    tx.execute(
        "DELETE FROM timeline WHERE related_id=?1 AND kind='kill'",
        [&fight.id],
    )?;
    if let (
        Some(run),
        Some(kill),
        Some(TargetKind::Boss | TargetKind::EventBoss | TargetKind::Miniboss),
    ) = (&fight.run_id, &fight.kill, &fight.target_kind)
    {
        let explanation = format!(
            "Kill observed: {}{}",
            tag(&kill.evidence)?,
            if kill.provisional {
                " (provisional)"
            } else {
                ""
            }
        );
        timeline(
            tx,
            run,
            &format!("kill:{}", fight.id),
            kill.at,
            TimelineKind::Kill,
            Some(&fight.id),
            &explanation,
        )?;
    }
    Ok(())
}

// Only aggregate scalars cross the SQLite boundary. In particular player sources,
// damage buckets and per-player series are never deserialized for a run story.
pub(crate) fn summary_json(fight: &FightView) -> Result<String, StoreError> {
    let mut value = serde_json::to_value(crate::project::fight_summary(fight)?)?;
    value["unattributed_damage"] = serde_json::to_value(fight.unattributed_damage)?;
    value["kill_count"] = serde_json::json!(u32::from(fight.kill.is_some()));
    json(&value)
}

const TOTALS_SQL: &str = "SELECT count(*),
    coalesce(sum(json_extract(json,'$.kill_count')),0),
    min(CAST(json_extract(json,'$.first_hit') AS INTEGER)),
    max(CAST(json_extract(json,'$.last_hit') AS INTEGER)),
    coalesce(sum(json_extract(json,'$.total_damage')),0.0),
    coalesce(sum(json_extract(json,'$.unattributed_damage')),0.0),
    coalesce(max(json_extract(json,'$.partial') OR json_extract(json,'$.estimated')),0),
    coalesce(sum(json_extract(json,'$.player_count')),0),
    coalesce(max(json_type(json,'$.unattributed_damage') IS NULL),0)
    FROM fight_summaries WHERE run_id=?1";

// The fixed-width timestamp and hex ID form an unambiguous ordered key.
// '!' sorts before every hex digit, preserving ID-prefix ordering as well.
// This implements last-non-null without sorting all observations twice.
const PLAYERS_SQL: &str = "WITH combined AS (
    SELECT p.player_id,
        max(CASE WHEN CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.name') END IS NOT NULL THEN
            printf('%019d',f.started)||hex(f.fight_id)||'!'||CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.name') END END) AS name_key,
        max(CASE WHEN CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.class_id') END IS NOT NULL THEN
            printf('%019d',f.started)||hex(f.fight_id)||'!'||CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.class_id') END END) AS class_key,
        max(CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END) AS is_local,sum(p.damage) AS damage,
        min(4294967295,sum(CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.hits') END)) AS hits,
        min(4294967295,sum(CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.deaths') END)) AS deaths
    FROM fight_summaries f CROSS JOIN fight_players p INDEXED BY pd_fight_players_totals
    ON p.fight_id=f.fight_id WHERE f.run_id=?1
        AND CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END IS NOT NULL
    GROUP BY p.player_id
)
SELECT player_id,substr(name_key,instr(name_key,'!')+1),
    CAST(substr(class_key,instr(class_key,'!')+1) AS INTEGER),is_local,damage,hits,deaths
FROM combined";

pub(crate) fn totals(conn: &Connection, run: &RunView) -> Result<Option<RunTotals>, StoreError> {
    let (mut totals, expected_players, missing_unattributed) =
        conn.query_row(TOTALS_SQL, [&run.id], |r| {
            Ok((
                RunTotals {
                    fights: r.get(0)?,
                    kills: r.get(1)?,
                    first_hit: r.get::<_, Option<i64>>(2)?.map(|n| Timestamp(n as u64)),
                    last_hit: r.get::<_, Option<i64>>(3)?.map(|n| Timestamp(n as u64)),
                    total_damage: r.get(4)?,
                    unattributed_damage: r.get(5)?,
                    partial: r.get(6)?,
                    players: Vec::new(),
                    local: None,
                    fame_delta: None,
                },
                r.get::<_, i64>(7)?,
                r.get::<_, bool>(8)?,
            ))
        })?;
    if totals.fights == 0 {
        return Ok(None);
    }
    let cached: i64 = conn.query_row(
        "SELECT coalesce(sum(contributions),0) FROM pd_run_players WHERE run_id=?1",
        [&run.id],
        |r| r.get(0),
    )?;
    let sql = if cached == expected_players {
        "SELECT player_id,name,class_id,is_local>0,damage+damage_correction,min(4294967295,hits),min(4294967295,deaths) FROM pd_run_players WHERE run_id=?1 ORDER BY player_id"
    } else {
        PLAYERS_SQL
    };
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([&run.id])?;
    let mut bytes = 0usize;
    let mut players = std::collections::BTreeMap::<String, RunPlayerTotal>::new();
    while let Some(r) = rows.next()? {
        let player_id: String = r.get(0)?;
        let name: Option<String> = r.get(1)?;
        let class_id: Option<u16> = r.get(2)?;
        if !players.contains_key(&player_id) {
            bytes = bytes
                .checked_add(player_id.len() + 128)
                .filter(|n| *n <= 32 * 1024 * 1024)
                .ok_or(StoreError::Limit)?;
        }
        let player = players
            .entry(player_id.clone())
            .or_insert_with(|| RunPlayerTotal {
                player_id,
                name: None,
                class_id: None,
                is_local: false,
                damage: 0.,
                hits: 0,
                deaths: 0,
                share: None,
                rank: None,
                dps: None,
            });
        if let Some(name) = name {
            bytes = bytes.saturating_sub(player.name.as_ref().map_or(0, String::len));
            bytes = bytes
                .checked_add(name.len())
                .filter(|n| *n <= 32 * 1024 * 1024)
                .ok_or(StoreError::Limit)?;
            player.name = Some(name);
        }
        if class_id.is_some() {
            player.class_id = class_id;
        }
        player.is_local |= r.get::<_, bool>(3)?;
        player.damage += r.get::<_, f64>(4)?;
        player.hits = player.hits.saturating_add(r.get::<_, u32>(5)?);
        player.deaths = player.deaths.saturating_add(r.get::<_, u32>(6)?);
    }
    let mut players: Vec<_> = players.into_values().collect();
    if missing_unattributed {
        totals.unattributed_damage =
            (totals.total_damage - players.iter().map(|p| p.damage).sum::<f64>()).max(0.);
    }
    players.sort_by(|a, b| {
        b.damage
            .total_cmp(&a.damage)
            .then_with(|| a.player_id.cmp(&b.player_id))
    });
    let mut last = None;
    let mut rank = 0;
    for (index, player) in players.iter_mut().enumerate() {
        player.share = maxed_engine::share(player.damage, totals.total_damage);
        player.dps = totals
            .first_hit
            .zip(totals.last_hit)
            .and_then(|(first, last)| maxed_engine::dps(player.damage, first, last));
        if player.damage > 0. {
            if last != Some(player.damage) {
                rank = index as u32 + 1;
                last = Some(player.damage);
            }
            player.rank = Some(rank);
        }
    }
    totals.local = players.iter().find(|p| p.is_local).cloned();
    players.truncate(maxed_engine::RUN_TOTAL_PLAYER_LIMIT);
    if let Some(local) = &totals.local {
        if !players.iter().any(|p| p.player_id == local.player_id) {
            if players.len() == maxed_engine::RUN_TOTAL_PLAYER_LIMIT {
                players.pop();
            }
            players.push(local.clone());
        }
    }
    totals.players = players;
    totals.fame_delta = conn.query_row("WITH samples AS (
        SELECT fame,row_number() OVER (ORDER BY at,stream_id,sequence) AS first,
            row_number() OVER (ORDER BY at DESC,stream_id DESC,sequence DESC) AS last
        FROM fame_samples WHERE character_id=?1 AND at>=?2 AND at<=?3
    ) SELECT CASE WHEN count(*)>=2 THEN max(CASE WHEN last=1 THEN fame END)-max(CASE WHEN first=1 THEN fame END) END FROM samples",
        params![run.character_id,micros(run.started)?,run.ended.map(micros).transpose()?.unwrap_or(i64::MAX)],
        |r| r.get::<_, Option<i64>>(0))?.and_then(|n| i32::try_from(n).ok());
    Ok(Some(totals))
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) fn assert_aggregate_plans(db: &rusqlite::Connection, statements: &[String]) {
        assert_eq!(statements.len(), 4);
        for (sql, index) in statements.iter().zip([
            "fight_summaries_run",
            "sqlite_autoindex_pd_run_players_1",
            "sqlite_autoindex_pd_run_players_1",
            "fame_character",
        ]) {
            let plan = db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            assert!(plan.contains(index), "{plan}");
            assert!(
                !plan.contains("SCAN fights")
                    && !plan.contains("SCAN fight_players")
                    && !plan.contains("SCAN fame_samples"),
                "{plan}"
            );
        }
    }
}

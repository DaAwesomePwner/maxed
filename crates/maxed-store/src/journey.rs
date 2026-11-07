//! Session-local inference from ordered visits; no portal or completion claims.
use super::StoreError;
use maxed_types::{ChainStep, ChainStepKind, RecapChain, Timestamp};
use rusqlite::{params, Connection, OptionalExtension};
use std::ops::Range;

pub(crate) struct VisitRow {
    pub id: String,
    pub account: Option<String>,
    pub kind: String,
    pub map: String,
    pub run_id: Option<String>,
    pub dungeon: Option<String>,
    pub started: i64,
    pub ended: Option<i64>,
}

/// The earliest 2,000 visits, in stable session order. Callers share this bounded
/// sequence for chains and repeat counts; inference cannot extend beyond it.
pub(crate) fn session_visits(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<VisitRow>, StoreError> {
    let mut statement = conn.prepare_cached(
        "SELECT map_visits.id,kind,map,run_id,r.dungeon,
                map_visits.started,map_visits.ended,(SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=CASE WHEN map_visits.run_id IS NOT NULL THEN r.character_id ELSE map_visits.character_id END),CASE WHEN map_visits.run_id IS NOT NULL THEN r.character_id ELSE map_visits.character_id END))
         FROM map_visits INDEXED BY map_visits_session
         LEFT JOIN runs r ON r.id=map_visits.run_id
         WHERE map_visits.session_id=?1
         ORDER BY map_visits.started,map_visits.id LIMIT 2000",
    )?;
    let rows = statement.query_map([session_id], |row| {
        Ok(VisitRow {
            id: row.get(0)?,
            account: row.get(7)?,
            kind: row.get(1)?,
            map: row.get(2)?,
            run_id: row.get(3)?,
            dungeon: row.get(4)?,
            started: row.get(5)?,
            ended: row.get(6)?,
        })
    })?;
    rows.map(|row| row.map_err(StoreError::from)).collect()
}

fn step_kind(visit: &VisitRow) -> Option<ChainStepKind> {
    match (visit.kind.as_str(), visit.dungeon.as_deref()) {
        ("realm", _) => Some(ChainStepKind::Realm),
        ("dungeon", Some("Oryx's Castle")) => Some(ChainStepKind::Castle),
        ("dungeon", Some("Oryx's Chamber" | "Wine Cellar" | "Wine Cellar Short")) => {
            Some(ChainStepKind::Chamber)
        }
        ("dungeon", Some("Oryx's Sanctuary" | "Oryx Mania Sanctuary")) => {
            Some(ChainStepKind::Sanctuary)
        }
        _ => None,
    }
}

pub(crate) fn chain_range(visits: &[VisitRow], index: usize) -> Option<Range<usize>> {
    visits.get(index)?.account.as_ref()?;
    let mut start = 0;
    while start + 1 < visits.len() {
        if visits[start].account.is_some()
            && visits[start].account == visits[start + 1].account
            && visits[start].kind == "realm"
            && step_kind(&visits[start + 1]) == Some(ChainStepKind::Castle)
        {
            let mut end = start + 2;
            while end < visits.len()
                && visits[end].account == visits[start].account
                && matches!(
                    step_kind(&visits[end]),
                    Some(ChainStepKind::Chamber | ChainStepKind::Sanctuary)
                )
            {
                end += 1;
            }
            if (start..end).contains(&index) {
                return Some(start..end);
            }
            start = end;
        } else {
            start += 1;
        }
    }
    None
}

/// Non-dungeon or unresolved visits have no repeat ordinal.
pub(crate) fn in_a_row(visits: &[VisitRow], index: usize) -> u32 {
    let Some(visit) = visits
        .get(index)
        .filter(|v| v.kind == "dungeon" && v.account.is_some())
    else {
        return 0;
    };
    let Some(dungeon) = visit.dungeon.as_deref() else {
        return 0;
    };
    let mut count: u32 = 1;
    for previous in visits[..index].iter().rev() {
        if previous.account != visit.account {
            break;
        }
        match previous.kind.as_str() {
            "nexus" | "other_hub" => continue,
            "dungeon" if previous.dungeon.as_deref() == Some(dungeon) => {
                count = count.saturating_add(1)
            }
            _ => break,
        }
    }
    count
}

pub(crate) fn chain(
    conn: &Connection,
    session_id: &str,
    visits: &[VisitRow],
    range: Range<usize>,
) -> Result<RecapChain, StoreError> {
    let selected = visits.get(range).ok_or(StoreError::InvalidEvent)?;
    let castle_run_id = selected.get(1).and_then(|visit| visit.run_id.as_deref());
    let steps = selected
        .iter()
        .map(|visit| step(conn, session_id, visit, castle_run_id))
        .collect::<Result<_, _>>()?;
    Ok(RecapChain { steps })
}

fn step(
    conn: &Connection,
    session_id: &str,
    visit: &VisitRow,
    castle_run_id: Option<&str>,
) -> Result<ChainStep, StoreError> {
    let kind = step_kind(visit).ok_or(StoreError::InvalidEvent)?;
    let realm = kind == ChainStepKind::Realm;
    let name = if realm {
        castle_realm_name(conn, castle_run_id)?.unwrap_or_else(|| visit.map.clone())
    } else {
        visit.dungeon.clone().ok_or(StoreError::InvalidEvent)?
    };

    // Pin realm lookups to session/time ranges instead of walking runless rows
    // across later sessions. Open visits use the largest stored timestamp bound.
    let (fight_index, fight_scope) = if realm {
        (
            "fights_session",
            "run_id IS NULL AND session_id=?2 AND started>=?3 AND started<?4",
        )
    } else {
        ("fights_run", "run_id=?1")
    };
    let (drop_index, drop_scope) = if realm {
        (
            "drops_session",
            "run_id IS NULL AND session_id=?2 AND at>=?3 AND at<?4",
        )
    } else {
        ("drops_run", "run_id=?1")
    };
    // Realm predicates use parameters 2..4; both variants bind the account at 5.
    let ended = visit.ended.unwrap_or(i64::MAX);
    let bindings = params![
        visit.run_id,
        session_id,
        visit.started,
        ended,
        visit.account
    ];
    let combat_sql = format!(
        "WITH scoped_fights AS MATERIALIZED (SELECT * FROM fights INDEXED BY {fight_index} WHERE {fight_scope} AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=fights.id),(SELECT character_id FROM runs WHERE id=fights.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=fights.id),(SELECT character_id FROM runs WHERE id=fights.run_id))))=?5),
         party_scope(fight_id) AS (SELECT id FROM scoped_fights),
         {},
         spans AS (
             SELECT f.*,CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.first_hit') AS INTEGER) END AS first_hit,
                 CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.last_hit') AS INTEGER) END AS last_hit
             FROM scoped_fights f
         ), eligible AS (
             SELECT local_dps,last_hit-first_hit AS w,p.avg_dps AS party_dps
             FROM spans s LEFT JOIN party_avg p ON p.fight_id=s.id
             WHERE ended IS NOT NULL AND local_dps IS NOT NULL
                 AND first_hit>=started AND last_hit>first_hit AND last_hit<=ended
         )
         SELECT sum(local_dps*w)/sum(w),
             sum(party_dps*w)/sum(CASE WHEN party_dps IS NOT NULL THEN w END)
         FROM eligible",
        include_str!("party_average_scoped.sql"),
    );
    let (local_dps, party_dps): (Option<f64>, Option<f64>) = conn
        .prepare_cached(&combat_sql)?
        .query_row(bindings, |row| Ok((row.get(0)?, row.get(1)?)))?;
    let local_dps = local_dps.filter(|value| value.is_finite());
    let party_dps = party_dps.filter(|value| value.is_finite());
    // Junk bags are excluded upstream by migration 011 and the writer.
    let drops_sql = format!(
        "WITH scoped_drops AS MATERIALIZED (SELECT * FROM drops INDEXED BY {drop_index} WHERE {drop_scope} AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))))=?5)
         SELECT count(*),count(*) FILTER (WHERE bag_rank=255) FROM scoped_drops"
    );
    let (bags, whites): (u32, u32) = conn
        .prepare_cached(&drops_sql)?
        .query_row(bindings, |row| Ok((row.get(0)?, row.get(1)?)))?;
    let items_sql = format!(
        "WITH scoped_drops AS (SELECT * FROM drops INDEXED BY {drop_index} WHERE {drop_scope} AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))))=?5),
         appearances AS (
             SELECT i.item_type,d.bag_rank,d.at,i.slot,d.id,
                 row_number() OVER (PARTITION BY i.item_type ORDER BY d.bag_rank DESC,d.at,i.slot,d.id) AS n
             FROM scoped_drops d JOIN drop_items i ON i.drop_id=d.id
         )
         SELECT item_type FROM appearances
         WHERE n=1
         ORDER BY bag_rank DESC,at,slot,id,item_type LIMIT 4"
    );
    let item_types = conn
        .prepare_cached(&items_sql)?
        .query_map(bindings, |row| row.get(0))?
        .collect::<Result<Vec<u16>, _>>()?;
    let timestamp = |value: i64| {
        u64::try_from(value)
            .map(Timestamp)
            .map_err(|_| StoreError::InvalidEvent)
    };
    Ok(ChainStep {
        visit_id: visit.id.clone(),
        kind,
        name,
        run_id: visit.run_id.clone(),
        started: timestamp(visit.started)?,
        ended: visit.ended.map(timestamp).transpose()?,
        local_dps,
        party_dps,
        bags,
        whites,
        item_types,
    })
}

/// The immediately following Castle run owns the realm entry context. Callers
/// establish adjacency and dungeon kind before passing its id here.
pub(crate) fn castle_realm_name(
    conn: &Connection,
    castle_run_id: Option<&str>,
) -> Result<Option<String>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT CASE WHEN json_valid(json) THEN
             CASE WHEN json_type(json,'$.context.entered_from_realm')='true'
                 AND json_type(json,'$.context.realm_name')='text'
             THEN json_extract(json,'$.context.realm_name') END END
         FROM runs WHERE id=?1",
            [castle_run_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}

//! As-of-run records. Do not use the mutable all-time personal_bests projection.
use crate::{
    alerts::candidate,
    project::MAX_EVENT_BYTES,
    stats::{count, unsigned},
    StoreError,
};
use maxed_types::{BossDpsRecord, DryStreakEnd, FightView, PbClear, RecapMilestone, RunRecap};
use rusqlite::{params, Connection, Params};
use std::collections::{hash_map::Entry, HashMap};

pub(crate) struct RunFacts {
    pub session: String,
    pub dungeon: String,
    pub started: i64,
    pub cleared: bool,
    pub clear_micros: Option<i64>,
    pub has_white: bool,
}

pub(crate) fn run_records(
    conn: &Connection,
    run_id: &str,
    run: &RunFacts,
    chase_types: &[u16],
) -> Result<RunRecap, StoreError> {
    let account: Option<String> = conn.query_row(
        "SELECT (SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=?1)",
        [run_id],
        |row| row.get(0),
    )?;
    let known = account.is_some();
    let (clears, previous_best, dry_runs, had_previous_white): (i64, Option<i64>, i64, bool) = conn
        .prepare_cached(include_str!("records_runs.sql"))?
        .query_row(params![run_id, run.dungeon, run.started], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
    let clear_number = (known && run.cleared)
        .then(|| count(clears + 1))
        .transpose()?;
    let pb_clear = run
        .clear_micros
        .map(unsigned)
        .transpose()?
        .zip(previous_best.map(unsigned).transpose()?)
        .and_then(|(clear, previous)| {
            (clear < previous).then_some(PbClear {
                clear_micros: clear,
                previous_best_micros: previous,
            })
        });
    let dry_streak_end = (known && run.has_white)
        .then(|| -> Result<_, StoreError> {
            Ok(DryStreakEnd {
                runs: count(dry_runs)?,
                had_previous_white,
            })
        })
        .transpose()?;
    let mut milestones = Vec::new();
    match clear_number {
        Some(1) => milestones.push(RecapMilestone::FirstClear),
        Some(n @ (10 | 25 | 50 | 100 | 250 | 500 | 1000)) => {
            milestones.push(RecapMilestone::ClearCount { count: n });
        }
        _ => {}
    }
    if known && run.has_white && !had_previous_white {
        milestones.push(RecapMilestone::FirstWhite);
    }
    if let Some(items) = new_items(conn, run_id, chase_types)? {
        milestones.push(items);
    }
    Ok(RunRecap {
        run_id: run_id.into(),
        clear_number,
        in_a_row: u32::from(known),
        chain: None,
        pb_clear,
        dry_streak_end,
        best_dps: best_dps(conn, include_str!("records_fights.sql"), [run_id])?,
        milestones,
    })
}

fn fight(text: String) -> Result<FightView, StoreError> {
    if text.len() > MAX_EVENT_BYTES {
        return Err(StoreError::Limit);
    }
    Ok(serde_json::from_str(&text)?)
}

pub(crate) fn best_dps(
    conn: &Connection,
    sql: &str,
    args: impl Params,
) -> Result<Vec<BossDpsRecord>, StoreError> {
    let mut statement = conn.prepare_cached(sql)?;
    let mut earlier = conn.prepare_cached(include_str!("records_previous_dps.sql"))?;
    let mut by_id = conn.prepare_cached("SELECT json FROM fights WHERE id=?1")?;
    let mut rows = statement.query(args)?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        let current = fight(row.get(0)?)?;
        if let Some(c) = candidate(conn, &current)? {
            candidates.push((crate::project::micros(current.started)?, c));
        }
    }
    struct PreviousFight {
        id: String,
        started: i64,
        // Outer None is unchecked; inner None is an ineligible baseline.
        dps: Option<Option<f64>>,
    }
    let latest = candidates
        .iter()
        .map(|(started, _)| *started)
        .max()
        .unwrap_or(0);
    let mut cache: HashMap<(String, u16), Vec<PreviousFight>> = HashMap::new();
    let mut records = Vec::new();
    for (started, c) in candidates {
        let previous = match cache.entry((c.character_id.clone(), c.boss_type)) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(
                earlier
                    .query_map(params![c.character_id, c.boss_type, latest], |row| {
                        Ok(PreviousFight {
                            id: row.get(0)?,
                            started: row.get(2)?,
                            dps: None,
                        })
                    })?
                    .collect::<Result<_, _>>()?,
            ),
        };
        // Candidates stay DPS-sorted, but every comparison retains its own time
        // boundary. JSON/eligibility are fetched lazily and checked once per read.
        for previous in previous.iter_mut().filter(|p| p.started < started) {
            let baseline = if let Some(dps) = previous.dps {
                dps
            } else {
                let previous_fight = fight(by_id.query_row([&previous.id], |row| row.get(0))?)?;
                let dps = candidate(conn, &previous_fight)?
                    .filter(|b| b.character_id == c.character_id && b.boss_type == c.boss_type)
                    .map(|b| b.dps);
                previous.dps = Some(dps);
                dps
            };
            let Some(baseline) = baseline else { continue };
            if c.dps > baseline {
                records.push(BossDpsRecord {
                    fight_id: c.fight_id,
                    boss_type: c.boss_type,
                    dps: c.dps,
                    previous_best_dps: baseline,
                });
            }
            break;
        }
    }
    Ok(records)
}

fn new_items(
    conn: &Connection,
    run_id: &str,
    chase_types: &[u16],
) -> Result<Option<RecapMilestone>, StoreError> {
    if chase_types.is_empty() {
        return Ok(None);
    }
    let mut statement = conn.prepare_cached(include_str!("records_items.sql"))?;
    let items = statement.query_map(
        params![run_id, serde_json::to_string(chase_types)?],
        |row| row.get(0),
    )?;
    let mut item_types = Vec::new();
    let mut more = 0;
    for item in items {
        let item = item?;
        if item_types.len() < 8 {
            item_types.push(item);
        } else {
            more += 1;
        }
    }
    Ok((!item_types.is_empty()).then_some(RecapMilestone::NewItems { item_types, more }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::{demo, DpsBasis, Timestamp};
    use std::cell::Cell;

    thread_local! { static SCANS: Cell<usize> = const { Cell::new(0) }; }

    #[test]
    fn repeated_boss_records_scan_once_and_keep_each_fights_prior_best() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO play_sessions VALUES('session',0,100,100,'{}')",
            [],
        )
        .unwrap();
        let mut character = demo::demo_run_story().character.unwrap();
        character.id = "synthetic-self".into();
        character.identity_verified = true;
        conn.execute(
            "INSERT INTO characters(id,observed_at,json) VALUES(?1,0,?2)",
            params![character.id, serde_json::to_string(&character).unwrap()],
        )
        .unwrap();
        for (id, start, dps) in [
            ("baseline", 1, 100.),
            ("first", 10, 200.),
            ("same-start", 10, 150.),
            ("second", 40, 300.),
            ("future", 100, 1000.),
        ] {
            let mut f = demo::demo_snapshot().fights.remove(0);
            f.id = id.into();
            f.run_id = None;
            f.session_id = "session".into();
            f.started = Timestamp(start);
            f.ended = Some(Timestamp(start + 10));
            f.first_hit = Some(Timestamp(start + 1));
            f.last_hit = Some(Timestamp(start + 9));
            f.end_reason = Some(maxed_types::FightEnd::Killed);
            f.boss_type = Some(42);
            f.dps_basis = DpsBasis::PlayerActiveWindowV1;
            f.complete_capture = true;
            f.pending_local_reconciliation = false;
            f.issues.clear();
            f.players.retain(|p| p.is_local);
            f.players[0].player_id = character.id.clone();
            f.players[0].dps = Some(dps);
            f.players[0].damage = dps;
            conn.execute(
                "INSERT INTO fights VALUES(?1,NULL,'session',?2,?2+1,42,?3,1,?4)",
                params![id, start as i64, dps, serde_json::to_string(&f).unwrap()],
            )
            .unwrap();
            conn.execute("INSERT INTO fight_players(fight_id,player_id,object_id,damage,dps,json) VALUES(?1,?2,1,?3,?3,'{}')", params![id,character.id,dps]).unwrap();
        }
        conn.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(|event| {
                if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
                    if statement
                        .expanded_sql()
                        .unwrap()
                        .contains("INDEXED BY fights_final_boss_key")
                    {
                        SCANS.with(|count| count.set(count.get() + 1));
                    }
                }
            }),
        );
        // Separate reads must each scan once; the cache cannot outlive a read.
        for _ in 0..2 {
            SCANS.with(|count| count.set(0));
            let records = best_dps(
                &conn,
                include_str!("visit_records.sql"),
                params!["session", "[[10,50]]"],
            )
            .unwrap();
            assert_eq!(
                records
                    .iter()
                    .map(|r| (r.fight_id.as_str(), r.previous_best_dps))
                    .collect::<Vec<_>>(),
                [("first", 100.), ("same-start", 100.), ("second", 200.)]
            );
            assert_eq!(SCANS.with(Cell::get), 1);
        }
    }
}

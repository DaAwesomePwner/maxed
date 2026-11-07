//! A dungeon's learned boss order and character-specific run baselines.
use crate::{
    queries::identifier,
    stats::{bounds, count, median},
    StoreError, StoreReader,
};
use maxed_types::{DungeonProfile, DungeonProfileRequest, ProfileBoss, StatsFilter, Timestamp};
use rusqlite::params;

impl StoreReader {
    pub fn dungeon_profile(
        &self,
        request: DungeonProfileRequest,
        account: Option<&str>,
    ) -> Result<DungeonProfile, StoreError> {
        identifier(&request.dungeon)?;
        let filter = StatsFilter {
            range: Some(request.range),
            character_id: request.character_id,
        };
        let (lo, hi, character) = bounds(&filter)?;
        // Boss order and both character baselines must see the same history.
        let tx = self.conn.unchecked_transaction()?;
        let (clears, durations): (i64, String) = tx.query_row(
            "SELECT COUNT(*) FILTER (WHERE outcome='cleared'),
                json_group_array(completed_at-started) FILTER (
                  WHERE outcome='cleared' AND completed_at>=started
                    AND (?3 IS NULL OR character_id=?3))
             FROM runs WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=runs.character_id),runs.character_id))=?5 AND started>=?1 AND started<?2 AND dungeon=?4",
            params![lo, hi, character, request.dungeon, account],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut durations: Vec<u64> = serde_json::from_str(&durations)?;
        let clear_samples = u32::try_from(durations.len()).map_err(|_| StoreError::InvalidEvent)?;
        let usual_clear_micros = (clear_samples >= 3)
            .then(|| median(&mut durations))
            .flatten();

        let sql = include_str!("dungeon_profile.sql").replacen(
            "/*fight_character*/",
            include_str!("fight_character.sql"),
            1,
        );
        let mut statement = tx.prepare_cached(&sql)?;
        let mut rows = statement.query(params![lo, hi, character, request.dungeon, account])?;
        let mut bosses = Vec::new();
        while let Some(row) = rows.next()? {
            let mut offsets: Vec<u64> = serde_json::from_str(&row.get::<_, String>(3)?)?;
            let mut durations: Vec<u64> = serde_json::from_str(&row.get::<_, String>(4)?)?;
            let kill_samples =
                u32::try_from(durations.len()).map_err(|_| StoreError::InvalidEvent)?;
            bosses.push(ProfileBoss {
                target_type: row.get(0)?,
                target_name: row.get(1)?,
                seen_in: count(row.get(2)?)?,
                median_offset_micros: median(&mut offsets).ok_or(StoreError::InvalidEvent)?,
                median_kill_micros: (kill_samples >= 3)
                    .then(|| median(&mut durations))
                    .flatten(),
                kill_samples,
                best_dps: row.get(5)?,
            });
        }
        bosses.sort_by_key(|boss| (boss.median_offset_micros, boss.target_type));
        drop(rows);
        drop(statement);

        let mut statement = tx.prepare_cached(include_str!("dungeon_profile_dps.sql"))?;
        let mut rows = statement.query(params![lo, hi, character, request.dungeon, account])?;
        let mut run_dps_samples = 0u32;
        let mut mean = 0.0;
        while let Some(row) = rows.next()? {
            let dps = if row.get::<_, bool>(3)? {
                let first = row.get::<_, Option<i64>>(1)?.map(|n| Timestamp(n as u64));
                let last = row.get::<_, Option<i64>>(2)?.map(|n| Timestamp(n as u64));
                row.get::<_, Option<f64>>(4)?
                    .zip(first.zip(last))
                    .and_then(|(damage, (first, last))| maxed_engine::dps(damage, first, last))
            } else {
                // Missing or mixed-account run caches must aggregate only owned fights.
                let id: String = row.get(0)?;
                account_run_dps(&tx, &id, account)?
            };
            if let Some(dps) = dps {
                run_dps_samples = run_dps_samples
                    .checked_add(1)
                    .ok_or(StoreError::InvalidEvent)?;
                mean += (dps - mean) / f64::from(run_dps_samples);
            }
        }
        drop(rows);
        drop(statement);
        tx.commit()?;
        Ok(DungeonProfile {
            dungeon: request.dungeon,
            clears: count(clears)?,
            bosses,
            usual_run_dps: (run_dps_samples >= 3).then_some(mean),
            run_dps_samples,
            usual_clear_micros,
            clear_samples,
        })
    }
}

fn account_run_dps(
    conn: &rusqlite::Connection,
    run: &str,
    account: Option<&str>,
) -> Result<Option<f64>, StoreError> {
    let sql = include_str!("dungeon_profile_owned_dps.sql");
    let (first, last, damage): (Option<i64>, Option<i64>, Option<f64>) =
        conn.query_row(sql, params![run, account], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
    Ok(damage
        .zip(first.zip(last))
        .and_then(|(damage, (first, last))| {
            maxed_engine::dps(damage, Timestamp(first as u64), Timestamp(last as u64))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::*;
    use std::cell::RefCell;

    thread_local! {
        static STATEMENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn trace(event: rusqlite::trace::TraceEvent<'_>) {
        if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
            STATEMENTS
                .with(|statements| statements.borrow_mut().push(statement.sql().into_owned()));
        }
    }

    #[test]
    fn dungeon_profile_complete_cache_statement_count_is_constant() {
        let mut counts = Vec::new();
        for size in [1, 32] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("synthetic.sqlite");
            let mut db = rusqlite::Connection::open(&path).unwrap();
            crate::migrate(&mut db).unwrap();
            let mut snapshot = demo::demo_snapshot();
            snapshot.character.as_mut().unwrap().account_id = Some(
                "account-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            );
            let mut events = vec![
                EngineEvent::SessionUpdated(snapshot.session.unwrap()),
                EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
            ];
            let run = snapshot.run.unwrap();
            for i in 0..size {
                let mut run = run.clone();
                run.id = format!("synthetic-run-{i}");
                run.ended = Some(Timestamp(run.started.0 + 600_000_000));
                let mut fight = snapshot.fights[0].clone();
                fight.id = format!("synthetic-fight-{i}");
                fight.run_id = Some(run.id.clone());
                events.extend([EngineEvent::RunStarted(run), EngineEvent::FightEnded(fight)]);
            }
            let events: Vec<_> = events
                .into_iter()
                .enumerate()
                .map(|(i, event)| EventEnvelope {
                    stream_id: "synthetic-profile-count".into(),
                    sequence: Sequence(Timestamp(i as u64 + 1)),
                    at: Timestamp(0),
                    fight_chart_omitted: false,
                    event,
                })
                .collect();
            crate::apply_batch(&mut db, &events).unwrap();
            let reader = crate::open_reader(&path).unwrap();
            reader.conn.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                Some(trace),
            );
            STATEMENTS.with(|statements| statements.borrow_mut().clear());
            let result = reader
                .dungeon_profile(
                    DungeonProfileRequest {
                        dungeon: run.dungeon,
                        character_id: run.character_id,
                        range: TimeRange {
                            start: Timestamp(0),
                            end: Timestamp(1_000_000_000_000),
                        },
                    },
                    Some(
                        "account-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                )
                .unwrap();
            assert_eq!(result.run_dps_samples, size);
            let statements = STATEMENTS.with(|statements| statements.take());
            counts.push(statements.len());
        }
        assert_eq!(
            counts[0], counts[1],
            "SQL statement counts for 1 and 32 runs: {counts:?}"
        );
        // BEGIN, clear baselines, boss baselines, run DPS, COMMIT.
        assert_eq!(counts, [5, 5]);
    }
}

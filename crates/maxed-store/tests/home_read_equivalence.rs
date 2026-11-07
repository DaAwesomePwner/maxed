use crate::common::account_scope_fixture::ACCOUNT;
use maxed_types::*;
use rusqlite::params;

#[path = "common/home_workload.rs"]
mod home_workload;

fn median(mut values: Vec<u64>) -> Option<u64> {
    values.sort_unstable();
    let n = values.len();
    (n > 0).then(|| {
        if n % 2 == 1 {
            values[n / 2]
        } else {
            values[n / 2 - 1] + (values[n / 2] - values[n / 2 - 1]) / 2
        }
    })
}

fn legacy_bosses(db: &rusqlite::Connection, request: &BossHistoryRequest) -> BossHistory {
    let sql = include_str!("common/home_boss_history_legacy.sql")
        .replace(
            "/*fight_character*/",
            include_str!("../src/fight_character.sql"),
        )
        .replace(
            "/*party_average_scoped*/",
            include_str!("../src/party_average_scoped.sql"),
        );
    let targets = request
        .target_types
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap());
    let mut stmt = db.prepare(&sql).unwrap();
    let mut rows = stmt
        .query_map(
            params![
                request.range.as_ref().map(|r| r.start.0 as i64),
                request.range.as_ref().map(|r| r.end.0 as i64),
                request.character_id,
                targets
            ],
            |r| {
                let durations: Vec<u64> = serde_json::from_str(&r.get::<_, String>(8)?).unwrap();
                Ok(BossHistoryRow {
                    target_type: r.get(0)?,
                    target_name: r.get(1)?,
                    fights: r.get(2)?,
                    avg_dps: r.get(3)?,
                    best_dps: r.get(4)?,
                    best_fight_id: r.get(5)?,
                    avg_rank: r.get(6)?,
                    avg_party_dps: r.get(7)?,
                    kills: durations.len() as u32,
                    median_kill_micros: median(durations),
                })
            },
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let truncated = rows.len() > MAX_STATS_ROWS;
    rows.truncate(MAX_STATS_ROWS);
    BossHistory { rows, truncated }
}

fn legacy_profile(db: &rusqlite::Connection, request: &DungeonProfileRequest) -> Vec<ProfileBoss> {
    let sql = include_str!("common/home_dungeon_profile_legacy.sql").replace(
        "/*fight_character*/",
        include_str!("../src/fight_character.sql"),
    );
    let mut stmt = db.prepare(&sql).unwrap();
    let mut rows = stmt
        .query_map(
            params![
                request.range.start.0 as i64,
                request.range.end.0 as i64,
                request.character_id,
                request.dungeon
            ],
            |r| {
                let offsets: Vec<u64> = serde_json::from_str(&r.get::<_, String>(3)?).unwrap();
                let durations: Vec<u64> = serde_json::from_str(&r.get::<_, String>(4)?).unwrap();
                let kill_samples = durations.len() as u32;
                Ok(ProfileBoss {
                    target_type: r.get(0)?,
                    target_name: r.get(1)?,
                    seen_in: r.get(2)?,
                    median_offset_micros: median(offsets).unwrap(),
                    median_kill_micros: (kill_samples >= 3).then(|| median(durations)).flatten(),
                    kill_samples,
                    best_dps: r.get(5)?,
                })
            },
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    rows.sort_by_key(|r| (r.median_offset_micros, r.target_type));
    rows
}

fn mixed() -> home_workload::Fixture {
    let fixture = home_workload::Fixture::build(6);
    // All are synthetic projection edge cases: malformed JSON, a kill-only type,
    // equal-DPS ties and a negative duration. The normal workload also includes
    // other characters, no local DPS, unended fights and provisional kills.
    fixture.db.execute_batch("UPDATE fights SET json='malformed' WHERE id='synthetic-fight-00423';
        UPDATE fights SET local_dps=NULL,json=json_set(json,'$.target_type',60001,'$.kill.provisional',0,'$.kill.at',CAST(started+30000000+(id='synthetic-fight-00425') AS TEXT)) WHERE id IN ('synthetic-fight-00424','synthetic-fight-00425');
        UPDATE fights SET local_dps=5555,started=1700259260000000,json=json_set(json,'$.target_type',60002,'$.started','1700259260000000') WHERE id IN ('synthetic-fight-00426','synthetic-fight-00427');
        UPDATE fights SET json=json_set(json,'$.kill.at',CAST(started-1 AS TEXT),'$.kill.provisional',0) WHERE id='synthetic-fight-00428';").unwrap();
    // Learning must include kills outside the fight-start window when their run
    // starts inside it. Performance must still include other dungeons and realm visits.
    let end = fixture.range.end.0 as i64;
    fixture.db.execute("UPDATE fights SET started=?1,ended=?1+30000000,json=json_set(json,'$.started',CAST(?1 AS TEXT),'$.kill.at',CAST(?1+30000000 AS TEXT),'$.kill.provisional',0) WHERE id='synthetic-fight-00449'", [end]).unwrap();
    fixture.db.execute_batch("UPDATE fights SET local_dps=9900,ended=started+30000000,json=json_set(json,'$.target_type',0) WHERE id='synthetic-fight-00429';
        UPDATE fights SET local_dps=9999,ended=started+30000000,json=json_set(json,'$.target_type',0) WHERE id='synthetic-fight-00420';").unwrap();
    fixture
}

#[test]
fn boss_history_matches_legacy_on_mixed_distributed_history() {
    let fixture = mixed();
    let reader = fixture.reader();
    for range in [None, Some(fixture.range.clone())] {
        for character_id in [
            None,
            Some(home_workload::CHARACTER.into()),
            Some("synthetic-character-1".into()),
        ] {
            for target_types in [None, Some(vec![]), Some(vec![0, 1, 60001, 60002])] {
                let request = BossHistoryRequest {
                    range: range.clone(),
                    character_id: character_id.clone(),
                    target_types,
                };
                assert_eq!(
                    reader.boss_history(request.clone()).unwrap(),
                    legacy_bosses(&fixture.db, &request),
                    "{request:?}"
                );
            }
        }
    }
    let result = reader
        .boss_history(BossHistoryRequest {
            target_types: Some(vec![60001]),
            ..fixture.boss_request(1)
        })
        .unwrap();
    assert_eq!(result.rows[0].fights, 0);
    assert_eq!(
        (result.rows[0].kills, result.rows[0].median_kill_micros),
        (2, Some(30_000_000))
    );
}

#[test]
fn dungeon_profile_matches_legacy_on_mixed_distributed_history() {
    let fixture = mixed();
    let reader = fixture.reader();
    for character_id in [
        None,
        Some(home_workload::CHARACTER.into()),
        Some("synthetic-character-1".into()),
    ] {
        for dungeon in [
            home_workload::DUNGEON,
            "Synthetic Dungeon 1",
            "Synthetic Missing",
        ] {
            let request = DungeonProfileRequest {
                character_id: character_id.clone(),
                dungeon: dungeon.into(),
                ..fixture.profile_request()
            };
            assert_eq!(
                reader
                    .dungeon_profile(request.clone(), Some(ACCOUNT))
                    .unwrap()
                    .bosses,
                legacy_profile(&fixture.db, &request),
                "{request:?}"
            );
        }
    }
    let profile = reader
        .dungeon_profile(fixture.profile_request(), Some(ACCOUNT))
        .unwrap();
    assert!(!profile.bosses.is_empty());
    assert_eq!(
        profile
            .bosses
            .iter()
            .find(|b| b.target_type == 0)
            .unwrap()
            .best_dps,
        Some(9999.0)
    );
}

#[test]
fn home_boss_baselines_match_legacy_fields_in_every_filter_variant() {
    let fixture = mixed();
    let reader = fixture.reader();
    for range in [None, Some(fixture.range.clone())] {
        for character_id in [
            None,
            Some(home_workload::CHARACTER.into()),
            Some("synthetic-character-1".into()),
        ] {
            for target_types in [
                None,
                Some(vec![]),
                Some(vec![0, 1, 60001, 60002]),
                Some((0..64).collect()),
            ] {
                let request = BossHistoryRequest {
                    range: range.clone(),
                    character_id: character_id.clone(),
                    target_types,
                };
                let legacy = legacy_bosses(&fixture.db, &request);
                let result = reader
                    .home_boss_baselines(request.clone(), Some(ACCOUNT))
                    .unwrap();
                assert_eq!(result.truncated, legacy.truncated);
                assert_eq!(
                    result.rows,
                    legacy
                        .rows
                        .into_iter()
                        .map(|row| HomeBossBaseline {
                            target_type: row.target_type,
                            fights: row.fights,
                            avg_dps: row.avg_dps,
                            best_dps: row.best_dps,
                            kill_samples: row.kills,
                            median_kill_micros: row.median_kill_micros,
                        })
                        .collect::<Vec<_>>(),
                    "{request:?}"
                );
            }
        }
    }
}

#[test]
fn home_boss_baselines_match_current_boss_history_in_every_filter_variant() {
    let fixture = mixed();
    // Keep legacy-equivalence fixtures unchanged. Non-binary fractions exercise
    // the different accumulation orders of the lean and full queries.
    fixture
        .db
        .execute("UPDATE fights SET local_dps=local_dps/7.0", [])
        .unwrap();
    let reader = fixture.reader();
    for range in [None, Some(fixture.range.clone())] {
        for character_id in [
            None,
            Some(home_workload::CHARACTER.into()),
            Some("synthetic-missing-character".into()),
        ] {
            for target_types in [None, Some(vec![]), Some(vec![0, 1, 60001, 60002])] {
                let request = BossHistoryRequest {
                    range: range.clone(),
                    character_id: character_id.clone(),
                    target_types,
                };
                let full = reader.boss_history(request.clone()).unwrap();
                let lean = reader
                    .home_boss_baselines(request.clone(), Some(ACCOUNT))
                    .unwrap();
                assert_eq!(lean.truncated, full.truncated, "{request:?}");
                assert_eq!(lean.rows.len(), full.rows.len(), "{request:?}");
                if request.target_types.as_ref().is_none_or(|v| !v.is_empty())
                    && request.character_id.as_deref() != Some("synthetic-missing-character")
                {
                    assert!(
                        full.rows
                            .iter()
                            .any(|row| row.best_dps.is_some_and(|dps| dps.fract() != 0.0)),
                        "expected fractional local DPS: {request:?}"
                    );
                }
                // Zip preserves the query's row order, including ties by target type.
                for (actual, expected) in lean.rows.iter().zip(&full.rows) {
                    assert_eq!(
                        (
                            actual.target_type,
                            actual.fights,
                            actual.best_dps,
                            actual.kill_samples,
                            actual.median_kill_micros,
                        ),
                        (
                            expected.target_type,
                            expected.fights,
                            expected.best_dps,
                            expected.kills,
                            expected.median_kill_micros,
                        ),
                        "{request:?}"
                    );
                    match (actual.avg_dps, expected.avg_dps) {
                        (Some(actual_avg), Some(expected_avg)) => {
                            let tolerance = 1e-12 * actual_avg.abs().max(expected_avg.abs());
                            assert!(
                                (actual_avg - expected_avg).abs() <= tolerance,
                                "target {}: avg_dps {actual_avg} != {expected_avg}, tolerance {tolerance}: {request:?}",
                                actual.target_type
                            );
                        }
                        _ => assert_eq!(actual.avg_dps, expected.avg_dps, "{request:?}"),
                    }
                }
            }
        }
    }
}

#[test]
fn home_boss_baselines_validate_filters() {
    let (_dir, path, _db) = crate::common::stats::database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let request = BossHistoryRequest {
        range: None,
        character_id: None,
        target_types: None,
    };
    for invalid in [
        BossHistoryRequest {
            target_types: Some((0..65).collect()),
            ..request.clone()
        },
        BossHistoryRequest {
            character_id: Some("synthetic\u{7}".into()),
            ..request.clone()
        },
        BossHistoryRequest {
            range: Some(TimeRange {
                start: Timestamp(1),
                end: Timestamp(1),
            }),
            ..request.clone()
        },
        BossHistoryRequest {
            range: Some(TimeRange {
                start: Timestamp(0),
                end: Timestamp(i64::MAX as u64 + 1),
            }),
            ..request.clone()
        },
    ] {
        assert!(reader.home_boss_baselines(invalid, Some(ACCOUNT)).is_err());
    }
    assert!(reader
        .home_boss_baselines(request, Some(ACCOUNT))
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn account_scope_large_history_home_hud_bests_streak_plans() {
    use rusqlite::types::Value;
    let f = home_workload::Fixture::build(90); // 12,600 fights across the existing distributed fixture.
    let scope = include_str!("../src/home_scope.sql");
    let home = include_str!("../src/home.sql")
        .replace("/*home_scope*/", scope)
        .replace(
            "/*dungeon_stats*/",
            include_str!("../src/dungeon_stats.sql"),
        );
    let hud = include_str!("../src/home_hud.sql")
        .replace("/*home_scope*/", scope)
        .replace(
            "/*party_average_scoped*/",
            include_str!("../src/party_average_scoped.sql"),
        );
    let base = vec![
        Value::Integer(1),
        Value::Null,
        Value::Integer(f.range.start.0 as i64),
        Value::Integer(f.range.end.0 as i64),
        Value::Text(ACCOUNT.into()),
    ];
    let mut hud_args = base.clone();
    hud_args.extend([Value::Text("[]".into()), Value::Text("[]".into())]);
    for (name, sql, args, indexes) in [
        (
            "Home",
            home,
            base,
            vec![
                "pd_home_today_runs",
                "runs_character",
                "fights_started",
                "drops_at",
            ],
        ),
        (
            "HUD",
            hud,
            hud_args,
            vec!["fights_started", "drops_at", "identity_account"],
        ),
        (
            "bests",
            include_str!("../src/dungeon_bests.sql").into(),
            vec![Value::Text(ACCOUNT.into())],
            vec!["pd_run_evidence_kind"],
        ),
        (
            "streak",
            include_str!("../src/home_streak.sql").into(),
            vec![
                Value::Text(home_workload::DUNGEON.into()),
                Value::Text(ACCOUNT.into()),
            ],
            vec!["runs_loot_stats", "drops_white_runs"],
        ),
    ] {
        let plan =
            f.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
        eprintln!("account_scope {name}:\n{plan}");
        for index in indexes {
            assert!(plan.contains(index), "{name} missing {index}:\n{plan}");
        }
        for view in [
            "run_accounts",
            "fight_accounts",
            "drop_accounts",
            "character_accounts",
        ] {
            assert!(
                !plan.contains(&format!("MATERIALIZE {view}")),
                "{name}:\n{plan}"
            );
        }
        // Range reads and streaks still seek their base rows. Best clear ranking
        // retains its pre-existing all-time evidence walk.
        if name != "bests" {
            for alias in ["fights", "drops", "runs", "n"] {
                assert!(
                    !plan.lines().any(|line| line == format!("SCAN {alias}")),
                    "{name}:\n{plan}"
                );
            }
        }
    }
}

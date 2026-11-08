use crate::common::{envelope, story_events};
use maxed_store::{apply_batch, migrate, open_reader, StoreError};
use maxed_types::*;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn database() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    db
}

#[test]
fn first_gear_row_uses_first_seen_and_swaps_use_last_seen() {
    let mut db = database();
    apply_batch(
        &mut db,
        &[envelope(8, EngineEvent::PlayerObserved(profile(1, 500)))],
    )
    .unwrap();
    apply_batch(
        &mut db,
        &[envelope(9, EngineEvent::PlayerObserved(profile(2, 900)))],
    )
    .unwrap();
    let times: Vec<i64> = db
        .prepare("SELECT at FROM player_gear_log ORDER BY at")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(times, [100, 900]);
}

fn profile(item: u16, at: u64) -> PlayerProfileView {
    PlayerProfileView {
        maxed_gated: false,
        scope_id: "scope-a".into(),
        run_id: Some("demo-run".into()),
        player_id: "synthetic-player".into(),
        object_id: 101,
        is_local: false,
        name: Some("Synthetic Player".into()),
        class_id: Some(782),
        level: Some(20),
        stars: None,
        fame: None,
        skin_id: None,
        guild: None,
        stats: vec![],
        maxed: None,
        maxed_stats: vec![],
        equipment: vec![EquipmentView {
            observed_empty: None,
            slot: 0,
            item_type: Some(item),
            enchant: EnchantView {
                state: EnchantState::Missing,
                slots: None,
                applied: None,
                ids: vec![],
            },
        }],
        first_seen: Timestamp(100),
        last_seen: Timestamp(at),
    }
}

fn timeline(players: &[(&str, Vec<f64>)], full: bool) -> ScopeTimelineUpdate {
    ScopeTimelineUpdate {
        scope_id: "scope-a".into(),
        run_id: Some("demo-run".into()),
        buckets: vec![ScopeBucket {
            from: Timestamp(100),
            until: Timestamp(200),
            party_total: 30.,
            party_active: 2,
        }],
        players: players
            .iter()
            .map(|(id, points)| PlayerSeries {
                player_id: (*id).into(),
                object_id: 101,
                is_local: false,
                points: points.clone(),
            })
            .collect(),
        full,
        compacted: false,
        disabled: false,
    }
}

fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn stored_json(db: &Connection, table: &str, player: Option<&str>) -> Value {
    let text: String = if let Some(player) = player {
        db.query_row(
            &format!("SELECT json FROM {table} WHERE scope_id='scope-a' AND player_id=?1"),
            [player],
            |r| r.get(0),
        )
        .unwrap()
    } else {
        db.query_row(
            &format!("SELECT json FROM {table} WHERE scope_id='scope-a'"),
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    serde_json::from_str(&text).unwrap()
}

#[test]
fn migration_013_from_012_and_fresh() {
    let scripts = [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_fight_summaries.sql"),
        include_str!("../migrations/003_loot_and_sessions.sql"),
        include_str!("../migrations/004_alerts.sql"),
        include_str!("../migrations/005_instance.sql"),
        include_str!("../migrations/006_summary_target_kinds.sql"),
        include_str!("../migrations/007_pb_lookup_indexes.sql"),
        include_str!("../migrations/008_loot_browsing.sql"),
        include_str!("../migrations/009_chat.sql"),
        include_str!("../migrations/010_map_visits.sql"),
        include_str!("../migrations/011_exclude_junk_bags.sql"),
        include_str!("../migrations/012_event_fts.sql"),
    ];
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        if legacy {
            for (i, script) in scripts.iter().enumerate() {
                db.execute_batch(script).unwrap();
                db.execute(
                    "INSERT INTO schema_migrations VALUES(?1,?2)",
                    params![
                        i as i64 + 1,
                        format!("{:x}", Sha256::digest(script.as_bytes()))
                    ],
                )
                .unwrap();
            }
            db.pragma_update(None, "user_version", 12).unwrap();
            crate::common::apply_legacy_story(&mut db, &story_events());
        }
        migrate(&mut db).unwrap();
        assert_eq!(
            open_reader(&path).unwrap().user_version().unwrap(),
            maxed_store::SCHEMA_VERSION
        );
        for table in [
            "player_profiles",
            "player_gear_log",
            "scope_timelines",
            "player_timelines",
        ] {
            assert_eq!(count(&db, table), 0);
        }
        for (i, script) in scripts.iter().enumerate() {
            let checksum: String = db
                .query_row(
                    "SELECT checksum FROM schema_migrations WHERE version=?1",
                    [i as i64 + 1],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(checksum, format!("{:x}", Sha256::digest(script.as_bytes())));
        }
        migrate(&mut db).unwrap();
        assert_eq!(count(&db, "schema_migrations"), maxed_store::SCHEMA_VERSION);
        assert_eq!(count(&db, "runs"), i64::from(legacy));
    }
}

#[test]
fn profile_upsert_and_gear_log_diff() {
    let mut db = database();
    let mut latest = profile(2, 300);
    latest.name = None;
    latest.stats = vec![StatView {
        stat_id: 20,
        value: 75,
    }];
    let events = [profile(1, 100), profile(1, 200), latest.clone()]
        .into_iter()
        .enumerate()
        .map(|(i, p)| envelope(8 + i as u64, EngineEvent::PlayerObserved(p)))
        .collect::<Vec<_>>();
    apply_batch(&mut db, &events).unwrap();
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(count(&db, "player_profiles"), 1);
    assert_eq!(count(&db, "player_gear_log"), 2);
    assert_eq!(
        stored_json(&db, "player_profiles", Some(&latest.player_id)),
        json!(latest)
    );
    let rows = db
        .prepare("SELECT at,json FROM player_gear_log ORDER BY at")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), [100, 300]);
    assert_eq!(
        serde_json::from_str::<Value>(&rows[1].1).unwrap(),
        json!(latest.equipment)
    );
    let scalars: (i32, bool, String, i64, i64) = db
        .query_row(
            "SELECT object_id,is_local,run_id,first_seen,last_seen FROM player_profiles",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(scalars, (101, false, "demo-run".into(), 100, 300));
}

#[test]
fn gear_log_caps_at_64_keeping_latest() {
    let mut db = database();
    let events = (1..=70)
        .map(|i| {
            envelope(
                7 + i,
                EngineEvent::PlayerObserved(profile(i as u16, i * 100)),
            )
        })
        .collect::<Vec<_>>();
    apply_batch(&mut db, &events).unwrap();
    let rows = db
        .prepare("SELECT at,json_extract(json,'$[0].item_type') FROM player_gear_log ORDER BY at")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let expected = (1..=63)
        .chain([70])
        .map(|i| (i * 100, i))
        .collect::<Vec<_>>();
    assert_eq!(rows, expected);
}

#[test]
fn timeline_full_update_replaces_rows() {
    let mut db = database();
    let first = timeline(&[("a", vec![10.]), ("b", vec![20.])], false);
    apply_batch(
        &mut db,
        &[envelope(8, EngineEvent::ScopeTimelineUpdated(first))],
    )
    .unwrap();
    let mut partial = timeline(&[("a", vec![10., 5.])], false);
    partial.buckets.push(ScopeBucket {
        from: Timestamp(200),
        until: Timestamp(300),
        party_total: 5.,
        party_active: 1,
    });
    apply_batch(
        &mut db,
        &[envelope(
            9,
            EngineEvent::ScopeTimelineUpdated(partial.clone()),
        )],
    )
    .unwrap();
    assert_eq!(count(&db, "player_timelines"), 2);
    assert_eq!(
        stored_json(&db, "player_timelines", Some("b")),
        json!({"object_id":101,"is_local":false,"points":[20.]})
    );
    assert_eq!(
        stored_json(&db, "scope_timelines", None),
        json!({"buckets":partial.buckets,"compacted":false,"disabled":false})
    );
    partial.full = true;
    partial.compacted = true;
    apply_batch(
        &mut db,
        &[envelope(10, EngineEvent::ScopeTimelineUpdated(partial))],
    )
    .unwrap();
    assert_eq!(count(&db, "player_timelines"), 1);
    assert_eq!(
        stored_json(&db, "player_timelines", Some("a"))["points"],
        json!([10., 5.])
    );
    assert_eq!(stored_json(&db, "scope_timelines", None)["compacted"], true);
}

#[test]
fn disabled_timeline_clears_only_its_scope() {
    let mut db = database();
    let initial = timeline(&[("a", vec![10.])], false);
    let mut other = initial.clone();
    other.scope_id = "scope-b".into();
    let mut disabled = timeline(&[], false);
    disabled.buckets.clear();
    disabled.compacted = true;
    disabled.disabled = true;
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::ScopeTimelineUpdated(initial)),
            envelope(9, EngineEvent::ScopeTimelineUpdated(other)),
            envelope(10, EngineEvent::ScopeTimelineUpdated(disabled)),
        ],
    )
    .unwrap();
    assert_eq!(count(&db, "player_timelines"), 1);
    assert_eq!(
        stored_json(&db, "scope_timelines", None),
        json!({"buckets":[],"compacted":true,"disabled":true})
    );
    let scope: String = db
        .query_row("SELECT scope_id FROM player_timelines", [], |r| r.get(0))
        .unwrap();
    assert_eq!(scope, "scope-b");
}

#[test]
fn profile_without_run_row_does_not_fail_batch() {
    let mut db = database();
    let mut missing = profile(1, 100);
    missing.run_id = Some("unobserved-run".into());
    let mut runless = missing.clone();
    runless.scope_id = "scope-b".into();
    runless.run_id = None;
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::PlayerObserved(missing.clone())),
            envelope(9, EngineEvent::PlayerObserved(runless)),
        ],
    )
    .unwrap();
    assert_eq!(count(&db, "player_profiles"), 2);
    assert_eq!(
        stored_json(&db, "player_profiles", Some(&missing.player_id)),
        json!(missing)
    );
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}

#[test]
fn character_merge_rekeys_player_rows() {
    let mut db = database();
    let mut verified = demo::demo_run_story().character.unwrap();
    verified.id = "verified-character".into();
    let mut provisional = profile(1, 100);
    provisional.player_id = "demo-character".into();
    provisional.is_local = true;
    let mut collision = provisional.clone();
    collision.player_id = verified.id.clone();
    collision.object_id = 202;
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::CharacterUpdated(verified)),
            envelope(9, EngineEvent::PlayerObserved(provisional)),
            envelope(10, EngineEvent::PlayerObserved(collision)),
            envelope(
                11,
                EngineEvent::ScopeTimelineUpdated(timeline(
                    &[
                        ("demo-character", vec![10.]),
                        ("verified-character", vec![20.]),
                    ],
                    false,
                )),
            ),
            envelope(
                12,
                EngineEvent::CharacterMerged {
                    provisional_id: "demo-character".into(),
                    verified_id: "verified-character".into(),
                },
            ),
        ],
    )
    .unwrap();
    for table in ["player_profiles", "player_gear_log", "player_timelines"] {
        assert_eq!(count(&db, table), 1);
        let id: String = db
            .query_row(&format!("SELECT player_id FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(id, "verified-character");
    }
    assert_eq!(
        stored_json(&db, "player_profiles", Some("verified-character"))["player_id"],
        "verified-character"
    );
    assert_eq!(
        stored_json(&db, "player_profiles", Some("verified-character"))["object_id"],
        101
    );
    assert_eq!(
        stored_json(&db, "player_timelines", Some("verified-character"))["points"],
        json!([10.])
    );
}

fn reader_database() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    (dir, path, db)
}

fn run_profile(player_id: &str, item: u16, at: u64) -> PlayerProfileView {
    let mut p = profile(item, at);
    p.scope_id = "demo-run".into();
    p.player_id = player_id.into();
    p
}

fn notice(id: &str, at: u64, kind: InstanceEventKind) -> EngineEvent {
    EngineEvent::InstanceEvent(InstanceEventView {
        id: id.into(),
        session_id: "demo-session".into(),
        run_id: Some("demo-run".into()),
        map: None,
        at: Timestamp(at),
        kind,
        provisional: false,
    })
}

fn death_notice(id: Option<&str>, object_id: i32) -> InstanceEventKind {
    InstanceEventKind::PlayerDeath {
        player_id: id.map(str::to_owned),
        player_name: "Synthetic Player".into(),
        level: None,
        killed_by: None,
        grave_type: None,
        object_id: Some(object_id),
        class_id: None,
        is_local: false,
    }
}

#[test]
fn run_player_sums_sources_across_fights() {
    let (_dir, path, mut db) = reader_database();
    for index in 0..2 {
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.id = format!("source-fight-{index}");
        fight.started = Timestamp(5_000_000 + index * 2_000_000);
        fight.first_hit = Some(fight.started);
        fight.last_hit = Some(Timestamp(fight.started.0 + 1_000_000));
        let p = &mut fight.players[0];
        p.sources[0].damage = 100. + index as f64 * 50.;
        p.sources[0].hits = 2 + index as u32;
        p.sources[0].biggest_hit = Some(60. + index as f64 * 20.);
        p.sources[0].reasons = if index == 0 {
            vec!["b".into(), "a".into()]
        } else {
            vec!["a".into(), "b".into()]
        };
        let mut inferred = p.sources[0].clone();
        inferred.estimated = true;
        inferred.reasons = vec![INFERRED_EQUIPPED_WEAPON.into()];
        inferred.damage = 30.;
        inferred.hits = 1;
        inferred.biggest_hit = Some(30.);
        p.sources.push(inferred);
        apply_batch(
            &mut db,
            &[envelope(8 + index, EngineEvent::FightStarted(fight))],
        )
        .unwrap();
    }
    // Exclude the unrelated original demo fight's source group.
    db.execute("DELETE FROM damage_sources WHERE fight_id='demo-fight'", [])
        .unwrap();
    let reader = open_reader(&path).unwrap();
    let result = reader
        .get_run_player("demo-run", "demo-character")
        .unwrap()
        .unwrap();
    assert_eq!(result.sources.len(), 2);
    assert_eq!(
        (
            result.sources[0].damage,
            result.sources[0].hits,
            result.sources[0].biggest_hit
        ),
        (250., 5, Some(80.))
    );
    assert!(!result.sources[0].estimated);
    assert_eq!(result.sources[0].reasons, ["a", "b"]);
    assert_eq!(
        (
            result.sources[1].damage,
            result.sources[1].hits,
            result.sources[1].biggest_hit
        ),
        (60., 2, Some(30.))
    );
    assert!(result.sources[1].estimated);
    let totals = reader.get_run("demo-run").unwrap().unwrap().totals.unwrap();
    assert_eq!(result.player, totals.local);
    assert_eq!(result.partial, totals.partial);
    assert_eq!(result.party.total_damage, totals.total_damage);
    assert_eq!(result.party.average_dps, totals.local.as_ref().unwrap().dps);
    assert_eq!(
        result.party.top_player_id.as_deref(),
        Some("demo-character")
    );
    assert_eq!(result.party.players_with_damage, 1);
}

#[test]
fn run_player_without_damage_has_profile_and_roster() {
    let (_dir, path, mut db) = reader_database();
    let mut p = run_profile("observer", 1, 100);
    p.equipment.clear();
    apply_batch(
        &mut db,
        &[envelope(8, EngineEvent::PlayerObserved(p.clone()))],
    )
    .unwrap();
    let result = open_reader(&path)
        .unwrap()
        .get_run_player("demo-run", "observer")
        .unwrap()
        .unwrap();
    assert_eq!(result.profile, Some(p));
    assert!(result.player.is_none());
    assert!(result.sources.is_empty());
    assert!(result.fights[0].damage.is_none());
    assert_eq!(
        result
            .roster
            .iter()
            .map(|r| (r.player_id.as_str(), r.damage))
            .collect::<Vec<_>>(),
        [("demo-character", Some(300.)), ("observer", None)]
    );
    assert_eq!(result.gear_log.len(), 1);
    assert!(result.gear_log[0].equipment.is_empty());
}

#[test]
fn run_player_fights_preserve_current_and_legacy_dps_basis() {
    let (_dir, path, db) = reader_database();
    let reader = open_reader(&path).unwrap();
    for (basis, expected) in [
        (
            Some("player_active_window_v1"),
            DpsBasis::PlayerActiveWindowV1,
        ),
        (Some("whole_target_v0"), DpsBasis::WholeTargetV0),
        (None, DpsBasis::WholeTargetV0),
    ] {
        if let Some(basis) = basis {
            db.execute(
                "UPDATE fights SET json=json_set(json,'$.dps_basis',?1)",
                [basis],
            )
            .unwrap();
        } else {
            db.execute("UPDATE fights SET json=json_remove(json,'$.dps_basis')", [])
                .unwrap();
        }
        let result = reader
            .get_run_player("demo-run", "demo-character")
            .unwrap()
            .unwrap();
        assert_eq!(result.fights[0].dps_basis, expected);
        assert_eq!(result.fights[0].dps, Some(150.));
    }
}

#[test]
fn run_player_fight_object_id_requires_unique_row() {
    let (_dir, path, db) = reader_database();
    let reader = open_reader(&path).unwrap();
    let first = reader
        .get_run_player("demo-run", "demo-character")
        .unwrap()
        .unwrap();
    assert_eq!(first.fights[0].object_id, Some(1));
    assert_eq!(first.fights[0].dps, Some(150.));
    assert_eq!(first.fights[0].deaths, Some(0));
    db.execute("INSERT INTO fight_players SELECT fight_id,'collision',object_id,damage,dps,rank,json FROM fight_players", []).unwrap();
    let ambiguous = reader
        .get_run_player("demo-run", "demo-character")
        .unwrap()
        .unwrap();
    assert_eq!(ambiguous.fights[0].object_id, None);
    assert_eq!(ambiguous.fights[0].damage, Some(300.));
    assert_eq!(
        reader
            .get_run_player("demo-run", "missing")
            .unwrap()
            .unwrap()
            .fights[0]
            .object_id,
        None
    );
}

#[test]
fn run_player_legacy_run_has_no_profile_or_timeline() {
    let (_dir, path, db) = reader_database();
    db.execute("DELETE FROM pd_player_contributions", [])
        .unwrap();
    let reader = open_reader(&path).unwrap();
    let result = reader
        .get_run_player("demo-run", "demo-character")
        .unwrap()
        .unwrap();
    assert!(result.profile.is_none());
    assert!(result.timeline.is_none());
    assert!(result.gear_log.is_empty());
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.fights.len(), 1);
    assert_eq!(result.player.unwrap().damage, 300.);
    assert_eq!(result.roster[0].player_id, "demo-character");
    assert!(reader
        .get_run_player("absent", "demo-character")
        .unwrap()
        .is_none());
    assert!(reader
        .get_player_context("absent", "demo-character", None, None)
        .unwrap()
        .is_none());
}

#[test]
fn run_player_deaths_and_departure() {
    let (_dir, path, mut db) = reader_database();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.ended = Some(Timestamp(4_000_000));
    let mut local = run_profile("demo-character", 1, 5_000_000);
    local.is_local = true;
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::FightEnded(fight)),
            envelope(
                9,
                EngineEvent::PlayerObserved(run_profile("other", 1, 5_000_000)),
            ),
            envelope(10, EngineEvent::PlayerObserved(local)),
            envelope(
                11,
                EngineEvent::PlayerDied {
                    at: Timestamp(3_000_000),
                    character_id: Some("demo-character".into()),
                    run_id: Some("demo-run".into()),
                },
            ),
            envelope(
                12,
                notice("matched", 4_000_000, death_notice(Some("other"), 101)),
            ),
            envelope(13, notice("unmatched", 3_000_000, death_notice(None, 101))),
            envelope(
                14,
                notice("outside", 5_000_000, death_notice(Some("other"), 101)),
            ),
            envelope(
                15,
                notice(
                    "left-early",
                    5_000_000,
                    InstanceEventKind::PlayerLeft {
                        object_id: 101,
                        player_name: None,
                        class_id: None,
                    },
                ),
            ),
            envelope(
                16,
                notice(
                    "left-latest",
                    6_000_000,
                    InstanceEventKind::PlayerLeft {
                        object_id: 101,
                        player_name: None,
                        class_id: None,
                    },
                ),
            ),
            envelope(
                17,
                notice(
                    "wrong-object",
                    7_000_000,
                    InstanceEventKind::PlayerLeft {
                        object_id: 102,
                        player_name: None,
                        class_id: None,
                    },
                ),
            ),
            envelope(
                18,
                EngineEvent::PlayerDied {
                    at: Timestamp(4_000_000),
                    character_id: None,
                    run_id: Some("demo-run".into()),
                },
            ),
            envelope(
                19,
                notice(
                    "local-attributed",
                    3_000_000,
                    death_notice(Some("demo-character"), 101),
                ),
            ),
            envelope(
                20,
                notice(
                    "local-null-character",
                    4_000_000,
                    death_notice(Some("demo-character"), 101),
                ),
            ),
        ],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let local = reader
        .get_run_player("demo-run", "demo-character")
        .unwrap()
        .unwrap();
    assert_eq!(
        local.deaths,
        [
            PlayerDeathAt {
                at: Timestamp(3_000_000),
                fight_id: Some("demo-fight".into())
            },
            PlayerDeathAt {
                at: Timestamp(4_000_000),
                fight_id: Some("demo-fight".into())
            }
        ]
    );
    assert_eq!(
        reader
            .get_player_context("demo-run", "demo-character", None, None)
            .unwrap()
            .unwrap()
            .deaths,
        local.deaths
    );
    let other = reader.get_run_player("demo-run", "other").unwrap().unwrap();
    assert_eq!(
        other.deaths,
        [
            PlayerDeathAt {
                at: Timestamp(4_000_000),
                fight_id: Some("demo-fight".into())
            },
            PlayerDeathAt {
                at: Timestamp(5_000_000),
                fight_id: None
            }
        ]
    );
    assert_eq!(other.departed_at, Some(Timestamp(6_000_000)));
    let context = reader
        .get_player_context(
            "demo-run",
            "other",
            Some(Timestamp(4_000_000)),
            Some(Timestamp(4_000_000)),
        )
        .unwrap()
        .unwrap();
    assert_eq!(context.deaths, other.deaths[..1]);
}

#[test]
fn player_context_windows_gear_and_timeline() {
    let (_dir, path, mut db) = reader_database();
    let mut update = timeline(&[("synthetic-player", vec![10., 20., 30., 40.])], true);
    update.buckets = (0..4)
        .map(|i| ScopeBucket {
            from: Timestamp(i * 100),
            until: Timestamp((i + 1) * 100),
            party_total: (i + 1) as f64 * 10.,
            party_active: 1,
        })
        .collect();
    update.compacted = true;
    let gear_profiles: Vec<_> = [
        profile(1, 50),
        profile(2, 100),
        profile(3, 200),
        profile(4, 300),
    ]
    .into_iter()
    .map(|mut p| {
        p.first_seen = Timestamp(50);
        p
    })
    .collect();
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::PlayerObserved(gear_profiles[0].clone())),
            envelope(9, EngineEvent::PlayerObserved(gear_profiles[1].clone())),
            envelope(10, EngineEvent::PlayerObserved(gear_profiles[2].clone())),
            envelope(11, EngineEvent::PlayerObserved(gear_profiles[3].clone())),
            envelope(12, EngineEvent::ScopeTimelineUpdated(update)),
        ],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let context = reader
        .get_player_context(
            "scope-a",
            "synthetic-player",
            Some(Timestamp(150)),
            Some(Timestamp(250)),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        context.gear_log.iter().map(|g| g.at.0).collect::<Vec<_>>(),
        [100, 200]
    );
    let timeline = context.timeline.unwrap();
    assert_eq!(timeline.points, [20., 30.]);
    assert_eq!(
        timeline
            .buckets
            .iter()
            .map(|b| b.from.0)
            .collect::<Vec<_>>(),
        [100, 200]
    );
    assert!(timeline.compacted);
    let before = reader
        .get_player_context("scope-a", "synthetic-player", None, Some(Timestamp(100)))
        .unwrap()
        .unwrap();
    assert_eq!(
        before.gear_log.iter().map(|g| g.at.0).collect::<Vec<_>>(),
        [50, 100]
    );
    let after = reader
        .get_player_context("scope-a", "synthetic-player", Some(Timestamp(200)), None)
        .unwrap()
        .unwrap();
    assert_eq!(
        after.gear_log.iter().map(|g| g.at.0).collect::<Vec<_>>(),
        [200, 300]
    );
}

#[test]
fn run_player_over_budget_is_limit() {
    let (_dir, path, mut db) = reader_database();
    apply_batch(
        &mut db,
        &[envelope(
            8,
            EngineEvent::PlayerObserved(run_profile("observer", 1, 100)),
        )],
    )
    .unwrap();
    db.execute(
        "UPDATE player_profiles SET json=json_set(json,'$.name',?1)",
        ["x".repeat(2 * 1024 * 1024)],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    assert!(matches!(
        reader.get_run_player("demo-run", "demo-character"),
        Err(StoreError::Limit)
    ));
    assert!(matches!(
        reader.get_player_context("demo-run", "observer", None, None),
        Err(StoreError::Limit)
    ));
    db.execute("DELETE FROM player_profiles", []).unwrap();
    let mut update = timeline(&[("demo-character", vec![10.])], true);
    update.scope_id = "demo-run".into();
    apply_batch(
        &mut db,
        &[envelope(9, EngineEvent::ScopeTimelineUpdated(update))],
    )
    .unwrap();
    db.execute(
        "UPDATE scope_timelines SET json=json_set(json,'$.extra',?1)",
        ["x".repeat(2 * 1024 * 1024)],
    )
    .unwrap();
    assert!(matches!(
        reader.get_run_player("demo-run", "demo-character"),
        Err(StoreError::Limit)
    ));
}

#[test]
fn timeline_points_are_zero_padded() {
    let (_dir, path, mut db) = reader_database();
    let mut first = timeline(&[("a", vec![10.]), ("b", vec![20.])], true);
    first.scope_id = "demo-run".into();
    let mut update = first.clone();
    update.full = false;
    update.players.truncate(1);
    update.players[0].points.push(5.);
    update.buckets.push(ScopeBucket {
        from: Timestamp(200),
        until: Timestamp(300),
        party_total: 5.,
        party_active: 1,
    });
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::ScopeTimelineUpdated(first)),
            envelope(9, EngineEvent::ScopeTimelineUpdated(update)),
        ],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    assert_eq!(
        reader
            .get_run_player("demo-run", "b")
            .unwrap()
            .unwrap()
            .timeline
            .unwrap()
            .points,
        [20., 0.]
    );
    assert_eq!(
        reader
            .get_player_context("demo-run", "b", Some(Timestamp(210)), None)
            .unwrap()
            .unwrap()
            .timeline
            .unwrap()
            .points,
        [0.]
    );
    let mut disabled = timeline(&[], false);
    disabled.scope_id = "demo-run".into();
    disabled.buckets.clear();
    disabled.disabled = true;
    apply_batch(
        &mut db,
        &[envelope(10, EngineEvent::ScopeTimelineUpdated(disabled))],
    )
    .unwrap();
    // Simulate a stored disabled scope retaining stale data; the reader owns this invariant.
    db.execute(
        "UPDATE scope_timelines SET json=json_set(json,'$.buckets',json(?1)) WHERE scope_id='demo-run'",
        [serde_json::to_string(&vec![ScopeBucket {
            from: Timestamp(100),
            until: Timestamp(200),
            party_total: 20.,
            party_active: 1,
        }]).unwrap()],
    ).unwrap();
    db.execute(
        "INSERT INTO player_timelines(scope_id,player_id,json) VALUES('demo-run','b',?1)",
        [json!({"points":[20.]}).to_string()],
    )
    .unwrap();
    let result = reader
        .get_player_context("demo-run", "b", None, None)
        .unwrap()
        .unwrap()
        .timeline
        .unwrap();
    assert!(result.disabled);
    assert!(result.points.is_empty());
    assert!(result.buckets.is_empty());
    assert!(
        reader
            .get_run_player("demo-run", "b")
            .unwrap()
            .unwrap()
            .timeline
            .unwrap()
            .disabled
    );
}

#[test]
fn run_player_party_excludes_zero_damage_and_roster_sorts_names() {
    let (_dir, path, mut db) = reader_database();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    let mut ally = fight.players[0].clone();
    ally.player_id = "ally".into();
    ally.object_id = 2;
    ally.is_local = false;
    ally.damage = 100.;
    ally.name = Some("Synthetic Ally".into());
    let mut zero = ally.clone();
    zero.player_id = "zero".into();
    zero.object_id = 3;
    zero.damage = 0.;
    zero.hits = 0;
    zero.sources.clear();
    fight.players.extend([ally, zero]);
    fight.total_damage = 400.;
    let mut z = run_profile("z", 1, 100);
    z.name = Some("Zed".into());
    let mut a = run_profile("a", 1, 100);
    a.name = Some("Alpha".into());
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::FightUpdated(fight)),
            envelope(9, EngineEvent::PlayerObserved(z)),
            envelope(10, EngineEvent::PlayerObserved(a)),
        ],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let result = reader.get_run_player("demo-run", "zero").unwrap().unwrap();
    assert!(result.player.is_none());
    assert_eq!(result.party.average_dps, Some(100.));
    assert_eq!(result.party.top_dps, Some(150.));
    assert_eq!(result.party.players_with_damage, 2);
    assert_eq!(
        result
            .roster
            .iter()
            .map(|r| r.player_id.as_str())
            .collect::<Vec<_>>(),
        ["demo-character", "ally", "zero", "a", "z"]
    );
}

#[test]
fn player_queries_validate_identifiers_and_window() {
    let (_dir, path, _db) = reader_database();
    let reader = open_reader(&path).unwrap();
    for id in ["", "bad\n", &"x".repeat(257)] {
        assert!(matches!(
            reader.get_run_player(id, "player"),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            reader.get_run_player("demo-run", id),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            reader.get_player_context(id, "player", None, None),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            reader.get_player_context("demo-run", id, None, None),
            Err(StoreError::InvalidEvent)
        ));
    }
    for (from, until) in [
        (Some(Timestamp(200)), Some(Timestamp(100))),
        (Some(Timestamp(u64::MAX)), None),
        (None, Some(Timestamp(u64::MAX))),
    ] {
        assert!(matches!(
            reader.get_player_context("demo-run", "player", from, until),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn player_context_enforces_one_mib_budget() {
    let (_dir, path, mut db) = reader_database();
    apply_batch(
        &mut db,
        &[envelope(
            8,
            EngineEvent::PlayerObserved(run_profile("observer", 1, 100)),
        )],
    )
    .unwrap();
    db.execute(
        "UPDATE player_profiles SET json=json_set(json,'$.name',?1)",
        ["x".repeat(1024 * 1024)],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    assert!(matches!(
        reader.get_player_context("demo-run", "observer", None, None),
        Err(StoreError::Limit)
    ));
    assert!(reader
        .get_run_player("demo-run", "demo-character")
        .unwrap()
        .is_some());
}

#[test]
fn player_context_map_scope_keeps_deaths_in_the_observed_map() {
    let (_dir, path, mut db) = reader_database();
    let scope = "test-stream:map:2";
    let mut p = profile(1, 4_000_000);
    p.scope_id = scope.into();
    p.run_id = None;
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = "realm-boss".into();
    fight.run_id = None;
    fight.scope_id = Some(scope.into());
    fight.ended = Some(Timestamp(4_000_000));
    let events = [
        EngineEvent::PlayerObserved(p),
        EngineEvent::FightStarted(fight),
        notice(
            "test-stream:instance:2:1",
            3_000_000,
            death_notice(Some("synthetic-player"), 101),
        ),
        notice(
            "test-stream:instance:3:1",
            3_000_000,
            death_notice(Some("synthetic-player"), 101),
        ),
        notice(
            "test-stream:instance:2:2",
            3_000_000,
            death_notice(None, 101),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, mut event)| {
        if let EngineEvent::InstanceEvent(notice) = &mut event {
            notice.run_id = None;
        }
        envelope(8 + i as u64, event)
    })
    .collect::<Vec<_>>();
    apply_batch(&mut db, &events).unwrap();
    let context = open_reader(&path)
        .unwrap()
        .get_player_context(scope, "synthetic-player", None, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        context.deaths,
        [PlayerDeathAt {
            at: Timestamp(3_000_000),
            fight_id: Some("realm-boss".into())
        }]
    );
}

#[test]
fn stored_provenance_normalizes_all_profile_reads_without_upgrading_replay() {
    for local in [true, false] {
        for gated in [true, false] {
            for count in [Some(8), None] {
                let (_dir, path, mut db) = reader_database();
                let mut p = run_profile("synthetic-player", 1, 500);
                p.is_local = local;
                p.maxed_gated = gated;
                p.maxed = count;
                p.maxed_stats = vec![20];
                apply_batch(
                    &mut db,
                    &[envelope(8, EngineEvent::PlayerObserved(p.clone()))],
                )
                .unwrap();
                if !gated {
                    db.execute(
                        "UPDATE player_profiles SET json=json_remove(json,'$.maxed_gated')",
                        [],
                    )
                    .unwrap();
                }
                let raw: String = db
                    .query_row(
                        "SELECT json FROM player_profiles WHERE player_id='synthetic-player'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                let replay: PlayerProfileView = serde_json::from_str(&raw).unwrap();
                assert_eq!(replay.maxed_gated, gated);
                let reader = open_reader(&path).unwrap();
                let context = reader
                    .get_player_context("demo-run", "synthetic-player", None, None)
                    .unwrap()
                    .unwrap();
                let run = reader
                    .get_run_player("demo-run", "synthetic-player")
                    .unwrap()
                    .unwrap();
                let supported = !local || gated;
                for actual in [context.profile.unwrap(), run.profile.unwrap()] {
                    let mut expected = p.clone();
                    if !supported {
                        expected.maxed = None;
                        expected.maxed_stats.clear();
                    }
                    assert_eq!(actual, expected);
                }
                let party = reader.get_party_run("demo-run").unwrap().unwrap();
                let member = party
                    .members
                    .iter()
                    .find(|m| m.player_id == p.player_id)
                    .unwrap();
                assert_eq!(member.maxed, if supported { count } else { None });
                assert_eq!(member.maxed_gated, gated);
                apply_batch(&mut db, &[envelope(9, EngineEvent::PlayerObserved(replay))]).unwrap();
                let raw: String = db
                    .query_row(
                        "SELECT json FROM player_profiles WHERE player_id='synthetic-player'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(
                    serde_json::from_str::<PlayerProfileView>(&raw)
                        .unwrap()
                        .maxed_gated,
                    gated
                );
            }
        }
    }
}

#[test]
fn fight_revisions_match_full_replacement_and_only_write_changed_sources() {
    fn rows(db: &Connection, table: &str) -> Vec<Vec<rusqlite::types::Value>> {
        let mut statement = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2,3"))
            .unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
    let mut actual = database();
    let mut replacement = database();
    actual
        .execute_batch(
            "CREATE TEMP TABLE writes(table_name TEXT, operation TEXT);
         CREATE TEMP TRIGGER source_insert AFTER INSERT ON damage_sources BEGIN
             INSERT INTO writes VALUES('source','insert'); END;
         CREATE TEMP TRIGGER source_update AFTER UPDATE ON damage_sources BEGIN
             INSERT INTO writes VALUES('source','update'); END;
         CREATE TEMP TRIGGER source_delete AFTER DELETE ON damage_sources BEGIN
             INSERT INTO writes VALUES('source','delete'); END;
         CREATE TEMP TRIGGER player_update AFTER UPDATE ON fight_players BEGIN
             INSERT INTO writes VALUES('player','update'); END;",
        )
        .unwrap();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    let template = fight.players[0].clone();
    assert!(!template.sources.is_empty());
    fight.players = (0..40)
        .map(|i| {
            let mut player = template.clone();
            player.player_id = format!("synthetic-{i}");
            player.object_id = i;
            player.name = Some(format!("Synthetic{i}"));
            player.is_local = i == 0;
            player.sources = vec![template.sources[0].clone(); 3];
            player
        })
        .collect();
    for revision in 0..7 {
        match revision {
            1 => {
                fight.players[0].sources[0].damage += 1.;
                fight.players[0].sources[0].hits += 1;
            }
            2 => {
                fight.players[0].sources.remove(0);
                fight.players[1].sources.push(template.sources[0].clone());
                fight.players[2].name = Some("Synthetic changed".into());
            }
            3 => {
                fight.players.remove(1);
                let mut added = template.clone();
                added.player_id = "added".into();
                fight.players.push(added);
            }
            4 => fight.players.reverse(),
            5 => fight.players.clear(),
            6 => fight.players.push(template.clone()),
            _ => {}
        }
        for player in &mut fight.players {
            player.damage = player.sources.iter().map(|source| source.damage).sum();
            player.hits = player.sources.iter().map(|source| source.hits).sum();
        }
        fight.total_damage = fight.players.iter().map(|player| player.damage).sum();
        for player in &mut fight.players {
            player.share = (fight.total_damage > 0.).then(|| player.damage / fight.total_damage);
        }
        actual.execute("DELETE FROM writes", []).unwrap();
        let event = envelope(8 + revision * 2, EngineEvent::FightUpdated(fight.clone()));
        apply_batch(&mut actual, std::slice::from_ref(&event)).unwrap();
        // Force the reference path to insert all children from scratch.
        replacement
            .execute("DELETE FROM damage_sources WHERE fight_id=?1", [&fight.id])
            .unwrap();
        replacement
            .execute("DELETE FROM fight_players WHERE fight_id=?1", [&fight.id])
            .unwrap();
        apply_batch(&mut replacement, &[event]).unwrap();
        for table in [
            "fights",
            "fight_summaries",
            "fight_players",
            "damage_sources",
            "pd_player_contributions",
            "pd_run_players",
            "party_run_members",
            "party_players",
        ] {
            assert_eq!(
                rows(&actual, table),
                rows(&replacement, table),
                "{table}, revision {revision}"
            );
        }
        if revision == 1 {
            let writes: Vec<(String, String)> = actual
                .prepare("SELECT * FROM writes")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let source_writes: Vec<_> = writes
                .iter()
                .filter(|(table, _)| table == "source")
                .collect();
            assert_eq!(source_writes, [&("source".into(), "update".into())]);
            // fight_players share churn is a known, separately tracked follow-up.
            assert_eq!(
                writes
                    .iter()
                    .filter(|(table, operation)| table == "player" && operation == "update")
                    .count(),
                40
            );
            assert_eq!(count(&actual, "pd_player_contributions"), 40);
            assert_eq!(count(&actual, "pd_run_players"), 40);
            assert_eq!(count(&actual, "party_run_members"), 39);
            assert!(count(&actual, "party_players") >= 39);
            // Old implementation deleted and inserted all 120 source rows.
            assert_eq!(count(&actual, "damage_sources"), 120);
        }
        actual.execute("DELETE FROM writes", []).unwrap();
        // A new receipt for the same projection must not rewrite either table.
        let repeat = envelope(9 + revision * 2, EngineEvent::FightUpdated(fight.clone()));
        apply_batch(&mut actual, std::slice::from_ref(&repeat)).unwrap();
        apply_batch(&mut replacement, &[repeat]).unwrap();
        assert_eq!(count(&actual, "writes"), 0);
    }
}

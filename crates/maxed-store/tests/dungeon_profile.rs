use crate::common::account_scope_fixture::ACCOUNT;
use crate::common::stats::{character, database, run, session, Stream};

use maxed_types::*;
use rusqlite::Connection;

const SECOND: u64 = 1_000_000;
const DUNGEON: &str = "Synthetic Dungeon";

fn request() -> DungeonProfileRequest {
    DungeonProfileRequest {
        dungeon: DUNGEON.into(),
        character_id: Some("alpha".into()),
        range: TimeRange {
            start: Timestamp(100 * SECOND),
            end: Timestamp(100_000 * SECOND),
        },
    }
}

fn cleared(index: u64, character: &str, seconds: u64) -> RunView {
    let mut run = run(
        &format!("synthetic-run-{index}"),
        "synthetic-session",
        DUNGEON,
        (100 + index * 1000) * SECOND,
    );
    run.character_id = Some(character.into());
    run.completed_at = Some(Timestamp(run.started.0 + seconds * SECOND));
    // Leaving the map later must not inflate the clear time.
    run.ended = Some(Timestamp(run.completed_at.unwrap().0 + 30 * SECOND));
    run.outcome = Some(Outcome::Cleared);
    run
}

fn kill(run: &RunView, target: u16, offset: u64, duration: u64) -> FightView {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = format!("{}-{target}-{offset}", run.id);
    fight.session_id = run.session_id.clone();
    fight.run_id = Some(run.id.clone());
    fight.target_type = Some(target);
    fight.boss_type = Some(target);
    fight.target_kind = Some(TargetKind::Boss);
    fight.started = Timestamp(run.started.0 + (offset - duration) * SECOND);
    fight.ended = Some(Timestamp(run.started.0 + offset * SECOND));
    fight.kill = Some(KillView {
        at: fight.ended.unwrap(),
        evidence: KillEvidence::HpZeroRemoved,
        provisional: false,
    });
    fight.first_hit = Some(fight.started);
    fight.last_hit = fight.ended;
    fight.issues.clear();
    fight.players.truncate(1);
    fight.players[0].is_local = true;
    fight.players[0].player_id = "SyntheticLocal".into();
    fight.players[0].dps = Some(1234.0);
    fight.players[0].damage = 10_000.0;
    fight.total_damage = 10_000.0;
    fight.unattributed_damage = 0.0;
    fight
}

fn seed(db: &mut Connection, runs: &[RunView], fights: Vec<FightView>) -> Stream {
    let mut stream = Stream::new("synthetic-dungeon-profile");
    let mut events = vec![
        session("synthetic-session", 0, 0, None),
        character("alpha", "Synthetic Alpha", 0),
        character("beta", "Synthetic Beta", 0),
    ];
    events.extend(runs.iter().cloned().map(EngineEvent::RunStarted));
    events.extend(fights.into_iter().map(EngineEvent::FightEnded));
    stream.apply(db, events);
    stream
}

#[test]
fn dungeon_profile_orders_bosses_by_median_kill_offset() {
    let (_dir, path, mut db) = database();
    let runs = [
        cleared(0, "alpha", 600),
        cleared(1, "beta", 600),
        cleared(2, "alpha", 600),
    ];
    let mut fights = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        fights.push(kill(run, 42, [240, 120, 180][i], [60, 80, 100][i]));
        let mut event_boss = kill(run, 43, [90, 220, 100][i], 30);
        event_boss.target_kind = Some(TargetKind::EventBoss);
        event_boss.players[0].dps = None;
        fights.push(event_boss);
    }
    seed(&mut db, &runs, fights);
    db.execute(
        "UPDATE fight_summaries SET json=json_set(json,'$.target_name','Synthetic Boss 42')
        WHERE json_extract(json,'$.target_type')=42",
        [],
    )
    .unwrap();
    let result = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .dungeon_profile(request(), Some(ACCOUNT))
        .unwrap();
    assert_eq!(result.clears, 3);
    assert_eq!(
        result
            .bosses
            .iter()
            .map(|b| b.target_type)
            .collect::<Vec<_>>(),
        [43, 42]
    );
    assert_eq!(result.bosses[0].median_offset_micros, 100 * SECOND);
    assert_eq!(result.bosses[0].best_dps, None);
    assert_eq!(result.bosses[1].median_offset_micros, 180 * SECOND);
    assert_eq!(result.bosses[1].median_kill_micros, Some(80 * SECOND));
    assert_eq!(
        result.bosses[1].target_name.as_deref(),
        Some("Synthetic Boss 42")
    );
    assert_eq!(
        (result.bosses[1].seen_in, result.bosses[1].kill_samples),
        (3, 3)
    );
}

#[test]
fn dungeon_profile_drops_bosses_seen_in_under_half_of_clears() {
    let (_dir, path, mut db) = database();
    let runs: Vec<_> = (0..4).map(|i| cleared(i, "alpha", 600)).collect();
    let fights = vec![
        kill(&runs[0], 42, 60, 30),
        kill(&runs[0], 42, 90, 30),
        kill(&runs[0], 42, 120, 30), // Repeats are still only one run.
        kill(&runs[0], 43, 80, 30),
        kill(&runs[1], 43, 100, 30),
    ];
    seed(&mut db, &runs, fights);
    let result = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .dungeon_profile(request(), Some(ACCOUNT))
        .unwrap();
    assert_eq!(result.clears, 4);
    assert_eq!(result.bosses.len(), 1);
    assert_eq!(result.bosses[0].target_type, 43);
    assert_eq!(result.bosses[0].seen_in, 2);
    assert_eq!(result.bosses[0].median_offset_micros, 90 * SECOND);
}

#[test]
fn dungeon_profile_kill_median_needs_three_samples() {
    let (_dir, path, mut db) = database();
    let runs = [
        cleared(0, "alpha", 600),
        cleared(1, "beta", 600),
        cleared(2, "alpha", 600),
    ];
    let mut stream = seed(
        &mut db,
        &runs,
        vec![kill(&runs[0], 42, 100, 60), kill(&runs[1], 42, 100, 80)],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let before = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!(
        (
            before.bosses[0].kill_samples,
            before.bosses[0].median_kill_micros
        ),
        (2, None)
    );
    stream.apply(
        &mut db,
        vec![EngineEvent::FightEnded(kill(&runs[2], 42, 120, 100))],
    );
    let after = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!(
        (
            after.bosses[0].kill_samples,
            after.bosses[0].median_kill_micros
        ),
        (3, Some(80 * SECOND))
    );
}

#[test]
fn dungeon_profile_usual_run_dps_needs_three_runs_on_character() {
    let (_dir, path, mut db) = database();
    let mut runs: Vec<_> = (0..5)
        .map(|i| cleared(i, if i == 3 { "beta" } else { "alpha" }, 600))
        .collect();
    runs[1].outcome = Some(Outcome::Left);
    runs[1].completed_at = None;
    runs[2].outcome = Some(Outcome::Died);
    runs[2].completed_at = None;
    let third = runs[2].clone();
    runs[2].ended = None; // Not eligible until ended.
    let mut fights = Vec::new();
    for (i, run) in runs[..4].iter().enumerate() {
        for offset in [20, 110] {
            let mut fight = kill(run, 42, offset, 10);
            fight.players[0].damage = (i + 1) as f64 * 10_000.0;
            fight.total_damage = fight.players[0].damage;
            fights.push(fight);
        }
    }
    // The fifth ended run has no established DPS and contributes no sample.
    let mut stream = seed(&mut db, &runs, fights);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let before = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!((before.run_dps_samples, before.usual_run_dps), (2, None));
    stream.apply(&mut db, vec![EngineEvent::RunEnded(third)]);
    let after = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!(
        (after.run_dps_samples, after.usual_run_dps),
        (3, Some(400.0))
    );
    let story_mean = runs[..3]
        .iter()
        .map(|run| {
            reader
                .get_run(&run.id)
                .unwrap()
                .unwrap()
                .totals
                .unwrap()
                .local
                .unwrap()
                .dps
                .unwrap()
        })
        .sum::<f64>()
        / 3.0;
    assert_eq!(after.usual_run_dps, Some(story_mean));
    let all = reader
        .dungeon_profile(
            DungeonProfileRequest {
                character_id: None,
                ..request()
            },
            Some(ACCOUNT),
        )
        .unwrap();
    assert_eq!((all.run_dps_samples, all.usual_run_dps), (4, Some(500.0)));
}

#[test]
fn dungeon_profile_usual_clear_is_median_of_cleared_runs() {
    let (_dir, path, mut db) = database();
    let mut runs = vec![cleared(0, "alpha", 600), cleared(1, "alpha", 540)];
    let mut other = cleared(3, "alpha", 10);
    other.outcome = Some(Outcome::Left);
    other.completed_at = None;
    runs.extend([other, cleared(4, "beta", 100)]);
    let mut stream = seed(&mut db, &runs, vec![]);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let before = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!((before.clear_samples, before.usual_clear_micros), (2, None));
    stream.apply(
        &mut db,
        vec![EngineEvent::RunStarted(cleared(2, "alpha", 660))],
    );
    let result = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!(result.clears, 4);
    assert_eq!(
        (result.clear_samples, result.usual_clear_micros),
        (3, Some(600_000_000))
    );
    let all = reader
        .dungeon_profile(
            DungeonProfileRequest {
                character_id: None,
                ..request()
            },
            Some(ACCOUNT),
        )
        .unwrap();
    assert_eq!(
        (all.clear_samples, all.usual_clear_micros),
        (4, Some(570_000_000))
    );
}

#[test]
fn dungeon_profile_empty_for_unknown_dungeon() {
    let (_dir, path, mut db) = database();
    seed(&mut db, &[cleared(0, "alpha", 600)], vec![]);
    let result = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .dungeon_profile(
            DungeonProfileRequest {
                dungeon: "Unknown Synthetic Dungeon".into(),
                ..request()
            },
            Some(ACCOUNT),
        )
        .unwrap();
    assert_eq!(result.dungeon, "Unknown Synthetic Dungeon");
    assert_eq!(result.clears, 0);
    assert!(result.bosses.is_empty());
    assert_eq!(
        (result.usual_run_dps, result.usual_clear_micros),
        (None, None)
    );
    assert_eq!((result.run_dps_samples, result.clear_samples), (0, 0));
}

#[test]
fn dungeon_profile_skips_non_boss_provisional_unknown_and_inconsistent_kills() {
    let (_dir, path, mut db) = database();
    let run = cleared(0, "alpha", 600);
    let mut fights = vec![kill(&run, 42, 100, 60)];
    for (i, kind) in [Some(TargetKind::Miniboss), Some(TargetKind::Ordinary), None]
        .into_iter()
        .enumerate()
    {
        let mut fight = kill(&run, 50 + i as u16, 100, 60);
        fight.target_kind = kind;
        fights.push(fight);
    }
    let mut provisional = kill(&run, 60, 100, 60);
    provisional.kill.as_mut().unwrap().provisional = true;
    let mut unknown = kill(&run, 61, 100, 60);
    unknown.target_type = None;
    let mut no_kill = kill(&run, 62, 100, 60);
    no_kill.kill = None;
    let mut before_fight = kill(&run, 42, 200, 60);
    before_fight.kill.as_mut().unwrap().at = Timestamp(before_fight.started.0 - 1);
    let mut before_run = kill(&run, 42, 300, 60);
    before_run.started = Timestamp(run.started.0 - 2);
    before_run.kill.as_mut().unwrap().at = Timestamp(run.started.0 - 1);
    let broken = kill(&run, 63, 100, 60);
    let broken_id = broken.id.clone();
    fights.extend([
        provisional,
        unknown,
        no_kill,
        before_fight,
        before_run,
        broken,
    ]);
    seed(&mut db, &[run], fights);
    db.execute("UPDATE fights SET json='broken' WHERE id=?1", [&broken_id])
        .unwrap();
    let result = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .dungeon_profile(request(), Some(ACCOUNT))
        .unwrap();
    assert_eq!(result.bosses.len(), 1);
    assert_eq!(result.bosses[0].target_type, 42);
    assert_eq!(result.bosses[0].kill_samples, 1);
    assert_eq!(result.bosses[0].median_offset_micros, 100 * SECOND);
}

#[test]
fn dungeon_profile_scopes_clears_by_dungeon_and_half_open_run_start() {
    let (_dir, path, mut db) = database();
    let mut runs = vec![
        cleared(0, "alpha", 600),
        cleared(1, "beta", 600),
        cleared(2, "alpha", 600),
        cleared(3, "alpha", 600),
    ];
    runs[2].dungeon = "Other Synthetic Dungeon".into();
    runs[3].outcome = Some(Outcome::Left);
    let fights = runs.iter().map(|run| kill(run, 42, 100, 60)).collect();
    seed(&mut db, &runs, fights);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let result = reader
        .dungeon_profile(
            DungeonProfileRequest {
                range: TimeRange {
                    start: runs[0].started,
                    end: runs[1].started,
                },
                ..request()
            },
            Some(ACCOUNT),
        )
        .unwrap();
    assert_eq!(result.clears, 1);
    assert_eq!(result.bosses[0].kill_samples, 1);
    let result = reader
        .dungeon_profile(
            DungeonProfileRequest {
                range: TimeRange {
                    start: Timestamp(runs[0].started.0 + 1),
                    end: Timestamp(runs[1].started.0 + 1),
                },
                ..request()
            },
            Some(ACCOUNT),
        )
        .unwrap();
    // The run starts in range, even though its kill occurs after range.end.
    assert_eq!(result.clears, 1);
    assert_eq!(result.bosses[0].seen_in, 1);
    assert_eq!((result.clear_samples, result.run_dps_samples), (0, 0));
    let all = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!(all.clears, 2);
    assert_eq!(all.bosses[0].kill_samples, 2);
}

#[test]
fn dungeon_profile_best_dps_uses_character_resolution_and_fight_range() {
    let (_dir, path, mut db) = database();
    let run = cleared(0, "beta", 600);
    let mut realm = kill(&run, 42, 150, 60);
    realm.run_id = None;
    realm.players[0].dps = Some(2000.0);
    let mut open = realm.clone();
    open.id = "synthetic-open".into();
    open.ended = None;
    open.players[0].dps = Some(9000.0);
    let mut before = realm.clone();
    before.id = "synthetic-before".into();
    before.started = Timestamp(request().range.start.0 - 1);
    before.players[0].dps = Some(9000.0);
    let mut at_end = realm.clone();
    at_end.id = "synthetic-at-end".into();
    at_end.started = request().range.end;
    at_end.players[0].dps = Some(9000.0);
    let mut other = kill(&run, 42, 100, 60);
    other.players[0].dps = Some(8000.0);
    seed(&mut db, &[run], vec![other, realm, open, before, at_end]);
    db.execute("INSERT INTO map_visits(id,stream_id,session_id,character_id,map,kind,started,ended,source,json)
        VALUES('synthetic-visit','synthetic-dungeon-profile','synthetic-session','alpha','Synthetic Realm','realm',0,NULL,'live','{}')", []).unwrap();
    let result = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .dungeon_profile(request(), Some(ACCOUNT))
        .unwrap();
    assert_eq!(result.bosses[0].best_dps, Some(2000.0));
    let all = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .dungeon_profile(
            DungeonProfileRequest {
                character_id: None,
                ..request()
            },
            Some(ACCOUNT),
        )
        .unwrap();
    assert_eq!(all.bosses[0].best_dps, Some(8000.0));
}

#[test]
fn dungeon_profile_floors_even_medians_and_ignores_negative_clear_times() {
    let (_dir, path, mut db) = database();
    let mut runs: Vec<_> = (0..5).map(|i| cleared(i, "alpha", 600)).collect();
    let mut fights = Vec::new();
    for (i, run) in runs[..4].iter_mut().enumerate() {
        run.completed_at = Some(Timestamp(run.started.0 + 600 * SECOND + i as u64));
        let mut fight = kill(run, 42, 100, 60);
        fight.kill.as_mut().unwrap().at.0 += i as u64;
        fights.push(fight);
    }
    runs[4].completed_at = Some(Timestamp(runs[4].started.0 - 1));
    seed(&mut db, &runs, fights);
    let result = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .dungeon_profile(request(), Some(ACCOUNT))
        .unwrap();
    assert_eq!(
        (result.clear_samples, result.usual_clear_micros),
        (4, Some(600 * SECOND + 1))
    );
    assert_eq!(result.bosses[0].median_offset_micros, 100 * SECOND + 1);
    assert_eq!(result.bosses[0].median_kill_micros, Some(60 * SECOND + 1));
}

#[test]
fn dungeon_profile_rejects_invalid_filters_and_unknown_request_fields() {
    let (_dir, path, _db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    for dungeon in [
        String::new(),
        "x".repeat(257),
        "é".repeat(129),
        "Synthetic\nDungeon".into(),
    ] {
        assert!(reader
            .dungeon_profile(
                DungeonProfileRequest {
                    dungeon,
                    ..request()
                },
                Some(ACCOUNT)
            )
            .is_err());
    }
    for character_id in [String::new(), "x".repeat(257), "synthetic\u{7}".into()] {
        assert!(reader
            .dungeon_profile(
                DungeonProfileRequest {
                    character_id: Some(character_id),
                    ..request()
                },
                Some(ACCOUNT)
            )
            .is_err());
    }
    for (start, end) in [(2, 1), (1, 1), (0, i64::MAX as u64 + 1)] {
        assert!(reader
            .dungeon_profile(
                DungeonProfileRequest {
                    range: TimeRange {
                        start: Timestamp(start),
                        end: Timestamp(end)
                    },
                    ..request()
                },
                Some(ACCOUNT)
            )
            .is_err());
    }
    assert!(reader
        .dungeon_profile(
            DungeonProfileRequest {
                dungeon: "é".repeat(128),
                ..request()
            },
            Some(ACCOUNT)
        )
        .is_ok());
    let mut json = serde_json::to_value(request()).unwrap();
    json["extra"] = true.into();
    assert!(serde_json::from_value::<DungeonProfileRequest>(json).is_err());
}

fn reference_run_dps(
    db: &Connection,
    reader: &maxed_store::StoreReader,
    request: &DungeonProfileRequest,
) -> (u32, Option<f64>) {
    let mut statement = db
        .prepare(
            "SELECT id FROM runs WHERE dungeon=?4 AND started>=?1 AND started<?2
         AND ended>=started AND (?3 IS NULL OR character_id=?3) AND json_valid(json)
         ORDER BY started,id",
        )
        .unwrap();
    let ids = statement
        .query_map(
            rusqlite::params![
                request.range.start.0 as i64,
                request.range.end.0 as i64,
                request.character_id,
                request.dungeon,
            ],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    let mut count = 0;
    let mut mean = 0.0;
    for id in ids {
        if let Some(dps) = reader
            .get_run(&id.unwrap())
            .unwrap()
            .unwrap()
            .totals
            .and_then(|totals| totals.local)
            .and_then(|local| local.dps)
        {
            count += 1;
            mean += (dps - mean) / f64::from(count);
        }
    }
    (count, (count >= 3).then_some(mean))
}

#[test]
fn dungeon_profile_run_dps_matches_run_stories_on_mixed_history() {
    let (_dir, path, mut db) = database();
    let mut runs: Vec<_> = (0..16).map(|i| cleared(i, "alpha", 600)).collect();
    runs[1].started = runs[0].started; // Tie: id determines incremental-mean order.
    runs[5].character_id = Some("beta".into());
    runs[6].started = Timestamp(request().range.start.0 - 1);
    runs[7].started = request().range.end;
    runs[7].ended = Some(Timestamp(request().range.end.0 + SECOND));
    runs[8].started = Timestamp(request().range.end.0 + SECOND);
    runs[8].ended = Some(Timestamp(request().range.end.0 + 2 * SECOND));
    runs[10].dungeon = "Other Synthetic Dungeon".into();
    runs[11].ended = None;
    runs[12].ended = Some(Timestamp(runs[12].started.0 - 1));
    let mut fights = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        if i == 3 {
            continue;
        } // No fights.
        for offset in [20, 110] {
            let mut fight = kill(run, 42, offset, 10);
            fight.players[0].damage = (i + 1) as f64 * 10_000.0;
            fight.players[0].is_local = i != 4;
            if i == 1 || i == 2 {
                let mut second = fight.players[0].clone();
                second.player_id = "SyntheticOtherLocal".into();
                second.damage *= 3.0;
                fight.players.push(second);
            }
            if i == 13 {
                fight.last_hit = fight.first_hit;
            }
            if i == 14 {
                fight.first_hit = None;
            }
            if i == 15 {
                fight.last_hit = None;
            }
            fight.total_damage = fight.players.iter().map(|p| p.damage).sum();
            fights.push(fight);
            if i == 13 {
                break;
            } // Zero-length window gives None DPS.
        }
    }
    seed(&mut db, &runs, fights);
    // Legacy cache: leave only the lower-damage local cached. Fallback must win.
    db.execute(
        "DELETE FROM pd_run_players WHERE run_id=?1 AND player_id='SyntheticOtherLocal'",
        [&runs[2].id],
    )
    .unwrap();
    // Exercise compensated damage and ranking using the corrected value.
    db.execute("UPDATE pd_run_players SET damage_correction=90000 WHERE run_id=?1 AND player_id='SyntheticLocal'", [&runs[1].id]).unwrap();
    db.execute(
        "UPDATE runs SET json='invalid synthetic json' WHERE id=?1",
        [&runs[9].id],
    )
    .unwrap();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    for character_id in [Some("alpha".into()), None, Some("beta".into())] {
        let request = DungeonProfileRequest {
            character_id,
            ..request()
        };
        let expected = reference_run_dps(&db, &reader, &request);
        assert_eq!(
            expected.0,
            match request.character_id.as_deref() {
                Some("alpha") => 3,
                Some("beta") => 1,
                _ => 4,
            }
        );
        let actual = reader.dungeon_profile(request, Some(ACCOUNT)).unwrap();
        assert_eq!((actual.run_dps_samples, actual.usual_run_dps), expected);
    }
    // Equal local damage still agrees with totals()'s player-id tie-break.
    db.execute("UPDATE pd_run_players SET damage_correction=80000 WHERE run_id=?1 AND player_id='SyntheticLocal'", [&runs[1].id]).unwrap();
    let actual = reader.dungeon_profile(request(), Some(ACCOUNT)).unwrap();
    assert_eq!(
        (actual.run_dps_samples, actual.usual_run_dps),
        reference_run_dps(&db, &reader, &request())
    );
}

#[test]
fn dungeon_profile_run_dps_query_uses_run_indexes() {
    let (_dir, _path, mut db) = database();
    let runs: Vec<_> = (0..8).map(|i| cleared(i, "alpha", 600)).collect();
    let fights = runs.iter().map(|run| kill(run, 42, 100, 60)).collect();
    seed(&mut db, &runs, fights);
    let sql = include_str!("../src/dungeon_profile_dps.sql");
    let plan = db
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap()
        .query_map(
            rusqlite::params![
                (100 * SECOND) as i64,
                (100_000 * SECOND) as i64,
                "alpha",
                DUNGEON,
                ACCOUNT
            ],
            |row| row.get::<_, String>(3),
        )
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    for index in ["fight_summaries_run", "sqlite_autoindex_pd_run_players_1"] {
        assert!(plan.contains(index), "{plan}");
    }
    for table in ["fight_summaries", "fight_players", "pd_run_players"] {
        assert!(!plan.contains(&format!("SCAN {table}")), "{plan}");
    }
}

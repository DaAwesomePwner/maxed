use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;
mod common;
#[path = "common/d_legacy.rs"]
mod d_legacy;

#[test]
fn malformed_legacy_players_do_not_break_story_or_change_partial_rules() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("seed");
    let mut db = Connection::open(&seed).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    let path = dir.path().join("legacy");
    let mut legacy = Connection::open(&path).unwrap();
    d_legacy::copy_v4(&seed, &legacy);
    legacy
        .execute("UPDATE fight_players SET json='broken'", [])
        .unwrap();
    migrate(&mut legacy).unwrap();
    let reader = open_reader(&path).unwrap();
    for invalid in ["broken", "{}", "{\"is_local\":null}"] {
        legacy
            .execute("UPDATE fight_players SET json=?1", [invalid])
            .unwrap();
        for (partial, estimated) in [(false, false), (true, false), (false, true), (true, true)] {
            legacy.execute("UPDATE fight_summaries SET json=json_set(json,'$.partial',json(?1),'$.estimated',json(?2))",
                rusqlite::params![partial.to_string(), estimated.to_string()]).unwrap();
            let story = reader.get_run("demo-run").unwrap().unwrap();
            let totals = story.totals.unwrap();
            assert_eq!(totals.fights, 1);
            assert!(totals.players.is_empty());
            assert!(totals.local.is_none());
            assert_eq!(totals.unattributed_damage, totals.total_damage);
            assert_eq!(totals.partial, partial || estimated);
        }
    }
}
#[test]
fn story_totals_equal_engine_run_totals() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("totals");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    let first = demo::demo_snapshot().fights.remove(0);
    let mut second = first.clone();
    second.id = "second".into();
    second.first_hit = Some(Timestamp(7_000_000));
    second.last_hit = Some(Timestamp(9_000_000));
    second.players[0].name = Some("SynthBravo".into());
    let mut realm = second.clone();
    realm.id = "realm".into();
    realm.run_id = None;
    apply_batch(
        &mut db,
        &[
            common::envelope(8, EngineEvent::FightEnded(second.clone())),
            common::envelope(9, EngineEvent::FightEnded(realm)),
        ],
    )
    .unwrap();
    let expected = maxed_engine::run_totals(&[first, second], Some((1250, 1260)));
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .totals,
        Some(expected)
    );
}

#[test]
fn legacy_fallback_matches_cached_totals_on_the_same_history() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("cached");
    let mut db = Connection::open(&seed).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..3]).unwrap();
    let mut fights = Vec::new();
    for (index, id) in ["fight-a", "fight-a0", "fight-b"].into_iter().enumerate() {
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.id = id.into();
        let template = fight.players[0].clone();
        fight.players = (0..70)
            .map(|i| {
                let mut p = template.clone();
                p.player_id = format!("player-{i:02}");
                p.object_id = i;
                p.is_local = i == 69 && index == 0;
                p.damage = if i < 2 { 100. } else { (70 - i) as f64 };
                p.hits = u32::MAX;
                p.deaths = index as u32;
                p.name = match index {
                    0 => Some("SynthAlpha".into()),
                    1 => Some("SynthBravo!".into()),
                    _ => None,
                };
                p.class_id = (index != 1).then_some(100 + index as u16);
                p
            })
            .collect();
        fight.total_damage = fight.players.iter().map(|p| p.damage).sum();
        fight.unattributed_damage = 0.;
        fights.push(fight.clone());
        apply_batch(
            &mut db,
            &[common::envelope(
                index as u64 + 4,
                EngineEvent::FightEnded(fight),
            )],
        )
        .unwrap();
    }
    let expected = maxed_engine::run_totals(&fights, None);
    let cached = open_reader(&seed)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .totals
        .unwrap();
    assert_eq!(cached, expected);
    let path = dir.path().join("legacy");
    let mut legacy = Connection::open(&path).unwrap();
    d_legacy::copy_v4(&seed, &legacy);
    migrate(&mut legacy).unwrap();
    assert_eq!(
        legacy
            .query_row("SELECT count(*) FROM pd_run_players", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let reader = open_reader(&path).unwrap();
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().totals.unwrap(),
        cached
    );
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().totals.unwrap(),
        cached
    );
    // A revision only projects that fight; the fallback must still include
    // the untouched legacy fights after the writer commits.
    fights[1].players[0].name = Some("SynthRevised".into());
    fights[1].players[0].damage += 123.;
    fights[1].total_damage += 123.;
    apply_batch(
        &mut legacy,
        &[common::envelope(
            7,
            EngineEvent::FightUpdated(fights[1].clone()),
        )],
    )
    .unwrap();
    let expected = maxed_engine::run_totals(&fights, None);
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().totals.unwrap(),
        expected
    );
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .totals
            .unwrap(),
        expected
    );
    fights[0].run_id = None;
    apply_batch(
        &mut legacy,
        &[common::envelope(
            8,
            EngineEvent::FightUpdated(fights[0].clone()),
        )],
    )
    .unwrap();
    let expected = maxed_engine::run_totals(&fights[1..], None);
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().totals.unwrap(),
        expected
    );
    let mut rolled_back = fights[1].clone();
    rolled_back.players[0].damage += 999.;
    rolled_back.total_damage += 999.;
    assert!(apply_batch(
        &mut legacy,
        &[
            common::envelope(9, EngineEvent::FightUpdated(rolled_back.clone())),
            common::envelope(11, EngineEvent::FightUpdated(rolled_back)),
        ]
    )
    .is_err());
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().totals.unwrap(),
        expected
    );
    apply_batch(
        &mut legacy,
        &[
            common::envelope(
                9,
                EngineEvent::FameChanged {
                    at: Timestamp(1_000_000),
                    character_id: "demo-character".into(),
                    fame: 1250,
                },
            ),
            common::envelope(
                10,
                EngineEvent::FameChanged {
                    at: Timestamp(4_000_000),
                    character_id: "demo-character".into(),
                    fame: 1260,
                },
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().totals.unwrap(),
        maxed_engine::run_totals(&fights[1..], Some((1250, 1260)))
    );
}
#[test]
fn totals_none_without_fights() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("totals");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..3]).unwrap();
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .totals,
        None
    );
}
#[test]
fn totals_fame_delta_needs_two_samples() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("totals");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let events = common::story_events();
    apply_batch(&mut db, &events[..6]).unwrap();
    let reader = open_reader(&path).unwrap();
    assert_eq!(
        reader
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .totals
            .unwrap()
            .fame_delta,
        None
    );
    apply_batch(&mut db, &events[6..]).unwrap();
    assert_eq!(
        reader
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .totals
            .unwrap()
            .fame_delta,
        Some(10)
    );
}

#[test]
fn totals_cap_preserves_local_full_rank_and_ties() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cap");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..3]).unwrap();
    let mut f = demo::demo_snapshot().fights.remove(0);
    let template = f.players[0].clone();
    f.players = (0..70)
        .map(|i| {
            let mut p = template.clone();
            p.player_id = format!("p-{i:02}");
            p.object_id = i;
            p.damage = if i < 2 { 100. } else { (70 - i) as f64 };
            p.is_local = i == 69;
            p.name = Some("SynthAlpha".into());
            p
        })
        .collect();
    f.total_damage = f.players.iter().map(|p| p.damage).sum();
    f.unattributed_damage = 17.;
    f.total_damage += 17.;
    let expected = maxed_engine::run_totals(&[f.clone()], None);
    apply_batch(&mut db, &[common::envelope(4, EngineEvent::FightEnded(f))]).unwrap();
    let actual = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .totals
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.players.len(), 64);
    assert_eq!(actual.players[0].rank, Some(1));
    assert_eq!(actual.players[1].rank, Some(1));
    assert_eq!(actual.players[2].rank, Some(3));
    assert_eq!(actual.local.unwrap().rank, Some(70));
}
#[test]
fn totals_count_kills_without_promoting_unconfirmed_fights() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kills");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    let mut f = demo::demo_snapshot().fights.remove(0);
    f.id = "kill".into();
    f.kill = Some(KillView {
        at: Timestamp(4_000_000),
        evidence: KillEvidence::HpZeroRemoved,
        provisional: false,
    });
    apply_batch(&mut db, &[common::envelope(8, EngineEvent::FightEnded(f))]).unwrap();
    let totals = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .totals
        .unwrap();
    assert_eq!(totals.fights, 2);
    assert_eq!(totals.kills, 1);
}
#[test]
fn fame_is_limited_to_the_run_character_and_closed_window() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fame");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    let mut run = demo::demo_run_story().run;
    run.ended = Some(Timestamp(4_000_000));
    apply_batch(
        &mut db,
        &[
            common::envelope(8, EngineEvent::RunEnded(run)),
            common::envelope(
                9,
                EngineEvent::FameChanged {
                    at: Timestamp(4_000_000),
                    character_id: "demo-character".into(),
                    fame: 1255,
                },
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .totals
            .unwrap()
            .fame_delta,
        Some(5)
    );
}

#[test]
fn legacy_null_session_fame_samples_are_included() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-fame");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    db.execute("UPDATE fame_samples SET session_id=NULL", [])
        .unwrap();
    let totals = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .totals
        .unwrap();
    assert_eq!(totals.fame_delta, Some(10));
}

#[test]
fn persisted_players_follow_revisions_reassociation_and_latest_non_null_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("revisions");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..3]).unwrap();
    let mut a = demo::demo_snapshot().fights.remove(0);
    a.id = "fight-a".into();
    let mut b = a.clone();
    b.id = "fight-a0".into();
    let mut c = a.clone();
    c.id = "fight-c".into();
    a.players[0].name = Some("SynthAlpha".into());
    a.players[0].damage = 1e16;
    a.players[0].hits = u32::MAX;
    a.total_damage = 1e16;
    b.players[0].name = Some("SynthBravo!".into());
    b.players[0].class_id = None;
    b.players[0].damage = 2.;
    b.total_damage = 2.;
    c.players[0].name = None;
    c.players[0].is_local = false;
    c.players[0].class_id = Some(200);
    c.players[0].damage = 3.;
    c.total_damage = 3.;
    apply_batch(
        &mut db,
        &[
            common::envelope(4, EngineEvent::FightUpdated(a.clone())),
            common::envelope(5, EngineEvent::FightUpdated(b.clone())),
            common::envelope(6, EngineEvent::FightUpdated(c.clone())),
        ],
    )
    .unwrap();
    let read = || {
        open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .totals
            .unwrap()
    };
    assert_eq!(
        read(),
        maxed_engine::run_totals(&[a.clone(), b.clone(), c.clone()], None)
    );
    a.players[0].damage = 4.;
    a.players[0].hits = 4;
    a.players[0].is_local = false;
    a.total_damage = 4.;
    apply_batch(
        &mut db,
        &[common::envelope(7, EngineEvent::FightUpdated(a.clone()))],
    )
    .unwrap();
    assert_eq!(
        read(),
        maxed_engine::run_totals(&[a.clone(), b.clone(), c.clone()], None)
    );
    b.run_id = None;
    apply_batch(
        &mut db,
        &[common::envelope(8, EngineEvent::FightUpdated(b))],
    )
    .unwrap();
    assert_eq!(
        read(),
        maxed_engine::run_totals(&[a.clone(), c.clone()], None)
    );
    c.players.clear();
    c.total_damage = 0.;
    apply_batch(
        &mut db,
        &[common::envelope(9, EngineEvent::FightUpdated(c.clone()))],
    )
    .unwrap();
    assert_eq!(read(), maxed_engine::run_totals(&[a.clone(), c], None));
    let before = read();
    a.players[0].damage = 999.;
    a.total_damage = 999.;
    assert!(apply_batch(
        &mut db,
        &[
            common::envelope(10, EngineEvent::FightUpdated(a.clone())),
            common::envelope(12, EngineEvent::FightUpdated(a))
        ]
    )
    .is_err());
    assert_eq!(read(), before);
}

#[test]
fn player_scalar_trigger_reads_covering_index_without_reparsing_json() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    for trigger in [
        "pd_player_insert",
        "pd_player_update",
        "pd_fight_association",
    ] {
        let ddl: String = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name=?1",
                [trigger],
                |r| r.get(0),
            )
            .unwrap();
        let select = ddl
            .split("INSERT INTO pd_player_contributions")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .replace("NEW.fight_id", "'fight-a'")
            .replace("NEW.player_id", "'p'")
            .replace("NEW.run_id", "'demo-run'")
            .replace("NEW.started", "1")
            .replace("NEW.id", "'fight-a'");
        let plan: Vec<String> = db
            .prepare(&format!("EXPLAIN QUERY PLAN {select}"))
            .unwrap()
            .query_map([], |r| r.get(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            plan.iter()
                .any(|line| line.contains("COVERING INDEX pd_fight_players_totals")),
            "{plan:?}"
        );
        let functions: Vec<Option<String>> = db
            .prepare(&format!("EXPLAIN {select}"))
            .unwrap()
            .query_map([], |r| r.get(5))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            !functions.iter().flatten().any(|f| f.starts_with("json_")),
            "{functions:?}"
        );
    }
}

#[test]
fn fight_revisions_preserve_unchanged_players_and_reject_duplicate_ids() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..3]).unwrap();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    let mut stable = fight.players[0].clone();
    stable.player_id = "stable".into();
    stable.object_id = 99;
    stable.is_local = false;
    fight.players.push(stable);
    apply_batch(
        &mut db,
        &[common::envelope(
            4,
            EngineEvent::FightUpdated(fight.clone()),
        )],
    )
    .unwrap();
    db.execute_batch("CREATE TEMP TRIGGER stable_update BEFORE UPDATE ON fight_players WHEN OLD.player_id='stable' BEGIN SELECT RAISE(ABORT,'unchanged player updated'); END;
        CREATE TEMP TRIGGER stable_delete BEFORE DELETE ON fight_players WHEN OLD.player_id='stable' BEGIN SELECT RAISE(ABORT,'unchanged player deleted'); END;").unwrap();
    fight.players[0].damage += 1.;
    fight.total_damage += 1.;
    apply_batch(
        &mut db,
        &[common::envelope(
            5,
            EngineEvent::FightUpdated(fight.clone()),
        )],
    )
    .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM fight_players", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    fight.players[0].sources.clear();
    fight.players = vec![fight.players[0].clone(), fight.players[0].clone()];
    assert!(matches!(
        apply_batch(
            &mut db,
            &[common::envelope(6, EngineEvent::FightUpdated(fight))]
        ),
        Err(StoreError::InvalidEvent)
    ));
    assert_eq!(
        db.query_row("SELECT count(*) FROM fight_players", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

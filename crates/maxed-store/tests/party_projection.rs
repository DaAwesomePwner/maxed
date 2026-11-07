use crate::common::stats::{map, run, session, Stream};
use maxed_store::migrate;
use maxed_types::*;
use rusqlite::Connection;
use serde_json::{json, Value};

fn database() -> (Connection, Stream, RunView) {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let mut stream = Stream::new("party-stream");
    let run = run("party-run", "party-session", "Synthetic Dungeon", 100);
    stream.apply(
        &mut db,
        vec![
            session("party-session", 0, 10_000, None),
            EngineEvent::RunStarted(run.clone()),
        ],
    );
    (db, stream, run)
}

fn profile(id: &str, object_id: i32, name: Option<&str>) -> PlayerProfileView {
    PlayerProfileView {
        maxed_gated: false,
        scope_id: "party-run".into(),
        run_id: Some("party-run".into()),
        player_id: id.into(),
        object_id,
        is_local: false,
        name: name.map(str::to_owned),
        class_id: Some(782),
        level: Some(20),
        stars: None,
        fame: None,
        skin_id: None,
        guild: None,
        stats: vec![],
        maxed: None,
        maxed_stats: vec![],
        equipment: vec![],
        first_seen: Timestamp(150),
        last_seen: Timestamp(200),
    }
}

fn player(profile: &PlayerProfileView, damage: f64) -> PlayerDamage {
    PlayerDamage {
        object_id: profile.object_id,
        player_id: profile.player_id.clone(),
        name: profile.name.clone(),
        class_id: profile.class_id,
        is_local: profile.is_local,
        damage,
        hits: 1,
        biggest_hit: Some(damage),
        deaths: 0,
        dps: None,
        share: None,
        rank: None,
        sources: vec![],
    }
}

fn fight(run: &RunView, players: Vec<PlayerDamage>) -> FightView {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = "party-fight".into();
    fight.run_id = Some(run.id.clone());
    fight.session_id = run.session_id.clone();
    fight.scope_id = Some(run.id.clone());
    fight.started = Timestamp(250);
    fight.first_hit = Some(Timestamp(250));
    fight.last_hit = Some(Timestamp(500));
    fight.ended = Some(Timestamp(500));
    fight.end_reason = Some(FightEnd::Killed);
    fight.boss_type = None;
    fight.target_kind = Some(TargetKind::Ordinary);
    fight.total_damage = players.iter().map(|p| p.damage).sum();
    fight.players = players;
    fight.buckets.clear();
    fight
}

fn left(run: &RunView, id: &str, object_id: i32, name: Option<&str>, at: u64) -> EngineEvent {
    EngineEvent::InstanceEvent(InstanceEventView {
        id: id.into(),
        session_id: run.session_id.clone(),
        run_id: Some(run.id.clone()),
        map: Some(run.dungeon.clone()),
        at: Timestamp(at),
        kind: InstanceEventKind::PlayerLeft {
            object_id,
            player_name: name.map(str::to_owned),
            class_id: None,
        },
        provisional: false,
    })
}

fn end(run: &RunView) -> EngineEvent {
    let mut run = run.clone();
    run.ended = Some(Timestamp(900));
    EngineEvent::RunEnded(run)
}

fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn saved_player(db: &Connection) -> Value {
    let text: String = db
        .query_row(
            "SELECT json_object('key',key,'name',name,'first_seen',first_seen,'last_seen',last_seen,
                'scope',last_scope_id,'map',last_map,'profile',json(last_profile_json),'updated',updated)
             FROM party_players WHERE key='synthalpha'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn member_rows(db: &Connection, source: &str, run_id: &str) -> Vec<Value> {
    db.prepare(&format!(
        "SELECT json_object('key',key,'run_id',run_id,'player_id',player_id,'class_id',class_id,
            'joined',joined,'departed_at',departed_at,'left_early',left_early,'died',died,
            'damage',damage,'share',share,'boss_share',boss_share)
         FROM {source} ORDER BY key,player_id"
    ))
    .unwrap()
    .query_map([run_id], |r| r.get::<_, String>(0))
    .unwrap()
    .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
    .collect()
}

fn members(db: &Connection, run_id: &str) -> Vec<Value> {
    member_rows(db, "party_run_members WHERE run_id=?1", run_id)
}

#[test]
fn observed_remote_player_upserts_party_player() {
    let (mut db, mut stream, _) = database();
    let mut alpha = profile("alpha", 101, Some("SynthAlpha,fe14"));
    stream.apply(&mut db, vec![EngineEvent::PlayerObserved(alpha.clone())]);
    assert_eq!(
        saved_player(&db),
        json!({"key":"synthalpha","name":"SynthAlpha","first_seen":150,"last_seen":200,
            "scope":"party-run","map":"Synthetic Dungeon","profile":alpha,"updated":200})
    );

    let later = run(
        "later-run",
        "party-session",
        "Synthetic Later Dungeon",
        1000,
    );
    alpha.name = Some("SynthAlpha".into());
    alpha.scope_id = later.id.clone();
    alpha.run_id = Some(later.id.clone());
    alpha.first_seen = Timestamp(1100);
    alpha.last_seen = Timestamp(1200);
    alpha.class_id = Some(800);
    stream.apply(
        &mut db,
        vec![
            EngineEvent::RunStarted(later),
            EngineEvent::PlayerObserved(alpha.clone()),
        ],
    );
    assert_eq!(count(&db, "party_players"), 1);
    assert_eq!(
        saved_player(&db),
        json!({"key":"synthalpha","name":"SynthAlpha","first_seen":150,"last_seen":1200,
            "scope":"later-run","map":"Synthetic Later Dungeon","profile":alpha,"updated":1200})
    );

    alpha.scope_id = "party-stream:map:3".into();
    alpha.run_id = None;
    alpha.first_seen = Timestamp(2100);
    alpha.last_seen = Timestamp(2200);
    // A final profile for a closed scope must use that visit, not the new map.
    stream.apply(
        &mut db,
        vec![
            map("Synthetic Realm", AreaKind::Realm, 2000),
            map("Synthetic Hub", AreaKind::OtherHub, 3000),
            EngineEvent::PlayerObserved(alpha.clone()),
        ],
    );
    assert_eq!(saved_player(&db)["map"], "Synthetic Realm");
    assert_eq!(saved_player(&db)["profile"], json!(alpha));
    let newest = alpha.clone();

    // A stale observation with no matching visit preserves last_seen and last_map.
    alpha.scope_id = "unmapped-scope".into();
    alpha.first_seen = Timestamp(50);
    alpha.last_seen = Timestamp(60);
    stream.apply(&mut db, vec![EngineEvent::PlayerObserved(alpha.clone())]);
    let saved = saved_player(&db);
    assert_eq!(saved["first_seen"], 50);
    assert_eq!(saved["last_seen"], 2200);
    assert_eq!(saved["map"], "Synthetic Realm");
    assert_eq!(saved["scope"], newest.scope_id);
    assert_eq!(saved["profile"], json!(newest));
    assert_eq!(saved["updated"], 2200);
}

#[test]
fn profile_interval_selects_latest_overlapping_visit() {
    for previous_visit in [false, true] {
        for (first_seen, last_seen) in [
            (1990, 2100),
            (1990, 2000),
            (2000, 2000),
            (2000, 2100),
            (2100, 2200),
        ] {
            for closed in [false, true] {
                let (mut db, mut stream, _) = database();
                if previous_visit {
                    stream.apply(
                        &mut db,
                        vec![map("Synthetic Previous Hub", AreaKind::OtherHub, 1000)],
                    );
                }
                stream.apply(&mut db, vec![map("Synthetic Realm", AreaKind::Realm, 2000)]);
                if closed {
                    stream.apply(
                        &mut db,
                        vec![map("Synthetic Later Hub", AreaKind::OtherHub, 3000)],
                    );
                }
                let mut alpha = profile("alpha", 101, Some("SynthAlpha"));
                alpha.run_id = None;
                alpha.scope_id = "party-stream:map:realm".into();
                alpha.first_seen = Timestamp(first_seen);
                alpha.last_seen = Timestamp(last_seen);
                stream.apply(&mut db, vec![EngineEvent::PlayerObserved(alpha.clone())]);
                assert_eq!(
                    saved_player(&db)["map"],
                    "Synthetic Realm",
                    "previous={previous_visit}, interval={first_seen}..{last_seen}, closed={closed}"
                );
                assert_eq!(saved_player(&db)["profile"], json!(alpha));
            }
        }
    }

    // A visit ending at first_seen does not overlap, with no later visit to match.
    // An unmatched profile preserves a saved map through coalesce, or stays null.
    for prior_profile in [false, true] {
        let (mut db, mut stream, _) = database();
        let mut alpha = profile("alpha", 101, Some("SynthAlpha"));
        if prior_profile {
            stream.apply(&mut db, vec![EngineEvent::PlayerObserved(alpha.clone())]);
        }
        stream.apply(
            &mut db,
            vec![
                map("Synthetic Previous Hub", AreaKind::OtherHub, 1000),
                session("party-session", 0, 2000, Some(2000)),
            ],
        );
        alpha.run_id = None;
        alpha.scope_id = "party-stream:map:unmapped".into();
        alpha.first_seen = Timestamp(2000);
        alpha.last_seen = Timestamp(2100);
        stream.apply(&mut db, vec![EngineEvent::PlayerObserved(alpha.clone())]);
        assert_eq!(
            saved_player(&db)["map"],
            if prior_profile {
                json!("Synthetic Dungeon")
            } else {
                Value::Null
            },
            "prior_profile={prior_profile}"
        );
        assert_eq!(saved_player(&db)["profile"], json!(alpha));
    }
}

#[test]
fn profile_ending_before_next_visit_keeps_closed_map() {
    let (mut db, mut stream, _) = database();
    let mut alpha = profile("alpha", 101, Some("SynthAlpha"));
    alpha.run_id = None;
    alpha.scope_id = "party-stream:map:realm".into();
    alpha.first_seen = Timestamp(1100);
    alpha.last_seen = Timestamp(1999);
    stream.apply(
        &mut db,
        vec![
            map("Synthetic Realm", AreaKind::Realm, 1000),
            map("Synthetic Later Hub", AreaKind::OtherHub, 2000),
            EngineEvent::PlayerObserved(alpha),
        ],
    );
    assert_eq!(saved_player(&db)["map"], "Synthetic Realm");
}

#[test]
fn older_profile_preserves_newer_name_map_scope_and_profile() {
    let (mut db, mut stream, _) = database();
    let older = profile("alpha", 101, Some("synthalpha,old"));
    let later = run(
        "later-run",
        "party-session",
        "Synthetic Later Dungeon",
        1000,
    );
    let mut newer = older.clone();
    newer.name = Some("SynthAlpha,new".into());
    newer.run_id = Some(later.id.clone());
    newer.scope_id = later.id.clone();
    newer.first_seen = Timestamp(1100);
    newer.last_seen = Timestamp(1200);
    newer.maxed = Some(8);
    stream.apply(
        &mut db,
        vec![
            EngineEvent::RunStarted(later),
            EngineEvent::PlayerObserved(newer.clone()),
            EngineEvent::PlayerObserved(older),
        ],
    );
    assert_eq!(
        saved_player(&db),
        json!({
            "key":"synthalpha","name":"SynthAlpha","first_seen":150,"last_seen":1200,
            "scope":"later-run","map":"Synthetic Later Dungeon","profile":newer,"updated":1200
        })
    );
}

#[test]
fn rebuild_rejoin_only_departs_the_matching_object() {
    for first_fought in [false, true] {
        for rejoin_fought in [false, true] {
            let (mut db, mut stream, run) = database();
            let alpha = profile("alpha", 101, Some("SynthAlpha,first"));
            let mut rejoin = profile("alpha-rejoin", 202, Some("SYNTHALPHA,rejoin"));
            rejoin.first_seen = Timestamp(650);
            rejoin.last_seen = Timestamp(900);
            let mut damage = vec![];
            if first_fought {
                damage.push(player(&alpha, 10.));
            }
            if rejoin_fought {
                damage.push(player(&rejoin, 20.));
            }
            stream.apply(
                &mut db,
                vec![
                    EngineEvent::PlayerObserved(alpha),
                    left(&run, "alpha-left", 101, Some("SynthAlpha"), 600),
                    EngineEvent::PlayerObserved(rejoin),
                    EngineEvent::FightEnded(fight(&run, damage)),
                    end(&run),
                ],
            );
            let rows = members(&db, &run.id);
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0]["departed_at"], 600);
            assert_eq!(rows[0]["left_early"], 1);
            assert_eq!(rows[1]["departed_at"], Value::Null);
            assert_eq!(rows[1]["left_early"], 0);
        }
    }
}

#[test]
fn map_exit_batch_rebuilds_each_run_once_after_all_events() {
    let (mut db, mut stream, run) = database();
    let alpha = profile("alpha", 101, Some("SynthAlpha"));
    let bravo = profile("bravo", 102, Some("SynthBravo"));
    let mut other = run.clone();
    other.id = "other-run".into();
    let mut other_profile = profile("charlie", 103, Some("SynthCharlie"));
    other_profile.run_id = Some(other.id.clone());
    other_profile.scope_id = other.id.clone();
    stream.apply(
        &mut db,
        vec![
            EngineEvent::RunStarted(other.clone()),
            EngineEvent::PlayerObserved(alpha.clone()),
            EngineEvent::PlayerObserved(other_profile.clone()),
        ],
    );
    // A rebuild inserts every member. Count inserts by run without production instrumentation.
    db.execute_batch(
        "CREATE TEMP TABLE member_writes(run_id TEXT);
        CREATE TEMP TRIGGER count_member_writes AFTER INSERT ON party_run_members
        BEGIN INSERT INTO member_writes VALUES(NEW.run_id); END;",
    )
    .unwrap();
    let first = fight(&run, vec![player(&alpha, 30.)]);
    let mut second = fight(&run, vec![player(&alpha, 70.)]);
    second.id = "second-fight".into();
    stream.apply(
        &mut db,
        vec![
            end(&run),
            end(&other),
            end(&run),
            EngineEvent::FightEnded(first),
            EngineEvent::FightEnded(second),
            EngineEvent::PlayerObserved(alpha),
            EngineEvent::PlayerObserved(bravo),
            EngineEvent::PlayerObserved(other_profile),
        ],
    );
    for (id, expected) in [(&run.id, 2), (&other.id, 1)] {
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM member_writes WHERE run_id=?1",
                [id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            expected
        );
    }
    assert_eq!(members(&db, &run.id)[0]["damage"], 100.);
    assert_eq!(members(&db, &run.id)[1]["damage"], 0.);
}

#[test]
fn local_and_nameless_profiles_are_not_stored() {
    let (mut db, mut stream, run) = database();
    let mut local = profile("local", 101, Some("SynthLocal"));
    local.is_local = true;
    stream.apply(
        &mut db,
        vec![
            EngineEvent::PlayerObserved(local),
            EngineEvent::PlayerObserved(profile("nameless", 102, None)),
            EngineEvent::PlayerObserved(profile("empty", 103, Some(""))),
            EngineEvent::PlayerObserved(profile("suffix", 104, Some(",fe14"))),
            end(&run),
        ],
    );
    assert_eq!(count(&db, "player_profiles"), 4);
    assert_eq!(count(&db, "party_players"), 0);
    assert!(members(&db, &run.id).is_empty());
}

#[test]
fn run_end_writes_members_including_zero_damage_profiles() {
    for with_damage in [false, true] {
        let (mut db, mut stream, run) = database();
        let alpha = profile("alpha", 101, Some("SynthAlpha,fe14"));
        let bravo = profile("bravo", 102, Some("SynthBravo"));
        let charlie = profile("charlie", 103, Some("SynthCharlie,fe14"));
        let mut delta = profile("delta", 104, Some("SynthDelta"));
        delta.class_id = None;
        let mut elsewhere = profile("elsewhere", 105, Some("SynthElsewhere"));
        elsewhere.run_id = None;
        elsewhere.scope_id = "another-scope".into();
        let mut events = vec![
            EngineEvent::PlayerObserved(bravo),
            EngineEvent::PlayerObserved(charlie),
            EngineEvent::PlayerObserved(delta),
            EngineEvent::PlayerObserved(elsewhere),
            left(&run, "bravo-name", 999, Some("SYNTHBRAVO,ab,tail"), 300),
            left(&run, "bravo-late", 102, None, 500),
            left(&run, "bravo-first", 102, None, 400),
            left(&run, "charlie-late", 999, Some("synthcharlie"), 600),
            left(&run, "charlie-first", 999, Some("SYNTHCHARLIE,ab"), 350),
        ];
        if with_damage {
            let mut damage = player(&alpha, 100.);
            damage.deaths = 1;
            events.push(EngineEvent::FightEnded(fight(&run, vec![damage])));
            events.push(EngineEvent::PlayerObserved(alpha));
        }
        stream.apply(&mut db, events);
        assert!(members(&db, &run.id).is_empty(), "wait for run end");
        stream.apply(&mut db, vec![end(&run)]);
        let rows = members(&db, &run.id);
        let mut expected = vec![];
        if with_damage {
            // This SELECT is also compared with migration 014 by party_migration.
            expected = member_rows(
                &db,
                &format!("({})", maxed_store::party::RUN_MEMBERS_SQL),
                &run.id,
            );
            assert_eq!(expected.len(), 1);
            assert_eq!(expected[0]["died"], 1);
            assert_eq!(expected[0]["joined"], 100);
        }
        for (key, player, class, departed) in [
            ("synthbravo", "bravo", Some(782), Some(400)),
            ("synthcharlie", "charlie", Some(782), Some(350)),
            ("synthdelta", "delta", None, None),
        ] {
            expected.push(
                json!({"key":key,"run_id":run.id,"player_id":player,"class_id":class,
                "joined":150,"departed_at":departed,"left_early":i32::from(departed.is_some()),
                "died":0,"damage":0.,"share":with_damage.then_some(0.),"boss_share":null}),
            );
        }
        assert_eq!(rows, expected);
    }
}

#[test]
fn late_profile_after_run_end_becomes_member() {
    for (with_damage, with_boss_damage) in [(false, false), (true, false), (true, true)] {
        let (mut db, mut stream, run) = database();
        if with_damage {
            let mut local = profile("local", 101, Some("SynthLocal"));
            local.is_local = true;
            let mut fight = fight(&run, vec![player(&local, 100.)]);
            if with_boss_damage {
                fight.target_kind = Some(TargetKind::Boss);
            }
            stream.apply(&mut db, vec![EngineEvent::FightEnded(fight)]);
        }
        stream.apply(&mut db, vec![end(&run)]);
        assert!(members(&db, &run.id).is_empty());

        let mut bravo = profile("bravo", 102, Some("SynthBravo"));
        bravo.first_seen = Timestamp(800);
        bravo.last_seen = Timestamp(900);
        stream.apply(&mut db, vec![EngineEvent::PlayerObserved(bravo.clone())]);
        let expected = vec![json!({
            "key":"synthbravo","run_id":run.id,"player_id":"bravo","class_id":782,
            "joined":800,"departed_at":null,"left_early":0,"died":0,"damage":0.,
            "share":with_damage.then_some(0.),"boss_share":with_boss_damage.then_some(0.)
        })];
        assert_eq!(members(&db, &run.id), expected);

        stream.apply(&mut db, vec![EngineEvent::PlayerObserved(bravo)]);
        assert_eq!(members(&db, &run.id), expected);
    }
}

#[test]
fn late_fight_for_ended_run_rebuilds_members() {
    let (mut db, mut stream, run) = database();
    let alpha = profile("alpha", 101, Some("SynthAlpha"));
    let bravo = profile("bravo", 102, Some("SynthBravo"));
    stream.apply(&mut db, vec![end(&run)]);
    assert!(members(&db, &run.id).is_empty());
    let mut fight = fight(&run, vec![player(&alpha, 30.)]);
    fight.boss_type = Some(900);
    stream.apply(&mut db, vec![EngineEvent::FightStarted(fight.clone())]);
    assert_eq!(members(&db, &run.id)[0]["damage"], 30.);
    fight.players.push(player(&bravo, 70.));
    fight.total_damage = 100.;
    stream.apply(&mut db, vec![EngineEvent::FightUpdated(fight.clone())]);
    let rows = members(&db, &run.id);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["share"], 0.3);
    assert_eq!(rows[1]["boss_share"], 0.7);
    fight.players.remove(0);
    fight.total_damage = 70.;
    stream.apply(&mut db, vec![EngineEvent::FightEnded(fight)]);
    let rows = members(&db, &run.id);
    assert_eq!(rows.len(), 1, "replacement removes stale members");
    assert_eq!(rows[0]["key"], "synthbravo");
    assert_eq!(rows[0]["share"], 1.);
    assert_eq!(rows[0]["boss_share"], 1.);
}

#[test]
fn named_profile_survives_damage_excluded_from_members() {
    for (name, is_local) in [
        (None, false),
        (Some(""), false),
        (Some(",fe14"), false),
        (Some("SynthAlpha"), true),
    ] {
        let (mut db, mut stream, run) = database();
        let alpha = profile("alpha", 101, Some("SynthAlpha,fe14"));
        let mut damage = player(&alpha, 100.);
        damage.name = name.map(str::to_owned);
        damage.is_local = is_local;
        stream.apply(
            &mut db,
            vec![
                EngineEvent::FightEnded(fight(&run, vec![damage])),
                EngineEvent::PlayerObserved(alpha),
                left(&run, "alpha-left", 101, None, 600),
                end(&run),
            ],
        );
        assert_eq!(count(&db, "pd_run_players"), 1);
        assert_eq!(
            members(&db, &run.id),
            vec![json!({
                "key":"synthalpha","run_id":run.id,"player_id":"alpha","class_id":782,
                "joined":150,"departed_at":600,"left_early":1,"died":0,"damage":0.,
                "share":0.,"boss_share":null
            })]
        );
    }
}

#[test]
fn moving_fight_rebuilds_both_ended_runs() {
    let (mut db, mut stream, run) = database();
    let alpha = profile("alpha", 101, Some("SynthAlpha"));
    let mut other = run.clone();
    other.id = "other-run".into();
    let mut moved = fight(&run, vec![player(&alpha, 100.)]);
    stream.apply(
        &mut db,
        vec![
            EngineEvent::RunStarted(other.clone()),
            EngineEvent::FightEnded(moved.clone()),
            end(&run),
            end(&other),
        ],
    );
    let original = members(&db, &run.id);
    assert_eq!(original.len(), 1);
    assert_eq!(original[0]["damage"], 100.);
    assert!(members(&db, &other.id).is_empty());

    moved.run_id = Some(other.id.clone());
    moved.scope_id = Some(other.id.clone());
    stream.apply(&mut db, vec![EngineEvent::FightUpdated(moved)]);
    assert!(members(&db, &run.id).is_empty());
    let mut expected = original;
    expected[0]["run_id"] = json!(other.id);
    assert_eq!(members(&db, &other.id), expected);
}

#[test]
fn run_end_is_idempotent() {
    let (mut db, mut stream, run) = database();
    let alpha = profile("alpha", 101, Some("SynthAlpha"));
    let bravo = profile("bravo", 102, Some("SynthBravo"));
    stream.apply(
        &mut db,
        vec![
            EngineEvent::PlayerObserved(bravo),
            EngineEvent::FightEnded(fight(&run, vec![player(&alpha, 100.)])),
            left(&run, "alpha-left", 101, None, 600),
            end(&run),
        ],
    );
    let original = members(&db, &run.id);
    assert_eq!(original.len(), 2);
    let mut other = run.clone();
    other.id = "other-run".into();
    let mut other_profile = alpha;
    other_profile.run_id = Some(other.id.clone());
    other_profile.scope_id = other.id.clone();
    stream.apply(
        &mut db,
        vec![
            EngineEvent::RunStarted(other.clone()),
            EngineEvent::PlayerObserved(other_profile),
            end(&other),
        ],
    );
    let other_members = members(&db, &other.id);
    assert_eq!(other_members.len(), 1);
    // New sequence numbers exercise the projection again, not receipt deduplication.
    stream.apply(&mut db, vec![end(&run), end(&run)]);
    assert_eq!(members(&db, &run.id), original);
    assert_eq!(members(&db, &other.id), other_members);
}

#[test]
fn zero_damage_profile_has_zero_shares_when_run_has_boss_damage() {
    for (kind, boss_type) in [
        (TargetKind::Boss, None),
        (TargetKind::EventBoss, None),
        (TargetKind::Ordinary, Some(900)),
    ] {
        let (mut db, mut stream, run) = database();
        let mut local = profile("local", 101, Some("SynthLocal"));
        local.is_local = true;
        let bravo = profile("bravo", 102, Some("SynthBravo"));
        let mut fight = fight(&run, vec![player(&local, 100.)]);
        fight.target_kind = Some(kind);
        fight.boss_type = boss_type;
        stream.apply(
            &mut db,
            vec![
                EngineEvent::PlayerObserved(bravo),
                EngineEvent::FightEnded(fight),
                end(&run),
            ],
        );
        let rows = members(&db, &run.id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["key"], "synthbravo");
        assert_eq!(rows[0]["damage"], 0.);
        assert_eq!(rows[0]["share"], 0.);
        assert_eq!(rows[0]["boss_share"], 0.);
    }
}

use crate::common;
use crate::common::account_scope_fixture::ACCOUNT;
use common::stats::*;
use maxed_types::*;

const C: &str = "synth-character";

fn page(limit: u16) -> StatsSessionFilter {
    StatsSessionFilter {
        range: None,
        character_id: None,
        include_empty: true,
        cursor: None,
        limit,
    }
}

/// An earlier session on another stream leaves a baseline of `fame`.
fn baseline(db: &mut rusqlite::Connection, fame_value: i32) {
    let mut s = Stream::new("earlier");
    s.apply(
        db,
        vec![
            session("s0", 0, MIN, Some(MIN)),
            character(C, "SynthAlpha", 0),
            fame(C, fame_value, MIN / 2),
        ],
    );
}

fn detail(path: &std::path::Path, id: &str, character: Option<&str>) -> StatsSessionDetail {
    crate::common::account_scope_fixture::open_reader(path)
        .unwrap()
        .stats_session(StatsSessionRequest {
            id: id.into(),
            character_id: character.map(Into::into),
        })
        .unwrap()
        .unwrap()
}

#[test]
fn session_fame_matches_home_and_visits_add_up() {
    let (_dir, path, mut db) = database();
    baseline(&mut db, 1000);
    let mut s = Stream::new("live");
    let mut halls = run("r1", "s1", "Synthetic Halls", 40 * MIN);
    s.apply(
        &mut db,
        vec![
            session("s1", 10 * MIN, 10 * MIN, None),
            character(C, "SynthAlpha", 10 * MIN),
            map("Nexus", AreaKind::Nexus, 10 * MIN),
            fame(C, 1000, 11 * MIN),
            map("Realm of the Mad God", AreaKind::Realm, 12 * MIN),
            fame(C, 1300, 30 * MIN),
            map("Synthetic Halls", AreaKind::Dungeon, 40 * MIN),
            EngineEvent::RunStarted(halls.clone()),
            fame(C, 1400, 50 * MIN),
        ],
    );
    halls.character_id = Some(C.into());
    halls.ended = Some(Timestamp(55 * MIN));
    halls.outcome = Some(Outcome::Cleared);
    s.apply(
        &mut db,
        vec![
            EngineEvent::RunEnded(halls),
            map("Nexus", AreaKind::Nexus, 55 * MIN),
            session("s1", 10 * MIN, 60 * MIN, Some(60 * MIN)),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let home = reader
        .home_summary(HomeScope::Session { id: "s1".into() }, Some(ACCOUNT))
        .unwrap();
    let rows = reader.stats_sessions(page(10)).unwrap().rows;
    let s1 = rows.iter().find(|r| r.id == "s1").unwrap();
    assert_eq!(home.fame_delta, Some(400));
    assert_eq!((s1.fame, s1.missing_baseline), (400, false));
    assert_eq!((s1.runs, s1.clears, s1.deaths), (1, 1, 0));
    assert_eq!(s1.duration_micros, 50 * MIN);
    assert_eq!(s1.fame_per_hour, Some(480.0));
    let d = detail(&path, "s1", None);
    assert_eq!(d.maps.iter().map(|m| m.fame).sum::<i64>(), 400);
    assert_eq!(d.unrecorded_fame, None);
    assert_eq!(d.runs.len(), 1);
}

#[test]
fn last_visit_includes_sample_at_session_end() {
    let (_dir, path, mut db) = database();
    baseline(&mut db, 500);
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            map("Realm of the Mad God", AreaKind::Realm, MIN),
            fame(C, 500, 2 * MIN),
            fame(C, 560, 10 * MIN),
            session("s1", 0, 10 * MIN, Some(10 * MIN)),
        ],
    );
    let d = detail(&path, "s1", None);
    assert_eq!(d.session.fame, 60);
    assert_eq!(d.maps[0].fame, 60);
    assert_eq!(d.unrecorded_fame, None);
}

#[test]
fn fame_before_the_first_visit_is_unrecorded() {
    let (_dir, path, mut db) = database();
    baseline(&mut db, 500);
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            fame(C, 520, MIN),
            fame(C, 550, 3 * MIN / 2),
            map("Realm of the Mad God", AreaKind::Realm, 2 * MIN),
            fame(C, 600, 5 * MIN),
            session("s1", 0, 6 * MIN, Some(6 * MIN)),
        ],
    );
    // The earlier session's 500 is not a baseline: fame between sessions is not credited.
    let d = detail(&path, "s1", None);
    assert_eq!(d.session.fame, 80);
    assert_eq!(d.maps[0].fame, 50);
    assert_eq!(d.unrecorded_fame, Some(30));
}

#[test]
fn session_fame_counts_from_the_first_sample() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            character(C, "SynthAlpha", 0),
            map("Realm of the Mad God", AreaKind::Realm, 0),
            fame(C, 100, MIN),
            fame(C, 160, 4 * MIN),
            session("s1", 0, 5 * MIN, Some(5 * MIN)),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let home = reader
        .home_summary(HomeScope::Session { id: "s1".into() }, Some(ACCOUNT))
        .unwrap();
    assert_eq!(
        (home.fame_delta, home.fame_missing_baselines),
        (Some(60), 0)
    );
    let row = &reader.stats_sessions(page(10)).unwrap().rows[0];
    assert_eq!((row.fame, row.missing_baseline), (60, false));
}

#[test]
fn empty_sessions_are_hidden_unless_requested() {
    let (_dir, path, mut db) = database();
    baseline(&mut db, 700);
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 2 * MIN, 2 * MIN, None),
            map("Nexus", AreaKind::Nexus, 2 * MIN),
            fame(C, 700, 3 * MIN),
            session("s1", 2 * MIN, 4 * MIN, Some(4 * MIN)),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let hidden = reader
        .stats_sessions(StatsSessionFilter {
            include_empty: false,
            ..page(10)
        })
        .unwrap();
    assert!(hidden.rows.is_empty());
    assert_eq!(hidden.next, None);
    assert_eq!(reader.stats_sessions(page(10)).unwrap().rows.len(), 2);
}

#[test]
fn pages_newest_first_with_a_keyset_cursor() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    for i in 1..=5u64 {
        s.apply(
            &mut db,
            vec![session(
                &format!("s{i}"),
                i * MIN,
                i * MIN,
                Some(i * MIN + 1),
            )],
        );
    }
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut filter = page(2);
    let mut seen = vec![];
    loop {
        let result = reader.stats_sessions(filter.clone()).unwrap();
        seen.push(result.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>());
        match result.next {
            Some(next) => filter.cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, vec![vec!["s5", "s4"], vec!["s3", "s2"], vec!["s1"]]);
}

#[test]
fn character_filter_limits_sessions_and_totals() {
    let (_dir, path, mut db) = database();
    let mut a = Stream::new("a");
    a.apply(
        &mut db,
        vec![
            session("s1", MIN, MIN, None),
            character(C, "SynthAlpha", MIN),
            fame(C, 10, MIN),
            session("s1", MIN, 2 * MIN, Some(2 * MIN)),
        ],
    );
    let mut b = Stream::new("b");
    b.apply(
        &mut db,
        vec![
            session("s2", 3 * MIN, 3 * MIN, None),
            character("synth-other", "SynthBravo", 3 * MIN),
            fame("synth-other", 20, 3 * MIN),
            session("s2", 3 * MIN, 4 * MIN, Some(4 * MIN)),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mine = reader
        .stats_sessions(StatsSessionFilter {
            character_id: Some(C.into()),
            ..page(10)
        })
        .unwrap();
    assert_eq!(
        mine.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["s1"]
    );
    assert_eq!(mine.characters.len(), 2);
}

#[test]
fn detail_for_a_character_absent_from_the_session_shows_zeros() {
    let (_dir, path, mut db) = database();
    let mut a = Stream::new("a");
    a.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            character(C, "SynthAlpha", 0),
            character("synth-other", "SynthBravo", 0),
            map("Realm of the Mad God", AreaKind::Realm, 0),
            fame(C, 10, MIN),
            fame(C, 40, 2 * MIN),
            session("s1", 0, 3 * MIN, Some(3 * MIN)),
        ],
    );
    let d = detail(&path, "s1", Some("synth-other"));
    assert_eq!((d.session.fame, d.session.runs), (0, 0));
    assert!(d.maps.is_empty() && d.runs.is_empty());
    assert_eq!(d.unrecorded_fame, None);
}

#[test]
fn in_progress_session_runs_until_last_traffic() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(&mut db, vec![session("s1", MIN, 8 * MIN, None)]);
    let row = &crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .stats_sessions(page(1))
        .unwrap()
        .rows[0];
    assert_eq!((row.ended, row.duration_micros), (None, 7 * MIN));
}

#[test]
fn absent_and_invalid_sessions() {
    let (_dir, path, _db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    assert_eq!(
        reader
            .stats_session(StatsSessionRequest {
                id: "absent".into(),
                character_id: None
            })
            .unwrap(),
        None
    );
    assert!(reader
        .stats_session(StatsSessionRequest {
            id: String::new(),
            character_id: None
        })
        .is_err());
    for limit in [0, 101] {
        assert!(reader.stats_sessions(page(limit)).is_err());
    }
}

#[test]
fn party_lists_runs_with_their_totals() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    let mut r1 = run("r1", "s1", "Synthetic Halls", MIN);
    r1.ended = Some(Timestamp(2 * MIN));
    s.apply(
        &mut db,
        vec![session("s1", 0, 0, None), EngineEvent::RunEnded(r1)],
    );
    let party = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .stats_session_party("s1", None)
        .unwrap();
    assert_eq!(party.len(), 1);
    assert_eq!(party[0].run.id, "r1");
    assert_eq!(party[0].totals, None);
}

#[test]
fn sessions_export_stops_at_ten_thousand() {
    let (_dir, path, db) = database();
    db.execute_batch(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<10001)
         INSERT INTO play_sessions SELECT 's'||i,i,i,i,
             json_object('id','s'||i,'started',CAST(i AS TEXT),'last_traffic',CAST(i AS TEXT),
                 'ended',CAST(i AS TEXT),'issues',json('[]'))
         FROM n;",
    )
    .unwrap();
    let (rows, truncated) = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .stats_sessions_export(
            StatsFilter {
                range: None,
                character_id: None,
            },
            true,
        )
        .unwrap();
    assert_eq!((rows.len(), truncated), (MAX_STATS_EXPORT_SESSIONS, true));
    assert_eq!(rows[0].id, "s10001");
}

fn observed_bag(id: &str, session: &str, at: u64, rank: Option<u8>) -> DropView {
    let mut bag = demo::demo_snapshot().drops.remove(0);
    bag.id = id.into();
    bag.session_id = session.into();
    bag.run_id = None;
    bag.at = Timestamp(at);
    bag.bag_rank = rank;
    bag.bag_type = match rank {
        Some(255) => 1292,
        Some(90) => 1708,
        Some(80) => 1295,
        Some(70) => 1294,
        Some(10) => 1709,
        _ => 65000,
    };
    bag.items.clear();
    bag.contents.clear();
    bag.contents_known = false;
    bag
}

#[test]
fn session_loot_matches_feed_including_unknown_and_legacy_low_tiers() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("synthetic-loot");
    s.apply(&mut db, vec![session("s1", 0, 0, None)]);
    // Current capture excludes brown bags. If a legacy low-tier observation was retained,
    // Feed still displays it under Any bag, so Stats must count it too. Unknown contents
    // and ranks also remain visible; this is not a count of picked-up items.
    for (index, rank) in [Some(255), Some(90), Some(80), Some(70), Some(10), None]
        .into_iter()
        .enumerate()
    {
        s.apply(
            &mut db,
            vec![EngineEvent::BagDropped(observed_bag(
                &format!("synth-bag-{index}"),
                "s1",
                MIN,
                rank,
            ))],
        );
    }
    s.apply(
        &mut db,
        vec![
            EngineEvent::DropUpdated(observed_bag("synth-bag-0", "s1", MIN, Some(255))),
            session("s1", 0, 30 * MIN, Some(30 * MIN)),
            session("s2", 31 * MIN, 31 * MIN, None),
            EngineEvent::BagDropped(observed_bag("other-session", "s2", 32 * MIN, Some(80))),
            session("s2", 31 * MIN, 60 * MIN, None),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let filter = DropFilter {
        min_rank: None,
        place: None,
        item_type: None,
        enchanted_only: false,
        range: Some(TimeRange {
            start: Timestamp(0),
            end: Timestamp(30 * MIN),
        }),
        cursor: None,
        limit: MAX_LOOT_PAGE,
    };
    let all = reader.list_drops(filter.clone()).unwrap();
    let notable = reader
        .list_drops(DropFilter {
            min_rank: Some(80),
            ..filter
        })
        .unwrap();
    let rows = reader
        .stats_sessions(StatsSessionFilter {
            include_empty: false,
            ..page(10)
        })
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 2, "loot-only sessions are not empty");
    let first = rows.iter().find(|row| row.id == "s1").unwrap();
    assert_eq!((first.bags, first.notable_bags), (6, 3));
    assert_eq!(first.bags as usize, all.rows.len());
    assert_eq!(first.notable_bags as usize, notable.rows.len());
    assert_eq!(
        (first.bags_per_hour, first.notable_bags_per_hour),
        (Some(12.0), Some(6.0))
    );
    assert_eq!(rows[0].bags, 1);
    assert_eq!(rows[0].notable_bags, 1);
    assert_eq!(rows[0].bags_per_hour, Some(60.0 / 29.0));
    assert_eq!(detail(&path, "s1", None).session, *first);
}

#[test]
fn session_loot_rates_require_five_minutes_and_preserve_zero() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("synthetic-short-loot");
    for (id, duration, rank) in [
        ("short", 5 * MIN - 1, Some(255)),
        ("boundary", 5 * MIN, Some(80)),
        ("empty", 5 * MIN, None),
    ] {
        s.apply(&mut db, vec![session(id, 0, 0, None)]);
        if rank.is_some() {
            s.apply(
                &mut db,
                vec![EngineEvent::BagDropped(observed_bag(id, id, MIN, rank))],
            );
        }
        s.apply(&mut db, vec![session(id, 0, duration, Some(duration))]);
    }
    let short = detail(&path, "short", None).session;
    assert_eq!((short.bags, short.notable_bags), (1, 1));
    assert_eq!(
        (short.bags_per_hour, short.notable_bags_per_hour),
        (None, None)
    );
    let boundary = detail(&path, "boundary", None).session;
    assert_eq!(
        (boundary.bags_per_hour, boundary.notable_bags_per_hour),
        (Some(12.0), Some(12.0))
    );
    let empty = detail(&path, "empty", None).session;
    assert_eq!((empty.bags, empty.notable_bags), (0, 0));
    assert_eq!(
        (empty.bags_per_hour, empty.notable_bags_per_hour),
        (Some(0.0), Some(0.0))
    );
}

#[test]
fn session_loot_character_filter_uses_runs_and_containing_visits() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("synthetic-character-loot");
    let mut other_run = run("other-run", "s1", "Synthetic Cave", 2 * MIN);
    other_run.character_id = Some("synth-other".into());
    let mut run_bag = observed_bag("run-bag", "s1", 3 * MIN, Some(255));
    run_bag.run_id = Some(other_run.id.clone());
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            // No character evidence for this bag; only the all-character total includes it.
            EngineEvent::BagDropped(observed_bag("unattributed", "s1", 0, Some(255))),
            map("Realm of the Mad God", AreaKind::Realm, MIN),
            character(C, "SynthAlpha", MIN),
            EngineEvent::BagDropped(observed_bag("realm-bag", "s1", 2 * MIN, Some(80))),
            map("Synthetic Cave", AreaKind::Dungeon, 3 * MIN),
            character("synth-other", "SynthBravo", 3 * MIN),
            EngineEvent::RunStarted(other_run),
            EngineEvent::BagDropped(run_bag),
            session("s1", 0, 10 * MIN, Some(10 * MIN)),
        ],
    );
    assert_eq!(detail(&path, "s1", None).session.bags, 3);
    // A recorded unknown owner cannot be replaced by mutable visit identity.
    assert_eq!(detail(&path, "s1", Some(C)).session.bags, 0);
    // Exercise the legacy visit fallback with a pre-017 observation.
    db.execute(
        "DELETE FROM observation_characters WHERE kind='drop' AND observation_id='realm-bag'",
        [],
    )
    .unwrap();

    let mine = detail(&path, "s1", Some(C)).session;
    assert_eq!((mine.bags, mine.notable_bags), (1, 1));
    let other = detail(&path, "s1", Some("synth-other")).session;
    assert_eq!((other.bags, other.notable_bags), (1, 1));
    let absent = detail(&path, "s1", Some("absent")).session;
    assert_eq!((absent.bags, absent.notable_bags), (0, 0));
}

#[test]
fn immutable_observation_owners_survive_missing_or_changed_visits() {
    let (_dir, _path, db) = database();
    db.execute_batch("INSERT INTO play_sessions VALUES('s',0,100,100,'{}');
        INSERT INTO characters(id,observed_at,json) VALUES('owner',0,'{}'),('alias',0,'{}'),('later',0,'{}');
        INSERT INTO character_aliases VALUES('alias','owner');
        INSERT INTO fights(id,session_id,started,complete_capture,json) VALUES('f','s',10,1,'{}');
        INSERT INTO drops(id,session_id,at,bag_type,bag_rank,json) VALUES('d','s',10,1,80,'{}');
        INSERT INTO observation_characters VALUES('fight','f','alias'),('drop','d','alias');").unwrap();
    let fight_sql = format!(
        "WITH {} SELECT character_id FROM fight_character WHERE fight_id='f'",
        include_str!("../src/fight_character.sql")
    );
    let totals = include_str!("../src/stats_session_totals.sql");
    for sql in [&fight_sql, totals] {
        let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let args = vec![rusqlite::types::Value::Null; statement.parameter_count()];
        let plan = statement
            .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        for index in [
            "sqlite_autoindex_observation_characters_1",
            "sqlite_autoindex_character_aliases_1",
            "map_visits_session",
        ] {
            assert!(plan.contains(index), "{plan}");
        }
        for line in plan.lines() {
            assert!(
                !["SCAN o", "SCAN a", "SCAN v", "SCAN d", "SCAN r", "SCAN f"]
                    .iter()
                    .any(|scan| line == *scan || line.starts_with(&format!("{scan} "))),
                "{plan}"
            );
        }
    }
    let check = |expected: &str| {
        assert_eq!(
            db.query_row(&fight_sql, [], |r| r.get::<_, String>(0))
                .unwrap(),
            expected
        );
        let (active, bags): (bool, u32) = db
            .query_row(totals, ["s", expected], |r| Ok((r.get(5)?, r.get(6)?)))
            .unwrap();
        assert!(active);
        assert_eq!(bags, 1);
    };
    check("owner");
    db.execute_batch("INSERT INTO map_visits(id,stream_id,session_id,character_id,map,kind,started,ended,source,json)
        VALUES('v','s','s','later','Realm','realm',0,100,'live','{}');").unwrap();
    check("owner");
    // Only absence of the first-event record enables the pre-017 visit fallback.
    db.execute_batch("DELETE FROM observation_characters;")
        .unwrap();
    check("later");
    db.execute_batch(
        "INSERT INTO observation_characters VALUES('fight','f',NULL),('drop','d',NULL);",
    )
    .unwrap();
    assert_eq!(
        db.query_row(&fight_sql, [], |r| r.get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
    assert_eq!(
        db.query_row(totals, ["s", "later"], |r| r.get::<_, u32>(6))
            .unwrap(),
        0
    );
}

#[test]
fn death_only_sessions_are_active_and_use_run_character_fallback() {
    let (_dir, _path, db) = database();
    db.execute_batch(
        "INSERT INTO play_sessions VALUES('s',0,100,100,'{}'),('run-session',0,100,100,'{}');
        INSERT INTO characters(id,observed_at,json) VALUES('owner',0,'{}'),('other',0,'{}');
        INSERT INTO runs(id,session_id,character_id,dungeon,started,complete_capture,json)
            VALUES('r','run-session','owner','Synthetic',0,1,'{}');
        INSERT INTO deaths(stream_id,sequence,character_id,run_id,at,session_id)
            VALUES('s',1,NULL,'r',10,'s'),('s',2,'other','r',20,'s');",
    )
    .unwrap();
    for character in ["owner", "other"] {
        let (deaths, active): (u32, bool) = db
            .query_row(
                include_str!("../src/stats_session_totals.sql"),
                ["s", character],
                |r| Ok((r.get(4)?, r.get(5)?)),
            )
            .unwrap();
        assert_eq!(deaths, 1);
        assert!(active);
    }
}

#[test]
fn ownership_revisions_preserve_legacy_unknown_and_known_fights_and_drops() {
    for mode in ["legacy", "unknown", "known"] {
        let (_dir, path, mut db) = database();
        let mut stream = Stream::new("ownership-revisions");
        stream.apply(
            &mut db,
            vec![
                session("s", 0, 10 * MIN, None),
                character(C, "Synthetic", 0),
                character("other", "Other", 0),
                map("Realm", AreaKind::Realm, 0),
            ],
        );
        // Set a containing legacy visit independently of the first-event owner.
        db.execute("UPDATE map_visits SET character_id=?1", [C])
            .unwrap();
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.id = "f".into();
        fight.session_id = "s".into();
        fight.run_id = None;
        fight.owner_character_id = (mode == "known").then(|| C.into());
        fight.players.retain(|p| p.is_local);
        fight.ended = Some(Timestamp(fight.started.0 + 120_000_000));
        fight.boss_type = Some(42);
        fight.target_type = Some(42);
        fight.players[0].dps = Some(1000.0);
        let mut drop = observed_bag("d", "s", MIN, Some(80));
        drop.owner_character_id = fight.owner_character_id.clone();
        stream.apply(
            &mut db,
            vec![
                EngineEvent::FightUpdated(fight.clone()),
                EngineEvent::BagDropped(drop.clone()),
            ],
        );
        if mode == "legacy" {
            // Retain valid saved DTOs but remove ownership to reproduce the pre-017 shape.
            db.execute("DELETE FROM observation_characters", [])
                .unwrap();
        }
        for revised_owner in [Some("other".to_owned()), None] {
            fight.owner_character_id = revised_owner.clone();
            drop.owner_character_id = revised_owner;
            stream.apply(
                &mut db,
                vec![
                    EngineEvent::FightUpdated(fight.clone()),
                    EngineEvent::DropUpdated(drop.clone()),
                ],
            );
            for kind in ["fight", "drop"] {
                let (count, owner): (u32, Option<String>) = db.query_row(
                    "SELECT count(*),max(character_id) FROM observation_characters WHERE kind=?1",
                    [kind], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
                assert_eq!(count, u32::from(mode != "legacy"), "{mode}/{kind}");
                assert_eq!(
                    owner.as_deref(),
                    (mode == "known").then_some(C),
                    "{mode}/{kind}"
                );
            }
            let resolved: Option<String> = db
                .query_row(
                    &format!(
                        "WITH {} SELECT character_id FROM fight_character WHERE fight_id='f'",
                        include_str!("../src/fight_character.sql")
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                resolved.as_deref(),
                (mode != "unknown").then_some(C),
                "{mode}"
            );
            let bags: u32 = db
                .query_row(
                    include_str!("../src/stats_session_totals.sql"),
                    ["s", C],
                    |r| r.get(6),
                )
                .unwrap();
            assert_eq!(bags, u32::from(mode != "unknown"), "{mode}");
            let history = maxed_store::open_reader(&path)
                .unwrap()
                .boss_history(BossHistoryRequest {
                    range: None,
                    character_id: Some(C.into()),
                    target_types: None,
                })
                .unwrap();
            assert_eq!(history.rows.is_empty(), mode == "unknown", "{mode}");
        }
    }
}

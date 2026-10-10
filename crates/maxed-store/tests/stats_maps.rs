mod common;
use common::stats::*;
use maxed_types::*;

const C: &str = "synth-character";

fn all() -> StatsFilter {
    StatsFilter {
        range: None,
        character_id: None,
    }
}

/// Nexus 0–10, Realm 10–40, Vault 40–42, Synthetic Halls 42–62 (cleared), unknown 62–63.
fn one_session(db: &mut rusqlite::Connection) {
    let mut s = Stream::new("live");
    let mut halls = run("r1", "s1", "Synthetic Halls", 42 * MIN);
    s.apply(
        db,
        vec![
            session("s1", 0, 0, None),
            character(C, "SynthAlpha", 0),
            map("Nexus", AreaKind::Nexus, 0),
            fame(C, 100, MIN),
            map("Realm of the Mad God", AreaKind::Realm, 10 * MIN),
            fame(C, 250, 20 * MIN),
            fame(C, 400, 39 * MIN),
            map("Vault", AreaKind::OtherHub, 40 * MIN),
            map("Synthetic Halls", AreaKind::Dungeon, 42 * MIN),
            EngineEvent::RunStarted(halls.clone()),
            fame(C, 450, 50 * MIN),
            fame(C, 500, 61 * MIN),
        ],
    );
    halls.character_id = Some(C.into());
    halls.completed_at = Some(Timestamp(60 * MIN));
    halls.ended = Some(Timestamp(62 * MIN));
    halls.outcome = Some(Outcome::Cleared);
    s.apply(
        db,
        vec![
            EngineEvent::RunEnded(halls),
            map("Unmapped Place", AreaKind::Unknown, 62 * MIN),
            session("s1", 0, 63 * MIN, Some(63 * MIN)),
        ],
    );
}

#[test]
fn maps_are_grouped_and_ranked_by_fame_per_hour() {
    let (_dir, path, mut db) = database();
    one_session(&mut db);
    let maps = maxed_store::open_reader(&path)
        .unwrap()
        .stats_maps(all())
        .unwrap();
    let rows: Vec<_> = maps
        .rows
        .iter()
        .map(|r| {
            (
                r.group,
                r.map.as_str(),
                r.visits,
                r.time_micros,
                r.fame,
                r.missing_baseline,
                r.fame_per_hour,
                r.clears,
                r.deaths,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                StatsMapGroup::Realm,
                "Realm",
                1,
                30 * MIN,
                300,
                false,
                Some(600.0),
                None,
                None
            ),
            (
                StatsMapGroup::Dungeon,
                "Synthetic Halls",
                1,
                20 * MIN,
                100,
                false,
                Some(300.0),
                Some(1),
                Some(0)
            ),
            // The Nexus visit holds the character's first-ever sample: a missing baseline.
            (
                StatsMapGroup::Hubs,
                "Hubs",
                2,
                12 * MIN,
                0,
                true,
                Some(0.0),
                None,
                None
            ),
            // Under five minutes: no rate, sorted last.
            (
                StatsMapGroup::Other,
                "Other",
                1,
                MIN,
                0,
                false,
                None,
                None,
                None
            ),
        ]
    );
    assert_eq!(maps.tracking_since, Some(Timestamp(0)));
    assert!(!maps.truncated);
    assert_eq!(maps.characters.len(), 1);
}

#[test]
fn dungeon_deaths_match_outcomes_in_maps_dungeons_and_session_detail() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(&mut db, vec![session("s1", 0, 0, None)]);
    for (id, started, outcome) in [
        ("r1", MIN, Outcome::Cleared),
        ("r2", 20 * MIN, Outcome::Died),
    ] {
        let mut halls = run(id, "s1", "Synthetic Halls", started);
        s.apply(
            &mut db,
            vec![
                map("Synthetic Halls", AreaKind::Dungeon, started),
                EngineEvent::RunStarted(halls.clone()),
            ],
        );
        halls.ended = Some(Timestamp(started + 10 * MIN));
        halls.died_at = halls.ended;
        if outcome == Outcome::Cleared {
            // A death after the clear does not change the run's outcome.
            halls.completed_at = Some(Timestamp(started + 9 * MIN));
        }
        halls.outcome = Some(outcome);
        s.apply(&mut db, vec![EngineEvent::RunEnded(halls)]);
    }
    s.apply(&mut db, vec![session("s1", 0, 30 * MIN, Some(30 * MIN))]);

    let reader = maxed_store::open_reader(&path).unwrap();
    let maps = reader.stats_maps(all()).unwrap();
    let dungeons = reader.stats_dungeons(all()).unwrap();
    let detail = reader
        .stats_session(StatsSessionRequest {
            id: "s1".into(),
            character_id: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(dungeons.rows.len(), 1);
    let dungeon = &dungeons.rows[0];
    assert_eq!(
        (
            dungeon.dungeon.as_str(),
            dungeon.runs,
            dungeon.clears,
            dungeon.deaths
        ),
        ("Synthetic Halls", 2, 1, 1)
    );
    for rows in [&maps.rows, &detail.maps] {
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.map, dungeon.dungeon);
        assert_eq!(row.visits, dungeon.runs);
        assert_eq!(row.clears, Some(dungeon.clears));
        assert_eq!(row.deaths, Some(dungeon.deaths));
    }
}

#[test]
fn sample_at_a_map_change_belongs_to_the_next_map() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            character(C, "SynthAlpha", 0),
            fame(C, 100, 0),
            map("Nexus", AreaKind::Nexus, MIN),
            map("Realm of the Mad God", AreaKind::Realm, 10 * MIN),
            fame(C, 130, 10 * MIN),
            session("s1", 0, 20 * MIN, Some(20 * MIN)),
        ],
    );
    let maps = maxed_store::open_reader(&path)
        .unwrap()
        .stats_maps(all())
        .unwrap();
    let fame: Vec<_> = maps.rows.iter().map(|r| (r.map.as_str(), r.fame)).collect();
    assert!(fame.contains(&("Realm", 30)));
    assert!(fame.contains(&("Hubs", 0)));
}

#[test]
fn visits_without_character_count_time_but_no_fame() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            map("Realm of the Mad God", AreaKind::Realm, 0),
            session("s1", 0, 10 * MIN, Some(10 * MIN)),
        ],
    );
    let maps = maxed_store::open_reader(&path)
        .unwrap()
        .stats_maps(all())
        .unwrap();
    assert_eq!((maps.rows[0].time_micros, maps.rows[0].fame), (10 * MIN, 0));
}

#[test]
fn open_visit_runs_until_the_session_last_traffic() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            map("Realm of the Mad God", AreaKind::Realm, MIN),
            session("s1", 0, 8 * MIN, None),
        ],
    );
    let maps = maxed_store::open_reader(&path)
        .unwrap()
        .stats_maps(all())
        .unwrap();
    assert_eq!(maps.rows[0].time_micros, 7 * MIN);
}

#[test]
fn range_and_character_filters_select_visits() {
    let (_dir, path, mut db) = database();
    one_session(&mut db);
    let reader = maxed_store::open_reader(&path).unwrap();
    let late = reader
        .stats_maps(StatsFilter {
            range: Some(TimeRange {
                start: Timestamp(41 * MIN),
                end: Timestamp(62 * MIN),
            }),
            character_id: None,
        })
        .unwrap();
    assert_eq!(
        late.rows.iter().map(|r| r.map.as_str()).collect::<Vec<_>>(),
        vec!["Synthetic Halls"]
    );
    let other = reader
        .stats_maps(StatsFilter {
            range: None,
            character_id: Some("synth-other".into()),
        })
        .unwrap();
    assert!(other.rows.is_empty());
}

#[test]
fn backfilled_visit_stops_at_its_run_end() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            character(C, "SynthAlpha", 0),
            fame(C, 100, 0),
            fame(C, 160, 4 * MIN),
            // Realm fame after the run must not count toward the dungeon.
            fame(C, 900, 8 * MIN),
            session("s1", 0, 9 * MIN, Some(9 * MIN)),
        ],
    );
    db.execute_batch(
        "INSERT INTO map_visits VALUES('backfill:r1','backfill','s1','synth-character',NULL,
            'Synthetic Halls','dungeon',NULL,60000000,300000000,'backfill','{}');",
    )
    .unwrap();
    let maps = maxed_store::open_reader(&path)
        .unwrap()
        .stats_maps(all())
        .unwrap();
    assert_eq!(maps.rows[0].fame, 60);
    // Only backfilled history: tracking has not started.
    assert_eq!(maps.tracking_since, None);
}

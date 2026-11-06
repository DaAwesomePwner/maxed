use crate::common::stats::{database, run, session, Stream};
use maxed_store::open_reader;
use maxed_types::*;
use rusqlite::{params, Connection};

const SECOND: u64 = 1_000_000;

#[test]
fn boss_history_party_average_plan_searches_scoped_fights() {
    let (_dir, _path, mut db) = database();
    seed(&mut db, vec![fight("scoped", "alpha", 10, Some(1000.0))]);
    let sql = include_str!("../src/boss_history.sql")
        .replace(
            "/*fight_character*/",
            include_str!("../src/fight_character.sql"),
        )
        .replace(
            "/*party_average_scoped*/",
            include_str!("../src/party_average_scoped.sql"),
        );
    for (lo, hi, character, targets) in [
        (None, None, None, None),
        (Some(0), Some(600_000_000_i64), Some("alpha"), Some("[42]")),
    ] {
        let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = statement
            .query_map(params![lo, hi, character, targets], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        eprintln!("Boss history targets={targets:?}:\n{plan}");
        assert!(
            plan.lines()
                .any(|line| line.contains("SEARCH fight_players") && line.contains("fight_id=?")),
            "{plan}"
        );
        assert!(!plan.contains("SCAN fight_players"), "{plan}");
    }
}

fn all() -> BossHistoryRequest {
    BossHistoryRequest {
        range: None,
        character_id: None,
        target_types: None,
    }
}

fn fight(id: &str, character: &str, started: u64, dps: Option<f64>) -> FightView {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = id.into();
    fight.session_id = "synthetic-session".into();
    fight.run_id = Some(format!("synthetic-run-{character}"));
    fight.target_type = Some(42);
    fight.boss_type = Some(42);
    fight.started = Timestamp(started);
    fight.ended = Some(Timestamp(started + 120 * SECOND));
    fight.kill = None;
    fight.issues.clear();
    fight.players.truncate(1);
    fight.players[0].is_local = true;
    fight.players[0].dps = dps;
    fight.players[0].rank = Some(2);
    fight
}

fn seed(db: &mut Connection, fights: Vec<FightView>) {
    let mut stream = Stream::new("synthetic-boss-history");
    let mut events = vec![session("synthetic-session", 0, 0, None)];
    for character in ["alpha", "beta"] {
        events.push(crate::common::stats::character(character, "Synthetic", 0));
        let mut run = run(
            &format!("synthetic-run-{character}"),
            "synthetic-session",
            "Synthetic Dungeon",
            0,
        );
        run.character_id = Some(character.into());
        events.push(EngineEvent::RunStarted(run));
    }
    events.extend(fights.into_iter().map(EngineEvent::FightUpdated));
    stream.apply(db, events);
}

#[test]
fn boss_history_averages_local_dps_for_character_in_range() {
    let (_dir, path, mut db) = database();
    let mut first = fight("first", "alpha", 10, Some(1000.0));
    let mut other = first.players[0].clone();
    other.is_local = false;
    other.player_id = "SyntheticOther".into();
    other.object_id += 1;
    other.dps = Some(4000.0);
    first.players.push(other.clone());
    other.player_id = "SyntheticThird".into();
    other.object_id += 1;
    other.dps = Some(2000.0);
    first.players.push(other.clone());
    let mut second = fight("best", "alpha", 20, Some(3000.0));
    second.players[0].rank = Some(4);
    other.dps = Some(6000.0);
    second.players.push(other);
    seed(
        &mut db,
        vec![
            first,
            second,
            fight("before", "alpha", 9, Some(9000.0)),
            fight("at-end", "alpha", 30, Some(9000.0)),
            fight("other-character", "beta", 15, Some(8000.0)),
        ],
    );
    let result = open_reader(&path)
        .unwrap()
        .boss_history(BossHistoryRequest {
            range: Some(TimeRange {
                start: Timestamp(10),
                end: Timestamp(30),
            }),
            character_id: Some("alpha".into()),
            ..all()
        })
        .unwrap();
    assert!(!result.truncated);
    assert_eq!(result.rows.len(), 1);
    let row = &result.rows[0];
    assert_eq!(row.target_type, 42);
    assert_eq!(row.target_name, None);
    assert_eq!(row.fights, 2);
    assert_eq!(row.avg_dps, Some(2000.0));
    assert_eq!(row.best_dps, Some(3000.0));
    assert_eq!(row.best_fight_id.as_deref(), Some("best"));
    assert_eq!(row.avg_rank, Some(3.0));
    assert_eq!(row.avg_party_dps, Some(4500.0));
    assert_eq!((row.kills, row.median_kill_micros), (0, None));
}

#[test]
fn boss_history_excludes_kill_before_start_without_losing_good_rows() {
    let (_dir, path, mut db) = database();
    let fights = [9, 70, 90, 110]
        .into_iter()
        .enumerate()
        .map(|(i, killed)| {
            let mut fight = fight(&format!("kill-{i}"), "alpha", 10, Some(1000.0));
            fight.kill = Some(KillView {
                at: Timestamp(killed),
                evidence: KillEvidence::HpZeroRemoved,
                provisional: false,
            });
            fight
        })
        .collect();
    seed(&mut db, fights);
    let result = open_reader(&path).unwrap().boss_history(all()).unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].fights, 4);
    assert_eq!(result.rows[0].avg_dps, Some(1000.0));
    assert_eq!(result.rows[0].kills, 3);
    assert_eq!(result.rows[0].median_kill_micros, Some(80));
}

#[test]
fn boss_history_median_kill_ignores_character_and_provisional_kills() {
    let (_dir, path, mut db) = database();
    let fights = [
        ("alpha", 60, false),
        ("alpha", 80, false),
        ("alpha", 100, false),
        ("alpha", 10, true),
        ("beta", 70, false),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (character, seconds, provisional))| {
        let mut fight = fight(&format!("kill-{i}"), character, 10, Some(1000.0));
        fight.kill = Some(KillView {
            at: Timestamp(10 + seconds * SECOND),
            evidence: KillEvidence::HpZeroRemoved,
            provisional,
        });
        fight
    })
    .collect();
    seed(&mut db, fights);
    let result = open_reader(&path)
        .unwrap()
        .boss_history(BossHistoryRequest {
            range: Some(TimeRange {
                start: Timestamp(10),
                end: Timestamp(11),
            }),
            character_id: Some("alpha".into()),
            ..all()
        })
        .unwrap();
    assert_eq!(result.rows[0].fights, 4);
    assert_eq!(result.rows[0].kills, 4);
    assert_eq!(result.rows[0].median_kill_micros, Some(75_000_000));
}

fn visit(
    db: &Connection,
    id: &str,
    session: &str,
    character: &str,
    started: i64,
    ended: Option<i64>,
) {
    db.execute("INSERT INTO map_visits(id,stream_id,session_id,character_id,map,kind,started,ended,source,json)
        VALUES(?1,?1,?2,?3,'Synthetic Realm','realm',?4,?5,'live','{}')",
        params![id, session, character, started, ended]).unwrap();
}

#[test]
fn boss_history_attributes_realm_fights_by_visit_interval() {
    let (_dir, path, mut db) = database();
    let mut realm = fight("realm", "alpha", 20, Some(1200.0));
    realm.run_id = None;
    realm.boss_type = None; // The target type comes from JSON, including realm targets.
    let mut after = fight("after", "alpha", 30, Some(9000.0));
    after.run_id = None;
    seed(
        &mut db,
        vec![realm, after, fight("run-wins", "beta", 20, Some(8000.0))],
    );
    visit(&db, "earlier", "synthetic-session", "beta", 0, None);
    visit(&db, "matching", "synthetic-session", "alpha", 20, Some(30));
    let mut stream = Stream::new("synthetic-other-session");
    stream.apply(&mut db, vec![session("other-session", 0, 0, None)]);
    visit(&db, "wrong-session", "other-session", "beta", 20, None);
    let reader = open_reader(&path).unwrap();
    let mine = reader
        .boss_history(BossHistoryRequest {
            character_id: Some("alpha".into()),
            ..all()
        })
        .unwrap();
    assert_eq!(mine.rows[0].fights, 1);
    assert_eq!(mine.rows[0].avg_dps, Some(1200.0));
    let other = reader
        .boss_history(BossHistoryRequest {
            character_id: Some("beta".into()),
            ..all()
        })
        .unwrap();
    assert_eq!(other.rows[0].fights, 2);
}

#[test]
fn boss_history_excludes_fights_without_local_dps() {
    let (_dir, path, mut db) = database();
    seed(
        &mut db,
        vec![
            fight("known", "alpha", 10, Some(0.0)),
            fight("unknown", "alpha", 20, None),
        ],
    );
    let result = open_reader(&path).unwrap().boss_history(all()).unwrap();
    assert_eq!(result.rows[0].fights, 1);
    assert_eq!(result.rows[0].avg_dps, Some(0.0));
}

#[test]
fn boss_history_excludes_unended_fights() {
    let (_dir, path, mut db) = database();
    let mut open = fight("open", "alpha", 20, Some(9000.0));
    open.ended = None;
    seed(
        &mut db,
        vec![fight("ended", "alpha", 10, Some(1000.0)), open],
    );
    let result = open_reader(&path).unwrap().boss_history(all()).unwrap();
    assert_eq!(result.rows[0].fights, 1);
    assert_eq!(result.rows[0].best_dps, Some(1000.0));
}

#[test]
fn boss_history_filters_target_types() {
    let (_dir, path, mut db) = database();
    let mut other = fight("other", "alpha", 20, Some(2000.0));
    other.target_type = Some(43);
    seed(
        &mut db,
        vec![fight("first", "alpha", 10, Some(1000.0)), other],
    );
    let reader = open_reader(&path).unwrap();
    let result = reader
        .boss_history(BossHistoryRequest {
            target_types: Some(vec![43]),
            ..all()
        })
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].target_type, 43);
    assert!(reader
        .boss_history(BossHistoryRequest {
            target_types: Some(vec![]),
            ..all()
        })
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn boss_history_bounds_rows_in_fight_count_then_target_order() {
    let (_dir, path, mut db) = database();
    let mut fights: Vec<_> = (0..201)
        .map(|target| {
            let mut fight = fight(&format!("fight-{target}"), "alpha", 10, Some(1000.0));
            fight.target_type = Some(target);
            fight
        })
        .collect();
    let mut frequent = fights[200].clone();
    frequent.id = "frequent".into();
    fights.push(frequent);
    seed(&mut db, fights);
    let result = open_reader(&path).unwrap().boss_history(all()).unwrap();
    assert!(result.truncated);
    assert_eq!(result.rows.len(), 200);
    assert_eq!(result.rows[0].target_type, 200);
    assert_eq!(result.rows[1].target_type, 0);
    assert_eq!(result.rows[199].target_type, 198);
}

#[test]
fn boss_history_preserves_kills_without_local_dps_and_floors_even_median() {
    let (_dir, path, mut db) = database();
    let fights = [60_000_000, 60_000_001]
        .into_iter()
        .enumerate()
        .map(|(i, duration)| {
            let mut fight = fight(&format!("kill-{i}"), "beta", 10, None);
            fight.kill = Some(KillView {
                at: Timestamp(10 + duration),
                evidence: KillEvidence::HpZeroRemoved,
                provisional: false,
            });
            fight
        })
        .collect();
    seed(&mut db, fights);
    let result = open_reader(&path)
        .unwrap()
        .boss_history(BossHistoryRequest {
            character_id: Some("alpha".into()),
            ..all()
        })
        .unwrap();
    let row = &result.rows[0];
    assert_eq!(row.fights, 0);
    assert_eq!(
        (
            row.avg_dps,
            row.best_dps,
            &row.best_fight_id,
            row.avg_rank,
            row.avg_party_dps
        ),
        (None, None, &None, None, None)
    );
    assert_eq!((row.kills, row.median_kill_micros), (2, Some(60_000_000)));
}

#[test]
fn boss_history_skips_malformed_fight_json_and_unknown_targets() {
    let (_dir, path, mut db) = database();
    let mut unknown = fight("unknown", "alpha", 10, Some(9000.0));
    unknown.target_type = None;
    seed(
        &mut db,
        vec![
            fight("known", "alpha", 10, Some(1000.0)),
            fight("broken", "alpha", 10, Some(9000.0)),
            unknown,
        ],
    );
    db.execute("UPDATE fights SET json='broken' WHERE id='broken'", [])
        .unwrap();
    let result = open_reader(&path).unwrap().boss_history(all()).unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].fights, 1);
}

#[test]
fn boss_history_kill_range_is_half_open_and_odd_median_is_the_middle() {
    let (_dir, path, mut db) = database();
    let fights = [(9, 10), (10, 100), (11, 60), (12, 80), (13, 20)]
        .into_iter()
        .enumerate()
        .map(|(i, (started, seconds))| {
            let mut fight = fight(&format!("kill-{i}"), "alpha", started, None);
            fight.kill = Some(KillView {
                at: Timestamp(started + seconds * SECOND),
                evidence: KillEvidence::HpZeroRemoved,
                provisional: false,
            });
            fight
        })
        .collect();
    seed(&mut db, fights);
    let result = open_reader(&path)
        .unwrap()
        .boss_history(BossHistoryRequest {
            range: Some(TimeRange {
                start: Timestamp(10),
                end: Timestamp(13),
            }),
            ..all()
        })
        .unwrap();
    assert_eq!(result.rows[0].kills, 3);
    assert_eq!(result.rows[0].median_kill_micros, Some(80_000_000));
}

#[test]
fn boss_history_rejects_invalid_filters_and_unknown_request_fields() {
    let (_dir, path, _db) = database();
    let reader = open_reader(&path).unwrap();
    for request in [
        BossHistoryRequest {
            range: Some(TimeRange {
                start: Timestamp(2),
                end: Timestamp(1),
            }),
            ..all()
        },
        BossHistoryRequest {
            target_types: Some((0..65).collect()),
            ..all()
        },
        BossHistoryRequest {
            character_id: Some("synthetic\u{7}".into()),
            ..all()
        },
    ] {
        assert!(reader.boss_history(request).is_err());
    }
    assert!(
        serde_json::from_value::<BossHistoryRequest>(serde_json::json!({
            "range": null, "character_id": null, "target_types": null, "extra": true
        }))
        .is_err()
    );
}

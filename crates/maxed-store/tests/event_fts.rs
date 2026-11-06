use maxed_store::{migrate, open_reader};
use maxed_types::*;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

const LEGACY: [&str; 11] = [
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
];

fn hits(db: &Connection, term: &str) -> i64 {
    db.query_row(
        "SELECT count(*) FROM event_fts WHERE event_fts MATCH ?1",
        [term],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn existing_v11_names_backfill_and_triggers_replace_and_remove_terms() {
    let mut db = Connection::open_in_memory().unwrap();
    for (i, script) in LEGACY.iter().enumerate() {
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
    db.pragma_update(None, "user_version", 11).unwrap();
    db.execute("INSERT INTO play_sessions VALUES('s',0,0,NULL,'{}')", [])
        .unwrap();
    let insert = "INSERT INTO instance_events VALUES('event','s',NULL,1,'player_left',?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json";
    db.execute(insert, [r#"{"kind":{"player_name":"Vexlin"}}"#])
        .unwrap();
    db.execute(
        "INSERT INTO instance_events VALUES('unreadable','s',NULL,1,'player_left','unreadable')",
        [],
    )
    .unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT json FROM instance_events WHERE id='unreadable'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "unreadable"
    );
    assert_eq!(hits(&db, "Vex*"), 1);
    migrate(&mut db).unwrap();
    assert_eq!(hits(&db, "Vex*"), 1);
    db.execute(insert, [r#"{"kind":{"player_name":"Quenby"}}"#])
        .unwrap();
    assert_eq!(hits(&db, "Vex*"), 0);
    assert_eq!(hits(&db, "Quen*"), 1);
    db.execute(
        "UPDATE instance_events SET rowid=rowid+100,json=?1 WHERE id='event'",
        [r#"{"kind":{"opener":"Morrow"}}"#],
    )
    .unwrap();
    assert_eq!(hits(&db, "Quen*"), 0);
    assert_eq!(hits(&db, "Morr*"), 1);
    db.execute("DELETE FROM instance_events WHERE at<2", [])
        .unwrap();
    assert_eq!(hits(&db, "Morr*"), 0);
    db.execute(insert, [r#"{"kind":{"player_name":"Kestrel"}}"#])
        .unwrap();
    assert_eq!(hits(&db, "Kest*"), 1);
    db.execute(
        "UPDATE instance_events SET json='unreadable' WHERE id='event'",
        [],
    )
    .unwrap();
    assert_eq!(hits(&db, "Kest*"), 0);
    db.execute("DELETE FROM instance_events", []).unwrap();
    assert_eq!(hits(&db, "Kest*"), 0);
    db.execute(
        "INSERT INTO event_fts(event_fts) VALUES('integrity-check')",
        [],
    )
    .unwrap();
}

#[test]
fn every_name_field_uses_safe_prefix_terms() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    db.execute("INSERT INTO play_sessions VALUES('s',0,0,NULL,'{}')", [])
        .unwrap();
    let reader = open_reader(&path).unwrap();
    for field in ["player_name", "opener", "killed_by", "killer", "boss_key"] {
        let name = "Vexlin NEAR OR Quenby".to_owned();
        let (tag, kind) = match field {
            "player_name" => (
                "player_left",
                InstanceEventKind::PlayerLeft {
                    object_id: 1,
                    player_name: Some(name),
                    class_id: None,
                },
            ),
            "opener" => (
                "portal_opened",
                InstanceEventKind::PortalOpened {
                    opener: Some(name),
                    opener_is_local: None,
                    dungeon: None,
                    portal_type: None,
                },
            ),
            "killed_by" => (
                "player_death",
                InstanceEventKind::PlayerDeath {
                    player_id: None,
                    player_name: "Kestrel".into(),
                    killed_by: Some(name),
                    level: None,
                    grave_type: None,
                    object_id: None,
                    class_id: None,
                    is_local: false,
                },
            ),
            "killer" => (
                "realm_event_killed",
                InstanceEventKind::RealmEventKilled {
                    boss_key: "Synthetic sentinel".into(),
                    killer: Some(name),
                    object_type: None,
                },
            ),
            "boss_key" => (
                "realm_event_killed",
                InstanceEventKind::RealmEventKilled {
                    boss_key: name,
                    killer: None,
                    object_type: None,
                },
            ),
            _ => unreachable!(),
        };
        let value = InstanceEventView {
            id: "event".into(),
            session_id: "s".into(),
            run_id: None,
            map: None,
            at: Timestamp(1),
            provisional: true,
            kind,
        };
        db.execute("INSERT INTO instance_events VALUES('event','s',NULL,1,?1,?2) ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,json=excluded.json", params![tag, serde_json::to_string(&value).unwrap()]).unwrap();
        for (query, count) in [
            ("Vex", 1),
            ("xlin", 0),
            ("\"Vex*()", 1),
            ("NEAR OR", 1),
            ("Vex Quen", 1),
            ("*()\"", 1),
            ("   ", 1),
        ] {
            let page = reader
                .list_events(EventFilter {
                    kinds: vec![],
                    run_id: None,
                    dungeon: None,
                    cursor: None,
                    limit: 10,
                    range: None,
                    query: query.into(),
                    opener_local: None,
                })
                .unwrap();
            assert_eq!(page.rows.len(), count, "{field}: {query}");
        }
    }
}

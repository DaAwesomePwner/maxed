mod common;
use common::stats::*;
use maxed_types::*;
use rusqlite::params;
use std::time::Instant;

const HOUR: i64 = 3_600_000_000;
const STEP: i64 = 120_000_000; // two minutes per visit

/// 2,000 sessions × 25 visits = 50,000 visits, 500 samples each = 1,000,000 samples,
/// 10 dungeon runs each = 20,000 runs, across 10 synthetic characters.
fn seed(db: &mut rusqlite::Connection) {
    let tx = db.transaction().unwrap();
    for c in 0..10 {
        let view = character_view(&format!("c{c}"), &format!("Synth{c}"), 0);
        tx.execute(
            "INSERT INTO characters(id,observed_at,json) VALUES(?1,0,?2)",
            params![view.id, serde_json::to_string(&view).unwrap()],
        )
        .unwrap();
    }
    let kinds = ["nexus", "realm", "dungeon", "other_hub", "dungeon"];
    for s in 0..2_000i64 {
        let id = format!("s{s}");
        let started = s * HOUR;
        let ended = started + 25 * STEP;
        let character = format!("c{}", s % 10);
        let session = PlaySessionView {
            id: id.clone(),
            started: Timestamp(started as u64),
            last_traffic: Timestamp(ended as u64),
            ended: Some(Timestamp(ended as u64)),
            issues: vec![],
        };
        tx.execute(
            "INSERT INTO play_sessions VALUES(?1,?2,?3,?3,?4)",
            params![id, started, ended, serde_json::to_string(&session).unwrap()],
        )
        .unwrap();
        for v in 0..25i64 {
            let at = started + v * STEP;
            let kind = kinds[(v % 5) as usize];
            let run_id = (kind == "dungeon").then(|| format!("r{s}-{v}"));
            let map = if kind == "dungeon" {
                format!("Synthetic {}", v % 7)
            } else {
                kind.to_owned()
            };
            if let Some(run_id) = &run_id {
                let mut r = run(run_id, &id, &map, at as u64);
                r.character_id = Some(character.clone());
                r.ended = Some(Timestamp((at + STEP) as u64));
                r.outcome = Some(if v % 3 == 0 {
                    Outcome::Died
                } else {
                    Outcome::Cleared
                });
                tx.execute(
                    "INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,died_at,outcome,complete_capture,entry_character_json,json)
                     VALUES(?1,?2,?3,?4,?5,?6,NULL,NULL,?7,1,NULL,?8)",
                    params![run_id, id, character, map, at, at + STEP,
                        if v % 3 == 0 { "died" } else { "cleared" }, serde_json::to_string(&r).unwrap()],
                ).unwrap();
            }
            tx.execute(
                "INSERT INTO map_visits VALUES(?1,'bench',?2,?3,?4,?5,?6,NULL,?7,?8,'live','{}')",
                params![
                    format!("v{s}-{v}"),
                    id,
                    character,
                    run_id,
                    map,
                    kind,
                    at,
                    at + STEP
                ],
            )
            .unwrap();
        }
        for f in 0..500i64 {
            tx.execute(
                "INSERT INTO fame_samples VALUES('bench',?1,?2,?3,?4,?5)",
                params![
                    s * 500 + f + 1,
                    character,
                    started + f * 6_000_000,
                    s * 1000 + f,
                    id
                ],
            )
            .unwrap();
        }
    }
    tx.commit().unwrap();
}

#[test]
#[ignore = "benchmark: cargo test --release --locked -p maxed-store --test stats_performance -- --ignored --nocapture"]
fn fifty_thousand_visits_one_million_samples() {
    let (_dir, path, mut db) = database();
    seed(&mut db);
    drop(db);
    let reader = maxed_store::open_reader(&path).unwrap();
    let all = StatsFilter {
        range: None,
        character_id: None,
    };
    let month = StatsFilter {
        range: Some(TimeRange {
            start: Timestamp((2_000 - 720) * HOUR as u64),
            end: Timestamp(2_000 * HOUR as u64),
        }),
        character_id: None,
    };
    let one = StatsFilter {
        range: None,
        character_id: Some("c3".into()),
    };
    let time = |label: &str, read: &dyn Fn()| {
        let t = Instant::now();
        read();
        println!("{label}: {:?}", t.elapsed());
    };
    time("maps all time", &|| {
        reader.stats_maps(all.clone()).unwrap();
    });
    time("maps 30 days", &|| {
        reader.stats_maps(month.clone()).unwrap();
    });
    time("maps one character", &|| {
        reader.stats_maps(one.clone()).unwrap();
    });
    time("sessions first page", &|| {
        reader
            .stats_sessions(StatsSessionFilter {
                range: None,
                character_id: None,
                include_empty: false,
                cursor: None,
                limit: 50,
            })
            .unwrap();
    });
    time("session detail", &|| {
        reader
            .stats_session(StatsSessionRequest {
                id: "s1999".into(),
                character_id: None,
            })
            .unwrap();
    });
    time("dungeons all time", &|| {
        reader.stats_dungeons(all.clone()).unwrap();
    });
    time("sessions export", &|| {
        reader.stats_sessions_export(all.clone(), true).unwrap();
    });
}

#[test]
#[ignore = "benchmark: cargo test --release --locked -p maxed-store --test stats_performance -- --ignored --nocapture"]
fn migration_010_with_twenty_thousand_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v9");
    let mut db = v9_database(&path);
    let tx = db.transaction().unwrap();
    tx.execute("INSERT INTO play_sessions VALUES('s',0,0,0,'{}')", [])
        .unwrap();
    for i in 0..20_000i64 {
        tx.execute(
            "INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,died_at,outcome,complete_capture,entry_character_json,json)
             VALUES(?1,'s',NULL,'Synthetic Halls',?2,?3,NULL,NULL,'left',1,NULL,'{}')",
            params![format!("r{i}"), i * 1000, i * 1000 + 500],
        ).unwrap();
    }
    tx.commit().unwrap();
    let t = Instant::now();
    maxed_store::migrate(&mut db).unwrap();
    println!("migration 010: {:?}", t.elapsed());
}

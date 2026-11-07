//! Synthetic event-history workloads reuse run DTO templates without real captures.
use maxed_store::{apply_batch, migrate, open_reader, StoreReader};
use maxed_types::*;
use rusqlite::{params, Connection};
use std::time::{Duration, Instant};

pub const BATCH: usize = 100;
pub struct Fixture {
    pub _dir: tempfile::TempDir,
    pub path: std::path::PathBuf,
    pub db: Connection,
    pub sequence: u64,
}
impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pd");
        let mut db = Connection::open(&path).unwrap();
        let started = Instant::now();
        migrate(&mut db).unwrap();
        eprintln!("PD fresh migration: {:?}", started.elapsed());
        db.pragma_update(None, "journal_mode", "WAL").unwrap();
        db.pragma_update(None, "synchronous", "FULL").unwrap();
        let mut f = Self {
            _dir: dir,
            path,
            db,
            sequence: 0,
        };
        f.send(vec![EngineEvent::SessionUpdated(
            demo::demo_snapshot().session.unwrap(),
        )]);
        f
    }
    pub fn send(&mut self, events: Vec<EngineEvent>) -> Duration {
        let events: Vec<_> = events
            .into_iter()
            .map(|event| {
                self.sequence += 1;
                EventEnvelope {
                    fight_chart_omitted: false,
                    stream_id: "pd-volume".into(),
                    sequence: Sequence(Timestamp(self.sequence)),
                    at: Timestamp(5_000_000),
                    event,
                }
            })
            .collect();
        let start = Instant::now();
        apply_batch(&mut self.db, &events).unwrap();
        start.elapsed()
    }
    pub fn reader(&self) -> StoreReader {
        let start = Instant::now();
        let reader = open_reader(&self.path).unwrap();
        eprintln!("PD reader open/config/schema: {:?}", start.elapsed());
        reader
    }
    pub fn runs(&mut self) {
        let mut run = demo::demo_run_story().run;
        self.db
            .execute(
                "INSERT INTO accounts VALUES(?1)",
                [crate::common::account_scope_fixture::ACCOUNT],
            )
            .unwrap();
        self.db.execute("INSERT INTO characters(id,observed_at,json,account_id) VALUES('synthetic-d-owner',0,'{}',?1)", [crate::common::account_scope_fixture::ACCOUNT]).unwrap();
        run.character_id = Some("synthetic-d-owner".into());
        run.complete_capture = true;
        run.issues.clear();
        run.died_at = None;
        run.started = Timestamp(1_000_000);
        run.ended = Some(Timestamp(6_000_000));
        run.completed_at = Some(Timestamp(5_000_000));
        run.outcome = Some(Outcome::Cleared);
        run.evidence = vec![EvidenceAt {
            at: Timestamp(5_000_000),
            evidence: Evidence::Victory,
        }];
        self.send(vec![EngineEvent::RunEnded(run)]);
        self.db
            .execute_batch(
                "CREATE TEMP TABLE numbers(i INTEGER PRIMARY KEY);
            WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<9999)
            INSERT INTO numbers SELECT i FROM n;",
            )
            .unwrap();
        self.db.execute("INSERT INTO runs SELECT printf('pd-run-%05d',i),session_id,character_id,printf('Dungeon-%02d',i%60),started,ended,completed_at,NULL,outcome,complete_capture,NULL,
            json_set(json,'$.id',printf('pd-run-%05d',i),'$.dungeon',printf('Dungeon-%02d',i%60))
            FROM numbers CROSS JOIN runs WHERE id='demo-run' AND i>0",[]).unwrap();
        self.db.execute("INSERT INTO run_evidence SELECT id,0,completed_at,'victory',json_extract(json,'$.evidence[0]') FROM runs WHERE id<>'demo-run'",[]).unwrap();
        self.db.execute("UPDATE runs SET dungeon='Dungeon-00',json=json_set(json,'$.dungeon','Dungeon-00') WHERE id='demo-run'",[]).unwrap();
        self.db.execute_batch("DROP TABLE numbers").unwrap();
        assert_eq!(
            self.db
                .query_row("SELECT count(*) FROM runs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            10_000
        );
        assert_eq!(
            self.db
                .query_row("SELECT count(DISTINCT dungeon) FROM runs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            60
        );
    }
    pub fn add_bags(&mut self, count: usize) {
        let template = demo::demo_run_story().drops.remove(0);
        for chunk in (0..count).collect::<Vec<_>>().chunks(BATCH) {
            self.send(
                chunk
                    .iter()
                    .map(|i| {
                        let mut bag = template.clone();
                        bag.id = format!("pd-bag-{i:05}");
                        bag.bag_type = if i % 2 == 0 { 1287 } else { 1288 };
                        bag.bag_rank = Some(if i % 2 == 0 { 10 } else { 20 });
                        bag.capture_scope = Some(CaptureScope::RealmShark);
                        EngineEvent::BagDropped(bag)
                    })
                    .collect(),
            );
        }
    }
}
pub fn instance(id: usize, session: &str, run: Option<String>) -> EngineEvent {
    EngineEvent::InstanceEvent(InstanceEventView {
        id: format!("pd-event-{id:07}"),
        session_id: session.into(),
        run_id: run,
        map: Some("Synthetic map".into()),
        at: Timestamp(4_000_000),
        kind: InstanceEventKind::PlayerLeft {
            object_id: (id % 1000) as i32,
            player_name: Some("SynthAlpha".into()),
            class_id: None,
        },
        provisional: true,
    })
}
pub fn warm<T: serde::Serialize>(label: &str, mut read: impl FnMut() -> T) -> Duration {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let bytes = serde_json::to_vec(&read()).unwrap().len();
    let mut samples = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let value = read();
        samples.push(start.elapsed());
        assert_eq!(serde_json::to_vec(&value).unwrap().len(), bytes);
    }
    samples.sort();
    eprintln!(
        "{label}: n=30 median={:?} p95={:?} max={:?} bytes={bytes}",
        samples[14], samples[28], samples[29]
    );
    samples[28]
}
/// Supply a fresh run-history baseline on the same host, in microseconds.
/// No historical shared-host timing is silently used as today's baseline.
pub fn baseline(key: &str) -> Duration {
    let micros = std::env::var(key)
        .unwrap_or_else(|_| {
            panic!("Set {key} to the same-machine P4 p95 in microseconds before this gate")
        })
        .parse::<f64>()
        .expect("positive finite P4 microseconds");
    assert!(micros.is_finite() && micros > 0.0);
    Duration::from_secs_f64(micros / 1_000_000.)
}
pub fn volume(f: &mut Fixture, pd: bool) -> Duration {
    // Matched baseline observations: one ordinary captured bag at each identical
    // session/run/time position, with unknown contents (no invented item work).
    // Both streams contain 1.2M events, 100 per transaction, and 600k timeline rows.
    let mut bag = demo::demo_run_story().drops.remove(0);
    bag.bag_type = 1287;
    bag.bag_rank = None;
    bag.capture_scope = None;
    bag.contents_known = false;
    bag.contents.clear();
    bag.items.clear();
    let observation = |id, session: &str, run: Option<String>| {
        if pd {
            instance(id, session, run)
        } else {
            let mut drop = bag.clone();
            drop.id = format!("pd-event-{id:07}");
            drop.session_id = session.into();
            drop.run_id = run;
            drop.at = Timestamp(4_000_000);
            EngineEvent::BagDropped(drop)
        }
    };
    let runs: Vec<String> =
        f.db.prepare("SELECT id FROM runs ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
    let mut samples = Vec::new();
    let mut batch = Vec::new();
    let mut id = 0;
    for run in &runs {
        for _ in 0..60 {
            batch.push(observation(id, "demo-session", Some(run.clone())));
            id += 1;
            if batch.len() == BATCH {
                samples.push(f.send(std::mem::take(&mut batch)));
                if samples.len().is_multiple_of(1000) {
                    eprintln!(
                        "{} projection progress: {} events",
                        if pd { "PD" } else { "P4" },
                        samples.len() * BATCH
                    );
                }
            }
        }
    }
    for session in 0..2000 {
        let sid = format!("realm-session-{session:04}");
        let mut s = demo::demo_snapshot().session.unwrap();
        s.id = sid.clone();
        f.send(vec![EngineEvent::SessionUpdated(s)]);
        for _ in 0..300 {
            batch.push(observation(id, &sid, None));
            id += 1;
            if batch.len() == BATCH {
                samples.push(f.send(std::mem::take(&mut batch)));
                if samples.len().is_multiple_of(1000) {
                    eprintln!(
                        "{} projection progress: {} events",
                        if pd { "PD" } else { "P4" },
                        samples.len() * BATCH
                    );
                }
            }
        }
    }
    assert!(batch.is_empty());
    assert_eq!(id, 1_200_000);
    assert_eq!(samples.len(), 12_000);
    let table = if pd { "instance_events" } else { "drops" };
    assert_eq!(
        f.db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1_200_000
    );
    assert_eq!(
        f.db.query_row(
            &format!(
                "SELECT count(*) FROM {}",
                if pd { "drops" } else { "instance_events" }
            ),
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        f.db.query_row(
            "SELECT max(n) FROM (SELECT count(*) n FROM timeline GROUP BY run_id)",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        62
    );
    for (scope, n) in [("run_id IS NOT NULL", 600_000), ("run_id IS NULL", 600_000)] {
        assert_eq!(
            f.db.query_row(
                &format!("SELECT count(*) FROM {table} WHERE {scope}"),
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            n
        );
    }
    samples.sort();
    let p95 = samples[(samples.len() * 95).div_ceil(100) - 1];
    eprintln!(
        "{} projection batches: size={BATCH} n={} median={:?} p95={p95:?} max={:?}",
        if pd {
            "PD instance"
        } else {
            "P4 ordinary-bag baseline"
        },
        samples.len(),
        samples[samples.len() / 2 - 1],
        samples[samples.len() - 1]
    );
    // The scoped index must seek even at 1.2 million retained events.
    let plan =
        f.db.prepare(&format!(
            "EXPLAIN QUERY PLAN SELECT json FROM {table} WHERE run_id=?1 ORDER BY at,id"
        ))
        .unwrap()
        .query_map(params!["demo-run"], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH")
            && plan.contains(if pd {
                "instance_events_run"
            } else {
                "drops_run"
            }),
        "{plan}"
    );
    p95
}

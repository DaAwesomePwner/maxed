//! Invented alert scale histories. No recordings, wall clock, or owner data.
//! Archive fights are compact valid DTOs, not a full replay throughput fixture.
#![allow(dead_code)]
use super::{apply_batch, migrate, open_reader, StoreReader};
use maxed_types::*;
use rusqlite::{params, Connection};
use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant},
};

pub const BASE: u64 = 1_790_830_800_000_000;
pub const LITERAL: &str = "literal %_\\ é";
pub const ORDINARY: usize = 100_000;
pub const STRESS: usize = 1_000_000;
pub fn bytes(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).unwrap().len()
}
pub fn count(db: &Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |r| r.get(0)).unwrap()
}
pub fn envelope(n: u64, event: EngineEvent) -> EventEnvelope {
    EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "alerts-workload".into(),
        sequence: Sequence(Timestamp(n)),
        at: Timestamp(BASE + n),
        event,
    }
}
pub fn compact_fight() -> FightView {
    let mut f = demo::demo_snapshot().fights.remove(0);
    f.id = "template".into();
    f.run_id = Some("r00000".into());
    f.started = Timestamp(BASE);
    f.ended = Some(Timestamp(BASE + 2_000_000));
    f.first_hit = Some(Timestamp(BASE));
    f.last_hit = f.ended;
    f.end_reason = Some(FightEnd::MapChanged);
    f.complete_capture = true;
    f.issues.clear();
    f.buckets.clear();
    f.players.clear();
    f.boss_type = None;
    f.total_damage = 0.;
    f.unattributed_damage = 0.;
    f
}
pub fn eligible_fight() -> FightView {
    let mut f = compact_fight();
    let mut p = demo::demo_snapshot().fights.remove(0).players.remove(0);
    p.player_id = "c000".into();
    p.is_local = true;
    p.damage = 200.;
    p.dps = Some(100.);
    p.sources.clear();
    f.players.push(p);
    f.boss_type = Some(0);
    f.total_damage = 200.;
    f
}
pub struct Fixture {
    // Fields drop in declaration order: close SQLite before removing its Windows directory.
    pub db: Connection,
    pub path: PathBuf,
    pub alerts: usize,
    pub _dir: tempfile::TempDir,
}

pub fn query_fixture(n: usize) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("query.sqlite");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    db.execute_batch("CREATE TEMP TABLE n(i INTEGER PRIMARY KEY); WITH RECURSIVE s(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM s WHERE i<9999) INSERT INTO n SELECT i FROM s;").unwrap();
    let mut f = Fixture {
        _dir: dir,
        path,
        db,
        alerts: 0,
    };
    f.add_alerts(n);
    f
}

fn perf_tempdir() -> tempfile::TempDir {
    // Keep performance databases under Cargo's build tree, where the benchmark
    // machine's scanning exclusions apply, rather than its system temp directory.
    // This module also supplies non-performance helpers to library unit tests,
    // where Cargo does not define CARGO_TARGET_TMPDIR.
    let Some(root) = option_env!("CARGO_TARGET_TMPDIR") else {
        panic!("performance fixtures must run as Cargo integration tests");
    };
    std::fs::create_dir_all(root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

/// SQL bulk expansion keeps indexed columns and serialized DTO fields identical.
/// No synthetic event receipt is fabricated for any of the four million fights.
pub fn build(alerts: usize) -> Fixture {
    let dir = perf_tempdir();
    let path = dir.path().join("alerts.sqlite");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .unwrap();
    let mut session = demo::demo_snapshot().session.unwrap();
    session.started = Timestamp(BASE);
    session.last_traffic = Timestamp(BASE + 10_000_000);
    session.ended = Some(session.last_traffic);
    apply_batch(
        &mut db,
        &[envelope(1, EngineEvent::SessionUpdated(session))],
    )
    .unwrap();
    let mut c = demo::demo_snapshot().character.unwrap();
    c.identity_verified = true;
    let mut r = demo::demo_snapshot().run.unwrap();
    r.started = Timestamp(BASE);
    r.ended = Some(Timestamp(BASE + 2_000_000));
    r.completed_at = None;
    r.died_at = None;
    r.outcome = Some(Outcome::Left);
    r.complete_capture = true;
    r.evidence.clear();
    r.issues.clear();
    let mut d = demo::demo_run_story().drops.remove(0);
    d.at = Timestamp(BASE);
    d.contents.clear();
    d.contents_known = false;
    d.bag_rank = None;
    let tx = db.transaction().unwrap();
    tx.execute_batch("CREATE TEMP TABLE n(i INTEGER PRIMARY KEY); WITH RECURSIVE s(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM s WHERE i<9999) INSERT INTO n SELECT i FROM s;").unwrap();
    tx.execute("INSERT INTO characters SELECT printf('c%03d',i),?1,json_set(?2,'$.id',printf('c%03d',i),'$.observed_at',CAST(?1 AS TEXT)) FROM n WHERE i<100", params![BASE as i64,serde_json::to_string(&c).unwrap()]).unwrap();
    tx.execute("INSERT INTO runs SELECT printf('r%05d',i),?1,printf('c%03d',i%100),?2,?3,?3+2000000,NULL,NULL,'left',1,NULL,json_set(?4,'$.id',printf('r%05d',i),'$.character_id',printf('c%03d',i%100)) FROM n",params![r.session_id,r.dungeon,BASE as i64,serde_json::to_string(&r).unwrap()]).unwrap();
    // 20 eligible bosses in every 400 fights; 100 characters x 100 bosses,
    // exactly 20 observations of every key. The remaining 3.8M are empty non-boss DTOs.
    let started = Instant::now();
    tx.execute("INSERT INTO fights SELECT printf('f%07d',a.i*400+b.i),printf('r%05d',a.i),?1,?2,?2+2000000,CASE WHEN b.i<20 THEN (a.i/100%5)*20+b.i END,CASE WHEN b.i<20 THEN 100.0 END,1,json_set(CASE WHEN b.i<20 THEN ?4 ELSE ?3 END,'$.id',printf('f%07d',a.i*400+b.i),'$.run_id',printf('r%05d',a.i),'$.boss_type',CASE WHEN b.i<20 THEN (a.i/100%5)*20+b.i END,'$.players',json(CASE WHEN b.i<20 THEN json_set(json_extract(?4,'$.players'),'$[0].player_id',printf('c%03d',a.i%100)) ELSE '[]' END)) FROM n a CROSS JOIN n b WHERE b.i<400",params![r.session_id,BASE as i64,serde_json::to_string(&compact_fight()).unwrap(),serde_json::to_string(&eligible_fight()).unwrap()]).unwrap();
    eprintln!("fixture bulk fights: {:?}", started.elapsed());
    tx.execute_batch("INSERT INTO fight_players SELECT id,json_extract(json,'$.players[0].player_id'),json_extract(json,'$.players[0].object_id'),200,100,json_extract(json,'$.players[0].rank'),json_extract(json,'$.players[0]') FROM fights WHERE boss_type IS NOT NULL;").unwrap();
    tx.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json) SELECT printf('d%05d',a.i*4+b.i),?1,printf('r%05d',a.i),?2+a.i*4+b.i,?3,CASE WHEN (a.i*4+b.i)%5=0 THEN 255 END,json_set(?4,'$.id',printf('d%05d',a.i*4+b.i),'$.run_id',printf('r%05d',a.i),'$.at',CAST(?2+a.i*4+b.i AS TEXT),'$.bag_rank',CASE WHEN (a.i*4+b.i)%5=0 THEN 255 END) FROM n a CROSS JOIN n b WHERE b.i<4",params![d.session_id,BASE as i64,d.bag_type,serde_json::to_string(&d).unwrap()]).unwrap();
    tx.execute("INSERT INTO deaths SELECT 'legacy-death',a.i*2+b.i+1,printf('c%03d',a.i%100),CASE WHEN b.i=0 THEN printf('r%05d',a.i) END,?1,?2 FROM n a CROSS JOIN n b WHERE b.i<2",params![BASE as i64,r.session_id]).unwrap();
    tx.execute_batch("INSERT INTO personal_bests SELECT json_extract(json,'$.players[0].player_id'),boss_type,min(id),100,started+2000000 FROM fights WHERE boss_type IS NOT NULL GROUP BY json_extract(json,'$.players[0].player_id'),boss_type;").unwrap();
    tx.commit().unwrap();
    let mut fixture = Fixture {
        _dir: dir,
        path,
        db,
        alerts: 0,
    };
    fixture.add_alerts(alerts);
    fixture
        .db
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    fixture
}
impl Fixture {
    pub fn add_alerts(&mut self, alerts: usize) {
        let view = AlertView {
            id: String::new(),
            at: Timestamp(BASE),
            kind: AlertKind::Death,
            run_id: None,
            explanation: "Synthetic retained observation".into(),
            details: None,
            presentation: None,
        };
        self.db.execute("INSERT INTO alerts SELECT printf('%064x',a.i*100+b.i),NULL,?1+(a.i*100+b.i)/4,CASE (a.i*100+b.i)%3 WHEN 0 THEN 'death' WHEN 1 THEN 'white_bag' ELSE 'personal_best' END,json_set(?2,'$.id',printf('%064x',a.i*100+b.i),'$.at',CAST(?1+(a.i*100+b.i)/4 AS TEXT),'$.kind',CASE (a.i*100+b.i)%3 WHEN 0 THEN 'death' WHEN 1 THEN 'white_bag' ELSE 'personal_best' END,'$.explanation',CASE WHEN (a.i*100+b.i)%997=0 THEN ?3 ELSE 'Synthetic retained observation' END) FROM n a CROSS JOIN n b WHERE b.i<100 AND a.i*100+b.i>=?4 AND a.i*100+b.i<?5",params![BASE as i64,serde_json::to_string(&view).unwrap(),LITERAL,self.alerts as i64,alerts as i64]).unwrap();
        self.alerts = alerts;
    }
    pub fn validate_counts(&self) {
        for (sql, expected) in [
            ("SELECT count(*) FROM runs",10_000), ("SELECT count(*) FROM fights",4_000_000),
            ("SELECT count(*) FROM (SELECT run_id FROM fights GROUP BY run_id HAVING count(*)=400)",10_000),
            ("SELECT count(*) FROM fights WHERE ended IS NOT NULL AND boss_type IS NOT NULL",200_000),
            ("SELECT count(*) FROM fight_players",200_000), ("SELECT count(*) FROM drops",40_000),
            ("SELECT count(*) FROM drops WHERE bag_rank=255",8_000), ("SELECT count(*) FROM deaths",20_000),
            ("SELECT count(*) FROM deaths WHERE run_id IS NULL",10_000),
            ("SELECT count(*) FROM personal_bests",10_000), ("SELECT count(*) FROM characters",100),
            ("SELECT count(*) FROM (SELECT character_id FROM personal_bests GROUP BY character_id HAVING count(*)=100)",100),
            ("SELECT count(*) FROM alerts",self.alerts as i64),
        ] { assert_eq!(count(&self.db,sql),expected,"{sql}"); }
        // Sample both ends of each compact/heavy archive variant independently.
        for id in ["f0000000", "f0000020", "f3999619", "f3999999"] {
            let f: FightView = serde_json::from_str(
                &self
                    .db
                    .query_row("SELECT json FROM fights WHERE id=?1", [id], |r| {
                        r.get::<_, String>(0)
                    })
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(f.id, id);
            assert_eq!(f.session_id, "demo-session");
            let c: CharacterView = serde_json::from_str(
                &self
                    .db
                    .query_row(
                        "SELECT json FROM characters WHERE id=?1",
                        [f.players
                            .first()
                            .map(|p| p.player_id.as_str())
                            .unwrap_or("c000")],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                maxed_engine::alerts::personal_best_candidate(&f, &c).is_some(),
                f.boss_type.is_some()
            );
        }
        assert_eq!(count(&self.db, "SELECT count(*) FROM event_receipts"), 1);
        eprintln!("validated: runs=10000 fights=4000000 eligible=200000 players=200000 drops=40000 whites=8000 deaths=20000 baselines=10000 alerts={}",self.alerts);
    }
    pub fn validate_pages(&self) -> StoreReader {
        let before = Instant::now();
        let reader = open_reader(&self.path).unwrap();
        eprintln!(
            "reader open/config/schema (separate): {:?}",
            before.elapsed()
        );
        for (label, filter) in cases(self.alerts) {
            let p = reader.list_alerts(filter.clone()).unwrap();
            let expected: Vec<_> = (0..self.alerts)
                .rev()
                .filter(|&i| matches_filter(i, &filter))
                .take(filter.limit as usize)
                .map(|i| format!("{i:064x}"))
                .collect();
            assert_eq!(
                p.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                expected,
                "{label}"
            );
            assert!(p.rows.len() <= 100);
            assert!(bytes(&p) <= 2 * 1024 * 1024);
            eprintln!("{label}: rows={} bytes={}", p.rows.len(), bytes(&p));
            self.assert_plan(label, &planned_sql(&filter));
        }
        // Traverse a bounded 2,000-row range entirely, including equal-time ties.
        let mut f = filter(100);
        f.range = Some(TimeRange {
            start: Timestamp(BASE),
            end: Timestamp(BASE + 500),
        });
        let mut seen = HashSet::new();
        let mut last = None;
        loop {
            let p = reader.list_alerts(f.clone()).unwrap();
            assert!(bytes(&p) <= 2 * 1024 * 1024);
            for row in &p.rows {
                let key = (row.at.0, row.id.clone());
                if let Some(prev) = last {
                    assert!(prev > key);
                }
                last = Some(key);
                assert!(seen.insert(row.id.clone()));
            }
            f.cursor = p.next;
            if f.cursor.is_none() {
                break;
            }
        }
        assert_eq!(seen.len(), 2000);
        reader
    }
    pub fn assert_plan(&self, label: &str, sql: &str) {
        let plan = self
            .db
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("; ");
        eprintln!("{label}: {sql}\n{plan}");
        assert!(!sql.contains("OFFSET"));
        assert!(!plan.contains("TEMP B-TREE"));
        assert!(plan.contains(if label == "kind-date" {
            "alerts_kind_at"
        } else {
            "alerts_at"
        }));
        if matches!(label, "middle" | "kind-date" | "date") {
            assert!(plan.contains("SEARCH"));
        }
    }
}
pub fn filter(limit: u16) -> AlertFilter {
    AlertFilter {
        kinds: vec![],
        exclude_kinds: vec![],
        kind: None,
        range: None,
        query: String::new(),
        cursor: None,
        limit,
    }
}
pub fn cases(n: usize) -> Vec<(&'static str, AlertFilter)> {
    let page = if n == STRESS { 100 } else { 50 };
    let first = filter(page);
    let mut middle = first.clone();
    middle.cursor = Some(AlertCursor {
        at: Timestamp(BASE + (n / 2 / 4) as u64),
        id: format!("{:064x}", n / 2),
    });
    let mut date = first.clone();
    date.range = Some(TimeRange {
        start: Timestamp(BASE + n as u64 / 16),
        end: Timestamp(BASE + n as u64 / 8),
    });
    let mut kind = date.clone();
    kind.kind = Some(AlertKind::WhiteBag);
    let mut text = first.clone();
    text.query = LITERAL.into();
    let mut absent = first.clone();
    absent.query = "absent %_\\ needle".into();
    vec![
        ("first", first),
        ("middle", middle),
        ("date", date),
        ("kind-date", kind),
        ("literal-search", text),
        ("absent-search", absent),
    ]
}
fn matches_filter(i: usize, f: &AlertFilter) -> bool {
    let at = BASE + i as u64 / 4;
    f.kind.as_ref().is_none_or(|_| i % 3 == 1)
        && f.range
            .as_ref()
            .is_none_or(|r| r.start.0 <= at && at < r.end.0)
        && f.cursor
            .as_ref()
            .is_none_or(|c| (at, format!("{i:064x}")) < (c.at.0, c.id.clone()))
        && (f.query.is_empty() || (f.query == LITERAL && i.is_multiple_of(997)))
}
pub fn planned_sql(f: &AlertFilter) -> String {
    let mut sql = "SELECT length(CAST(json AS BLOB)),json FROM alerts WHERE 1=1".to_string();
    if f.kind.is_some() {
        sql.push_str(" AND kind='white_bag'");
    }
    if let Some(r) = &f.range {
        sql.push_str(&format!(" AND at>={} AND at<{}", r.start.0, r.end.0));
    }
    if let Some(c) = &f.cursor {
        sql.push_str(&format!(" AND (at,id)<({},'{}')", c.at.0, c.id));
    }
    if !f.query.is_empty() {
        let escaped = f
            .query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
            .replace('\'', "''");
        sql.push_str(&format!(
            " AND json_extract(json,'$.explanation') LIKE '%{escaped}%' ESCAPE '\\'"
        ));
    }
    sql.push_str(&format!(" ORDER BY at DESC,id DESC LIMIT {}", f.limit + 1));
    sql
}
pub fn benchmark_queries(n: usize) {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let f = build(n);
    f.validate_counts();
    let reader = f.validate_pages();
    for (label, filter) in cases(n) {
        let expected = bytes(&reader.list_alerts(filter.clone()).unwrap());
        let mut times = Vec::new();
        for _ in 0..30 {
            let before = Instant::now();
            let page = reader.list_alerts(filter.clone()).unwrap();
            times.push(before.elapsed());
            assert_eq!(bytes(&page), expected);
        }
        times.sort();
        // Absent search is an unindexed full scan (999,999 steps at 1M alerts).
        // Outside Defender's scan path, Windows measured median 1.05-1.22s and
        // p95 1.13-1.64s: 1.5s was ~1.3x the median and failed intermittently.
        // The 2s budget still flags a ~30% regression against the measured p95.
        let limit = if n == STRESS && label == "absent-search" {
            2000
        } else if label.ends_with("search") {
            if n == STRESS {
                1500
            } else {
                250
            }
        } else if n == STRESS {
            250
        } else {
            50
        };
        eprintln!("alerts={n} {label}: n=30 median={:?} nearest-rank p95={:?} max={:?} bytes={expected} gate=<{limit}ms",times[14],times[28],times[29]);
        assert!(
            times[28] < Duration::from_millis(limit),
            "{label}: {:?} >= {limit}ms",
            times[28]
        );
    }
}

pub fn burst_fight(i: usize, stress: bool) -> FightView {
    let large = i < if stress { 10 } else { 20 };
    let players = if large && stress { 85 } else { 8 };
    let buckets = if large {
        if stress {
            1800
        } else {
            120
        }
    } else {
        8
    };
    let mut f = eligible_fight();
    f.id = format!("burst-{i:05}");
    f.boss_type = (i < 20).then_some(i as u16);
    let template = demo::demo_snapshot().fights.remove(0).players.remove(0);
    f.players = (0..players)
        .map(|p| {
            let mut v = template.clone();
            v.player_id = if p == 0 {
                "c000".into()
            } else {
                format!("p{p:03}")
            };
            v.object_id = p + 1;
            v.is_local = p == 0;
            v.name = Some(format!("Aé\"\\\n{p:03}"));
            v.damage = 300.;
            v.dps = Some(150.);
            v.rank = Some(p as u32 + 1);
            v.share = Some(1. / players as f64);
            v.sources = (0..3)
                .map(|s| {
                    let mut source = template.sources[0].clone();
                    source.kind =
                        [SourceKind::Weapon, SourceKind::Ability, SourceKind::Unknown][s].clone();
                    source.item_type = (s != 2).then_some(200);
                    source.ability = s == 1;
                    source.damage = 100.;
                    source.estimated = p != 0 && s == 2;
                    source.reasons = if source.estimated {
                        vec!["Synthetic estimate: missing projectile; é\"\\\n".into()]
                    } else {
                        vec![]
                    };
                    source
                })
                .collect();
            v
        })
        .collect();
    f.total_damage = 300. * players as f64;
    f.buckets = (0..buckets)
        .map(|b| DamageBucket {
            from: Timestamp(BASE + b as u64 * 1000),
            until: Timestamp(BASE + (b + 1) as u64 * 1000),
            total: f.total_damage / buckets as f64,
            local: Some(300. / buckets as f64),
        })
        .collect();
    f
}

pub fn burst_setup(db: &mut Connection) -> u64 {
    let mut c = demo::demo_snapshot().character.unwrap();
    c.id = "c000".into();
    c.identity_verified = true;
    let mut r = demo::demo_snapshot().run.unwrap();
    r.id = "r00000".into();
    r.character_id = Some(c.id.clone());
    r.started = Timestamp(BASE);
    r.ended = Some(Timestamp(BASE + 2_000_000));
    let mut session = demo::demo_snapshot().session.unwrap();
    session.started = Timestamp(BASE);
    session.ended = Some(Timestamp(BASE + 2_000_000));
    session.last_traffic = session.ended.unwrap();
    let events = [
        EngineEvent::SessionUpdated(session),
        EngineEvent::CharacterUpdated(c),
        EngineEvent::RunStarted(r),
    ];
    let mut seq = 0;
    for e in events {
        seq += 1;
        apply_batch(db, &[envelope(seq, e)]).unwrap();
    }
    // Seed twenty existing PB keys, so all twenty selected candidates improve.
    for i in 0..20 {
        let mut f = eligible_fight();
        f.id = format!("prior-{i:02}");
        f.boss_type = Some(i);
        seq += 1;
        apply_batch(db, &[envelope(seq, EngineEvent::FightEnded(f))]).unwrap();
    }
    // Existing unclassified bags become white in the twenty timed revisions.
    for i in 0..20 {
        let mut d = demo::demo_run_story().drops.remove(0);
        d.id = format!("burst-bag-{i:02}");
        d.run_id = Some("r00000".into());
        d.at = Timestamp(BASE + 1_000_000);
        d.bag_rank = None;
        seq += 1;
        apply_batch(db, &[envelope(seq, EngineEvent::BagDropped(d))]).unwrap();
    }
    seq
}

/// Samples only this benchmark process and its database files; no app/desktop claim.
pub struct Resources {
    child: std::process::Child,
    stop: PathBuf,
    output: PathBuf,
}
impl Drop for Resources {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.stop);
        let _ = self.child.wait();
    }
}
impl Resources {
    pub fn start(path: &std::path::Path) -> Self {
        let stop = path.with_extension("sampling");
        let output = path.with_extension("resources.json");
        std::fs::write(&stop, b"sample").unwrap();
        let quote = |p: &std::path::Path| p.to_string_lossy().replace('\'', "''");
        let script=format!("$ErrorActionPreference='Stop'; $peakPrivate=0L; $peakWorking=0L; $peakDb=0L; $peakWal=0L; while(Test-Path -LiteralPath '{}') {{ $p=Get-Process -Id {}; $peakPrivate=[Math]::Max($peakPrivate,$p.PrivateMemorySize64); $peakWorking=[Math]::Max($peakWorking,$p.PeakWorkingSet64); $d=Get-Item -LiteralPath '{}' -ErrorAction SilentlyContinue; $w=Get-Item -LiteralPath '{}-wal' -ErrorAction SilentlyContinue; if($d){{$peakDb=[Math]::Max($peakDb,$d.Length)}}; if($w){{$peakWal=[Math]::Max($peakWal,$w.Length)}}; if(!(Test-Path -LiteralPath '{}')){{Set-Content -LiteralPath '{}' -Value 'ready'}}; Start-Sleep -Milliseconds 50 }}; @{{peak_private_bytes=$peakPrivate;peak_working_set_bytes=$peakWorking;peak_db_bytes=$peakDb;peak_wal_bytes=$peakWal}} | ConvertTo-Json -Compress | Set-Content -LiteralPath '{}'",quote(&stop),std::process::id(),quote(path),quote(path),quote(&output),quote(&output),quote(&output));
        let child = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let sampler = Self {
            child,
            stop,
            output,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !sampler.output.exists() {
            assert!(
                Instant::now() < deadline,
                "resource sampler failed to start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        sampler
    }
    pub fn finish(mut self) -> String {
        std::fs::remove_file(&self.stop).unwrap();
        assert!(self.child.wait().unwrap().success());
        let result = std::fs::read_to_string(&self.output).unwrap();
        eprintln!("50ms sampled resource peaks (working-set peak is process lifetime): {result}");
        result
    }
}
pub fn file_sizes(path: &std::path::Path) -> (u64, u64) {
    let len = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    (
        len(path),
        len(&PathBuf::from(format!("{}-wal", path.display()))),
    )
}
pub fn benchmark_burst() {
    use super::{start_writer, AlertContext, SubmitError, WriterConfig};
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    for stress in [false, true] {
        let dir = perf_tempdir();
        let path = dir.path().join("burst.sqlite");
        let mut db = Connection::open(&path).unwrap();
        migrate(&mut db).unwrap();
        let mut seq = burst_setup(&mut db);
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "alerts-workload".into(),
                capacity: 1024,
                batch_size: 128,
                flush_millis: 60_000,
            },
        )
        .unwrap();
        let resources = Resources::start(&path);
        let start = Instant::now();
        let ctx = AlertContext {
            chat: None,
            live: true,
            preferences: AlertPreferences::default(),
            key_pop_dungeon: None,
            bag_window: None,
        };
        let fights = if stress { 3000 } else { 400 };
        let mut full = 0;
        let mut max_queued = 0;
        let mut max_event = 0;
        let mut total_bytes = 0;
        let mut pending_sizes = std::collections::VecDeque::new();
        let mut peak_queue_bytes = 0;
        let mut send = |event: EngineEvent| {
            seq += 1;
            let mut e = envelope(seq, event);
            let size = bytes(&e);
            max_event = max_event.max(size);
            total_bytes += size;
            assert!(size <= 8 * 1024 * 1024);
            loop {
                match writer.try_submit_with_alert_context(e, ctx.clone()) {
                    Ok(()) => break,
                    Err(SubmitError::Full(returned)) => {
                        full += 1;
                        e = returned;
                        writer.flush().unwrap();
                    }
                    Err(e) => panic!("{e:?}"),
                }
            }
            pending_sizes.push_back(size + bytes(&ctx));
            let queued = writer.status().queued;
            max_queued = max_queued.max(queued);
            assert!(queued <= 1024);
            while pending_sizes.len() > queued as usize {
                pending_sizes.pop_front();
            }
            let queued_bytes: usize = pending_sizes.iter().sum();
            peak_queue_bytes = peak_queue_bytes.max(queued_bytes);
            assert!(queued_bytes <= 32 * 1024 * 1024);
        };
        // Streaming production submission and ownership-preserving retry; never hold 3,000 DTOs.
        for i in 0..fights {
            send(EngineEvent::FightEnded(burst_fight(i, stress)));
        }
        for i in 0..20 {
            let mut d = demo::demo_run_story().drops.remove(0);
            d.id = format!("burst-bag-{i:02}");
            d.run_id = Some("r00000".into());
            d.at = Timestamp(BASE + 1_000_000);
            d.bag_rank = Some(255);
            d.contents_known = false;
            d.contents.clear();
            send(EngineEvent::DropUpdated(d));
        }
        send(EngineEvent::PlayerDied {
            at: Timestamp(BASE + 3_000_000),
            character_id: Some("c000".into()),
            run_id: Some("r00000".into()),
        });
        let submitted = start.elapsed();
        let before = Instant::now();
        writer.flush().unwrap();
        let flush = before.elapsed();
        let committed = Instant::now();
        let mut claimed = HashSet::new();
        let mut claim_times = Vec::new();
        let mut availability = Vec::new();
        let mut page_sizes = Vec::new();
        loop {
            let before = Instant::now();
            let page = writer.claim_alerts(32).unwrap();
            claim_times.push(before.elapsed());
            assert!(page.len() <= 32);
            if page.is_empty() {
                break;
            }
            page_sizes.push(page.len());
            for row in page {
                assert!(claimed.insert(row.view.id));
                availability.push(committed.elapsed());
            }
        }
        assert_eq!(claimed.len(), 40);
        assert_eq!(page_sizes, vec![32, 8]);
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        assert_eq!(
            count(&db, "SELECT count(*) FROM fights"),
            fights as i64 + 20
        );
        assert_eq!(
            count(&db, "SELECT count(*) FROM personal_bests WHERE dps=150"),
            20
        );
        assert_eq!(count(&db, "SELECT count(*) FROM drops"), 20);
        assert_eq!(count(&db, "SELECT count(*) FROM deaths"), 1);
        assert_eq!(
            count(&db, "SELECT count(*) FROM fight_players"),
            if stress { 24_770 } else { 3_200 } + 20
        );
        assert_eq!(
            count(&db, "SELECT count(*) FROM damage_sources"),
            if stress { 74_310 } else { 9_600 }
        );
        assert_eq!(
            count(
                &db,
                "SELECT sum(json_array_length(json,'$.buckets')) FROM fights"
            ),
            if stress { 41_920 } else { 5_440 }
        );
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM alert_occurrences WHERE state='pending'"
            ),
            0
        );
        assert_eq!(
            count(&db, "SELECT count(*) FROM event_receipts"),
            seq as i64
        );
        assert_eq!(writer.status().queued, 0);
        eprintln!("burst stress={stress}: fights={fights} eligible=20 revisions=20 preexisting_unclassified_bags=20 deaths=1 max_event_bytes={max_event} streamed_event_bytes={total_bytes} queue_peak_rows={max_queued} sampled_queue_bytes={peak_queue_bytes} production_queue_limit={} full_retries={full} submission_and_concurrent_commit={submitted:?} final_flush={flush:?} claim_times={claim_times:?}",32*1024*1024);
        if stress {
            assert!(
                full > 0,
                "3,000-event burst must exercise ownership-preserving backpressure"
            );
        }
        availability.sort();
        eprintln!("commit-flush-ack to claim availability p95={:?}; native presentation latency NOT MEASURED",availability[(availability.len()*95).div_ceil(100)-1]);
        writer.shutdown().unwrap();
        resources.finish();
    }
}

fn legacy_v1(f: &Fixture) {
    f.db.execute_batch("DROP TRIGGER pd_player_insert; DROP TRIGGER pd_player_delete; DROP TRIGGER pd_player_update; DROP TRIGGER pd_fight_association; DROP TABLE pd_player_contributions; DROP TABLE pd_run_players; DROP INDEX pd_home_session_runs; DROP INDEX pd_home_today_runs; DROP INDEX pd_run_evidence_kind; DROP INDEX pd_fight_players_totals; DROP INDEX pd_runs_comparison; DROP TABLE event_fts; DROP TABLE instance_events; DROP TABLE alert_occurrences; DROP TABLE personal_bests;
        DROP TABLE map_visits; DROP INDEX play_sessions_started; DROP TABLE chat_fts; DROP TABLE chat_messages; DROP INDEX drops_source; DROP INDEX runs_dungeon_ended; DROP INDEX drops_id_at; DROP INDEX drops_stats; ALTER TABLE drops DROP COLUMN source_type;
        DROP INDEX alerts_kind_at; DROP INDEX fights_final_boss; DROP INDEX fights_final_boss_key; DROP INDEX fight_players_identity; DROP INDEX character_aliases_verified;
        DROP TABLE stream_sessions; DROP INDEX fame_session; DROP INDEX deaths_session;
        ALTER TABLE fame_samples DROP COLUMN session_id; ALTER TABLE deaths DROP COLUMN session_id;
        ALTER TABLE drop_items RENAME TO v3_items; DROP INDEX drop_items_type;
        CREATE TABLE drop_items(drop_id TEXT NOT NULL REFERENCES drops(id),slot INTEGER NOT NULL,item_type INTEGER NOT NULL,quantity INTEGER NOT NULL CHECK(quantity>0),json TEXT NOT NULL,PRIMARY KEY(drop_id,slot));
        DROP TABLE v3_items; CREATE INDEX drop_items_type ON drop_items(item_type,drop_id);
        DROP TABLE fight_summaries; DELETE FROM schema_migrations WHERE version>=2; PRAGMA user_version=1;
        PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
}
fn legacy_v2(f: &Fixture) {
    use sha2::{Digest, Sha256};
    let script = include_str!("../../migrations/002_fight_summaries.sql");
    let tx = f.db.unchecked_transaction().unwrap();
    tx.execute_batch(script).unwrap();
    // Exactly the compact DTOs retained by this fixture, matching write_fight_summary.
    tx.execute("INSERT INTO fight_summaries SELECT id,run_id,started,json_object('id',id,'target_type',json_extract(json,'$.target_type'),'target_name',NULL,'boss_type',boss_type,'ordinal',0,'started',CAST(started AS TEXT),'ended',CAST(ended AS TEXT),'first_hit',json_extract(json,'$.first_hit'),'last_hit',json_extract(json,'$.last_hit'),'total_damage',json_extract(json,'$.total_damage'),'local_damage',CASE WHEN boss_type IS NOT NULL THEN 200.0 END,'local_dps',local_dps,'local_rank',json_extract(json,'$.players[0].rank'),'player_count',CASE WHEN boss_type IS NOT NULL THEN 1 ELSE 0 END,'partial',json('false'),'estimated',json('false')) FROM fights",[]).unwrap();
    tx.execute(
        "INSERT INTO schema_migrations VALUES(2,?1)",
        [format!("{:x}", Sha256::digest(script.as_bytes()))],
    )
    .unwrap();
    tx.pragma_update(None, "user_version", 2).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        count(&f.db, "SELECT count(*) FROM fight_summaries"),
        4_000_000
    );
    f.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

thread_local! {static P5_START:std::cell::RefCell<Option<Instant>>=const{std::cell::RefCell::new(None)};}
fn migration_progress(event: rusqlite::trace::TraceEvent<'_>) {
    if let rusqlite::trace::TraceEvent::Stmt(s, _) = event {
        let sql = s.sql();
        let sql = sql.trim_start();
        if sql.starts_with("CREATE TABLE personal_bests") {
            P5_START.with(|v| *v.borrow_mut() = Some(Instant::now()));
            eprintln!("P5 migration starts (prior P4 work excluded)");
        }
        if sql.starts_with("CREATE INDEX") || sql.starts_with("COMMIT") {
            eprintln!("migration progress: {}", sql.lines().next().unwrap_or(""));
        }
    }
}
pub fn benchmark_cold_upgrade() {
    use super::{start_writer, WriterConfig};
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    for n in [ORDINARY, STRESS] {
        let fixture = build(n);
        fixture.validate_counts();
        legacy_v1(&fixture);
        for version in [1, 2] {
            if version == 2 {
                legacy_v2(&fixture);
            }
            for trial in 1..=3 {
                let dir = perf_tempdir();
                let path = dir.path().join("cold.sqlite");
                std::fs::copy(&fixture.path, &path).unwrap(); // Fresh copy, outside every timer.
                let before_bytes = file_sizes(&path);
                let resources = Resources::start(&path);
                let mut db = Connection::open(&path).unwrap();
                assert_eq!(count(&db, "PRAGMA user_version"), version);
                db.trace_v2(
                    rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                    Some(migration_progress),
                );
                P5_START.with(|v| *v.borrow_mut() = None);
                let full_start = Instant::now();
                migrate(&mut db).unwrap();
                let reader_open = Instant::now();
                let reader = open_reader(&path).unwrap();
                let open = reader_open.elapsed();
                assert_eq!(reader.list_alerts(filter(50)).unwrap().rows.len(), 50);
                let p5 = P5_START.with(|v| v.borrow().expect("P5 CREATE TABLE trace").elapsed());
                let full = full_start.elapsed();
                // Cardinality validation is independent of elapsed time and never relaxed.
                for (kind, expected) in [
                    ("personal_best", 10_000),
                    ("white_bag", 8_000),
                    ("death", 20_000),
                ] {
                    assert_eq!(
                        db.query_row(
                            "SELECT count(*) FROM alert_occurrences WHERE kind=?1",
                            [kind],
                            |r| r.get::<_, i64>(0)
                        )
                        .unwrap(),
                        expected
                    );
                }
                assert_eq!(count(&db, "SELECT count(*) FROM alert_occurrences"), 38_000);
                // Migration 7 resets DPS records, preserving the earlier suppressed receipts.
                assert_eq!(count(&db, "SELECT count(*) FROM personal_bests"), 0);
                assert_eq!(count(&db,"SELECT count(*) FROM alert_occurrences WHERE state!='suppressed' OR alert_id IS NOT NULL OR source_stream_id IS NOT NULL"),0);
                assert_eq!(count(&db, "SELECT count(*) FROM alerts"), n as i64);
                assert_eq!(count(&db, "SELECT count(*) FROM event_receipts"), 1);
                assert_eq!(
                    count(&db, "SELECT count(*) FROM fight_summaries"),
                    4_000_000
                );
                let writer = start_writer(
                    path.clone(),
                    WriterConfig {
                        stream_id: "cold-claim-probe".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
                assert!(writer.claim_alerts(32).unwrap().is_empty());
                writer.shutdown().unwrap();
                let after_bytes = file_sizes(&path);
                resources.finish();
                eprintln!("cold alerts={n} v{version} trial={trial}/3: P5={p5:?}; full v{version}-to-current startup={full:?}; reader open={open:?}; DB/WAL before={before_bytes:?} after={after_bytes:?}; baselines=0 receipts=10000+8000+20000=38000 pending=0 claimable=0");
                assert!(
                    p5 <= Duration::from_secs(if n == ORDINARY { 60 } else { 120 }),
                    "P5 upgrade budget exceeded: {p5:?}"
                );
            }
        }
    }
}

/// Summary setup and scans must not mutate or warm the legacy list-query fixture.
pub fn benchmark_summaries(n: usize) {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let f = build(n);
    f.validate_counts();
    let reader = open_reader(&f.path).unwrap();
    let end = BASE + n as u64 / 4 + 1;
    let limit = Duration::from_millis(if n == STRESS { 1000 } else { 250 });
    let mut within_budget = true;
    // Exercise the runs primary-key lookup, not just a NULL-side outer join.
    f.db.execute("UPDATE alerts SET run_id='r00000',json=json_set(json,'$.run_id','r00000') WHERE id=(SELECT id FROM alerts WHERE kind='white_bag' ORDER BY at DESC,id DESC LIMIT 1)", []).unwrap();
    // Measure this independent summary fixture from a checkpointed state.
    f.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    for (label, range) in [
        ("summary-all", None),
        (
            "summary-30-day",
            Some(TimeRange {
                start: Timestamp(end - 30 * 86_400_000_000),
                end: Timestamp(end),
            }),
        ),
    ] {
        let predicate = range.as_ref().map_or_else(
            || "1=1".to_owned(),
            |r| format!("a.at>={} AND a.at<{}", r.start.0, r.end.0),
        );
        for (component, sql) in [
            ("kind-counts", format!("SELECT kind,count(*) FROM alerts a INDEXED BY alerts_kind_at WHERE {predicate} GROUP BY kind LIMIT 9")),
            ("last-white", format!("SELECT a.at,a.run_id,r.dungeon,length(CAST(a.run_id AS BLOB)),length(CAST(r.dungeon AS BLOB)) FROM (SELECT a.at,a.run_id FROM alerts a WHERE a.kind='white_bag' AND {predicate} ORDER BY a.at DESC,a.id DESC LIMIT 1) a LEFT JOIN runs r ON r.id=a.run_id")),
            ("first-at", format!("SELECT a.at FROM alerts a WHERE {predicate} ORDER BY a.at,a.id LIMIT 1")),
        ] {
            let plan = f.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap()
                .query_map([], |r| r.get::<_, String>(3)).unwrap()
                .collect::<Result<Vec<_>, _>>().unwrap().join("; ");
            eprintln!("alerts={n} {label} {component}: {sql}\n{plan}");
            let read = || {
                let mut statement = f.db.prepare(&sql).unwrap();
                let columns = statement.column_count();
                let mut rows = statement.query([]).unwrap();
                while let Some(row) = rows.next().unwrap() {
                    for column in 0..columns {
                        std::hint::black_box(row.get::<_, rusqlite::types::Value>(column).unwrap());
                    }
                }
            };
            read(); // Untimed warm-up on the same connection.
            let mut times = Vec::new();
            for _ in 0..30 {
                let start = Instant::now();
                read();
                times.push(start.elapsed());
            }
            times.sort();
            eprintln!("alerts={n} {label} {component}: n=30 median={:?} p95={:?} max={:?}", times[14], times[28], times[29]);
        }
        let request = AlertSummaryRequest { range };
        reader.alert_summary(request.clone()).unwrap();
        let mut times = Vec::new();
        for _ in 0..30 {
            let start = Instant::now();
            let result = reader.alert_summary(request.clone()).unwrap();
            times.push(start.elapsed());
            assert_eq!(result.total, n as u32);
            assert!(result.last_white.as_ref().unwrap().dungeon.is_some());
            assert!(bytes(&result) <= 2 * 1024 * 1024);
        }
        times.sort();
        eprintln!(
            "alerts={n} {label}: n=30 median={:?} p95={:?} max={:?} gate={limit:?}",
            times[14], times[28], times[29]
        );
        within_budget &= times[29] <= limit;
    }
    assert!(
        within_budget,
        "alert summary exceeds its timing budget; inspect component timings before schema changes"
    );
}

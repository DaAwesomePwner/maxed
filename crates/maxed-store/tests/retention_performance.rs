//! Synthetic, opt-in Phase 1 evidence. Never opens installed history.
mod common;

use crate::common::account_scope_fixture::ACCOUNT;
use common::installed_schema;
use maxed_store::{
    apply_batch,
    backup::{BackupEnvironment, BackupStore},
    maintenance::{Control, Phase, Step, StoppedMaintenance},
    migrate, start_writer, WriterConfig,
};
use maxed_types::*;
use rusqlite::{params, types::ValueRef, Connection};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    process::Command,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

const MIN_DATABASE_BYTES: u64 = 600_000_000;
const SEED: u64 = 0x2026_1012;
const STREAMS: u64 = 24;
const RECEIPTS_PER_STREAM: u64 = 256;

struct Space;
impl BackupEnvironment for Space {
    fn available_space(&self, _: &Path) -> Option<u64> {
        // Synthetic admission seam, not evidence about installed disk capacity.
        Some(u64::MAX)
    }
    fn timestamp(&self) -> u64 {
        200
    }
}

fn capture(stream: u64, sequence: u64) -> EventEnvelope {
    let mut event = common::envelope(
        sequence,
        EngineEvent::CaptureStateChanged(CaptureStatus::default()),
    );
    event.stream_id = format!("synthetic-stream-{stream:020}");
    assert_eq!(event.stream_id.len(), 37);
    event.at = Timestamp(SEED + sequence);
    event
}

fn scalar(db: &Connection, sql: &str) -> u64 {
    db.query_row(sql, [], |r| r.get::<_, i64>(0))
        .unwrap()
        .try_into()
        .unwrap()
}

fn file_bytes(path: &Path, suffix: &str) -> u64 {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    match std::fs::metadata(name) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => panic!("synthetic file measurement failed"),
    }
}

fn print_bytes(label: &str, db: &Connection, path: &Path) {
    let page_size = scalar(db, "PRAGMA page_size");
    let pages = scalar(db, "PRAGMA page_count");
    let free = scalar(db, "PRAGMA freelist_count");
    println!(
        "{label}: main={} wal={} shm={} allocated={} reusable={} occupied={}",
        file_bytes(path, ""),
        file_bytes(path, "-wal"),
        file_bytes(path, "-shm"),
        page_size * pages,
        page_size * free,
        page_size * (pages - free),
    );
}

fn seed(db: &mut Connection, version: usize) -> u64 {
    installed_schema::install(db, version);
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
        .unwrap();
    common::apply_legacy_story(db, &common::story_events());
    // Use real projections to grow the fixture; no padding table or sparse file.
    // These temporary additions are removed before the installed-schema fingerprint.
    db.execute_batch(include_str!("../migrations/016_maintenance_receipts.sql"))
        .unwrap();
    let mut begin = common::envelope(
        1,
        EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
    );
    begin.stream_id = "synthetic-history".into();
    apply_batch(db, &[begin]).unwrap();
    let mut template = demo::demo_snapshot().fights.remove(0);
    template.run_id = None;
    template.issues.clear();
    template.complete_capture = true;
    template.local_active_micros = Some(1_800_000_000);
    template.scope_id = None;
    template.ended = Some(Timestamp(template.started.0 + 1_800_000_000));
    template.first_hit = Some(template.started);
    template.last_hit = template.ended;
    template.end_reason = Some(FightEnd::MapChanged);
    template.player_series = None;
    let local = template
        .players
        .iter()
        .filter(|p| p.is_local)
        .map(|p| p.damage)
        .sum::<f64>();
    template.buckets = (0..1800)
        .map(|i| DamageBucket {
            from: Timestamp(template.started.0 + i * 1_000_000),
            until: Timestamp(template.started.0 + (i + 1) * 1_000_000),
            total: template.total_damage / 1800.0,
            local: Some(local / 1800.0),
            others_damage: None,
            others_presence: None,
        })
        .collect();
    let mut fights = 0;
    while scalar(db, "PRAGMA page_count") * scalar(db, "PRAGMA page_size") < MIN_DATABASE_BYTES {
        let events: Vec<_> = (0..8)
            .map(|_| {
                fights += 1;
                let mut fight = template.clone();
                fight.id = format!("scale-fight-{fights:06}");
                fight.target_id = fights as i32;
                let mut event = common::envelope(fights + 1, EngineEvent::FightEnded(fight));
                event.stream_id = "synthetic-history".into();
                event
            })
            .collect();
        apply_batch(db, &events).unwrap();
    }
    for stream in 0..STREAMS {
        let events: Vec<_> = (1..=RECEIPTS_PER_STREAM)
            .map(|sequence| capture(stream, sequence))
            .collect();
        apply_batch(db, &events).unwrap();
    }
    common::downgrade::remove_maintenance_schema(db);
    installed_schema::assert_installed(db, version);
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    // Leave a real committed update in WAL, plus an interrupted session for recovery.
    db.execute_batch("UPDATE play_sessions SET last_traffic=last_traffic+1,ended=NULL,json=json_set(json,'$.last_traffic',CAST(last_traffic+1 AS TEXT),'$.ended',NULL);")
        .unwrap();
    fights
}

// Streaming typed rows keeps the evidence collector independent of database size.
fn rows_hash(db: &Connection, sql: &str) -> (u64, [u8; 32]) {
    let mut statement = db.prepare(sql).unwrap();
    let columns = statement.column_count();
    let mut rows = statement.query([]).unwrap();
    let mut digest = Sha256::new();
    let mut count = 0;
    while let Some(row) = rows.next().unwrap() {
        count += 1;
        for i in 0..columns {
            match row.get_ref(i).unwrap() {
                ValueRef::Null => digest.update([0]),
                ValueRef::Integer(n) => {
                    digest.update([1]);
                    digest.update(n.to_le_bytes());
                }
                ValueRef::Real(n) => {
                    digest.update([2]);
                    digest.update(n.to_bits().to_le_bytes());
                }
                ValueRef::Text(bytes) => {
                    digest.update([3]);
                    digest.update((bytes.len() as u64).to_le_bytes());
                    digest.update(bytes);
                }
                ValueRef::Blob(bytes) => {
                    digest.update([4]);
                    digest.update((bytes.len() as u64).to_le_bytes());
                    digest.update(bytes);
                }
            }
        }
    }
    (count, digest.finalize().into())
}

fn history(db: &Connection) -> BTreeMap<String, (u64, [u8; 32])> {
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT IN ('schema_migrations','event_receipts','stream_heads','event_streams','event_receipts_v2','maintenance_jobs','backup_generations') ORDER BY name")
        .unwrap().query_map([], |r| r.get(0)).unwrap()
        .collect::<Result<_, _>>().unwrap();
    tables
        .into_iter()
        .map(|table| {
            let quoted = format!("\"{}\"", table.replace('"', "\"\""));
            let keys: Vec<String> = db
                .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk>0 ORDER BY pk")
                .unwrap()
                .query_map([&table], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let order = if keys.is_empty() {
                "rowid".into()
            } else {
                keys.iter()
                    .map(|key| format!("\"{}\"", key.replace('"', "\"\"")))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let columns = if table == "characters" {
                "id,observed_at,json"
            } else {
                "*"
            };
            let proof = rows_hash(
                db,
                &format!("SELECT {columns} FROM {quoted} ORDER BY {order}"),
            );
            (table, proof)
        })
        .collect()
}

fn queries(path: &Path, last: u64) -> Vec<[u8; 32]> {
    fn hash(value: impl serde::Serialize) -> [u8; 32] {
        Sha256::digest(serde_json::to_vec(&value).unwrap()).into()
    }
    let reader = maxed_store::open_reader(path).unwrap();
    let story = reader.get_run("demo-run").unwrap().unwrap();
    let fight = reader.get_fight(&story.fights[0].id).unwrap().unwrap();
    let session = story.run.session_id.clone();
    let stats = StatsFilter {
        range: None,
        character_id: None,
    };
    let mut result = vec![
        hash(&story),
        hash(
            reader
                .list_runs(common::filter(RunSort::Newest, 100))
                .unwrap(),
        ),
        hash(reader.dungeon_bests(Some(ACCOUNT)).unwrap()),
        hash(&fight),
        hash(reader.get_fight("scale-fight-000001").unwrap().unwrap()),
        hash(
            reader
                .get_fight(&format!("scale-fight-{last:06}"))
                .unwrap()
                .unwrap(),
        ),
        hash(
            reader
                .get_run_player("demo-run", &fight.players[0].player_id)
                .unwrap(),
        ),
        hash(
            reader
                .get_player_by_id(&fight.id, &fight.players[0].player_id)
                .unwrap(),
        ),
        hash(reader.stats_maps(stats.clone()).unwrap()),
        hash(reader.stats_dungeons(stats).unwrap()),
        hash(
            reader
                .stats_session(StatsSessionRequest {
                    id: session.clone(),
                    character_id: None,
                })
                .unwrap(),
        ),
        hash(reader.get_party_run("demo-run").unwrap()),
        hash(
            reader
                .boss_history(BossHistoryRequest {
                    range: None,
                    character_id: None,
                    target_types: None,
                })
                .unwrap(),
        ),
        hash(
            reader
                .dungeon_profile(
                    DungeonProfileRequest {
                        dungeon: story.run.dungeon.clone(),
                        character_id: None,
                        range: TimeRange {
                            start: Timestamp(0),
                            end: Timestamp(86_400_000_000),
                        },
                    },
                    Some(ACCOUNT),
                )
                .unwrap(),
        ),
    ];
    // SessionRunParty is an internal type; hash its serialized public inputs.
    for party in reader.stats_session_party(&session, None).unwrap() {
        result.push(hash((&party.run, &party.totals)));
    }
    for scope in [
        HomeScope::Session { id: session },
        HomeScope::Today {
            range: TimeRange {
                start: Timestamp(0),
                end: Timestamp(86_400_000_000),
            },
        },
    ] {
        let summary = reader.home_summary(scope.clone(), Some(ACCOUNT)).unwrap();
        assert!(summary.runs > 0 && summary.top_fight.is_some());
        assert!(summary.fame_delta.is_some());
        result.push(hash(summary));
        let hud = reader
            .home_hud(scope.clone(), Some(ACCOUNT), &[], &[])
            .unwrap();
        assert!(hud.combat.is_some());
        assert!(hud.fame_per_hour.is_some());
        result.push(hash(hud));
        result.push(hash(
            reader
                .scope_loot(scope, Some(ACCOUNT), None, 100, &[])
                .unwrap(),
        ));
    }
    result
}

fn distribution(label: &str, samples: &mut [f64]) {
    assert!(!samples.is_empty(), "measurement needs actual samples");
    samples.sort_by(f64::total_cmp);
    let percentile = |p: usize| samples[(samples.len() * p).div_ceil(100) - 1];
    println!(
        "{label}: n={} p50={:.3} p95={:.3} max={:.3}",
        samples.len(),
        percentile(50),
        percentile(95),
        samples[samples.len() - 1]
    );
}

fn metadata() {
    let output = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_else(|| "unavailable".into())
    };
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("phase1_600mb seed={SEED:#x} schemas=3 samples_per_schema=1 warm_opens_per_schema=3 profile={profile}");
    let status = output(
        "git",
        &["status", "--porcelain", "--untracked-files=normal"],
    );
    let dirty = match status.as_str() {
        "unavailable" => "unavailable",
        "" => "false",
        _ => "true",
    };
    println!(
        "revision={} dirty={dirty}",
        output("git", &["rev-parse", "HEAD"])
    );
    println!("toolchain={}", output("rustc", &["--version"]));
    #[cfg(windows)]
    {
        println!(
            "cpu={}",
            output(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    "(Get-CimInstance Win32_Processor | Select-Object -First 1).Name"
                ]
            )
        );
        println!(
            "os={}",
            output(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    "$o = Get-CimInstance Win32_OperatingSystem; $o.Caption + ' ' + $o.Version"
                ]
            )
        );
    }
    #[cfg(not(windows))]
    println!("cpu=unavailable os={}", std::env::consts::OS);
    println!("working_set: unavailable (no existing process-memory dependency); peak_working_set: unavailable");
}

#[test]
#[ignore = "600 MB per installed schema; see docs/development/storage-maintenance.md"]
fn phase1_600mb() {
    metadata();
    for version in [12, 14, 15] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        let generate = Instant::now();
        let fights = seed(&mut db, version);
        let receipt_count = scalar(&db, "SELECT count(*) FROM event_receipts");
        println!(
            "v{version}: generation_ms={:.3} fights={} legacy_receipts={} long_id_receipts={}",
            generate.elapsed().as_secs_f64() * 1000.0,
            fights,
            receipt_count,
            STREAMS * RECEIPTS_PER_STREAM
        );
        assert!(
            file_bytes(&path, "") >= MIN_DATABASE_BYTES,
            "the installed fixture must contain at least 600,000,000 actual database bytes"
        );
        assert!(
            (scalar(&db, "PRAGMA page_count") - scalar(&db, "PRAGMA freelist_count"))
                * scalar(&db, "PRAGMA page_size")
                >= MIN_DATABASE_BYTES
        );
        assert!(file_bytes(&path, "-wal") > 0);
        print_bytes("installed_bytes", &db, &path);
        // Isolate immutable existing scripts from 016. These are the exact 013/014
        // SQL/checksum writes used by migrate, committed as one existing-upgrade unit.
        let upgrade = Instant::now();
        if version == 12 {
            let tx = db.transaction().unwrap();
            for (version, script) in [
                (13, include_str!("../migrations/013_player_profiles.sql")),
                (14, include_str!("../migrations/014_party.sql")),
            ] {
                tx.execute_batch(script).unwrap();
                tx.execute(
                    "INSERT INTO schema_migrations VALUES(?1,?2)",
                    params![version, installed_schema::hash(script)],
                )
                .unwrap();
            }
            tx.pragma_update(None, "user_version", 14).unwrap();
            tx.commit().unwrap();
        }
        println!(
            "existing_013_014_ms={:.3} applied={}",
            upgrade.elapsed().as_secs_f64() * 1000.0,
            version == 12
        );
        installed_schema::assert_installed(&db, version.max(14));
        let indexes = Instant::now();
        if version < 15 {
            let tx = db.transaction().unwrap();
            let script = include_str!("../migrations/015_loot_stats_indexes.sql");
            tx.execute_batch(script).unwrap();
            tx.execute(
                "INSERT INTO schema_migrations VALUES(15,?1)",
                [installed_schema::hash(script)],
            )
            .unwrap();
            tx.pragma_update(None, "user_version", 15).unwrap();
            tx.commit().unwrap();
        }
        println!(
            "existing_015_ms={:.3} applied={}",
            indexes.elapsed().as_secs_f64() * 1000.0,
            version < 15
        );
        installed_schema::assert_installed(&db, 15);
        let before_ddl = history(&db);
        let ddl = Instant::now();
        migrate(&mut db).unwrap();
        println!(
            "additive_016_ms={:.3}",
            ddl.elapsed().as_secs_f64() * 1000.0
        );
        let upgraded_history = history(&db);
        assert!(
            before_ddl
                .iter()
                .all(|(table, rows)| upgraded_history.get(table) == Some(rows)),
            "upgrade must preserve installed history rows"
        );
        attribute_query_fixture(&db);
        assert_eq!(scalar(&db, "SELECT count(*) FROM maintenance_jobs"), 0);
        assert!(!dir.path().join("backups").exists());

        let ready = Instant::now();
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: capture(0, 1).stream_id,
                ..Default::default()
            },
        )
        .unwrap();
        println!(
            "existing_recovery_inclusive_ready_ms={:.3}",
            ready.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(
            scalar(
                &db,
                "SELECT count(*) FROM play_sessions WHERE ended IS NULL"
            ),
            0
        );
        let first = Instant::now();
        common::submit(&writer, capture(0, RECEIPTS_PER_STREAM + 1));
        assert_eq!(
            writer.flush().unwrap().sequence.0 .0,
            RECEIPTS_PER_STREAM + 1
        );
        println!(
            "first_durable_new_event_ms={:.3}",
            first.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(scalar(&db, "SELECT count(*) FROM maintenance_jobs"), 0);
        assert!(
            !dir.path().join("backups").exists(),
            "capture must precede any backup/copy"
        );
        writer.shutdown().unwrap();
        let mut warm = Vec::new();
        for _ in 0..3 {
            let ready = Instant::now();
            let writer = start_writer(path.clone(), Default::default()).unwrap();
            warm.push(ready.elapsed().as_secs_f64() * 1000.0);
            writer.shutdown().unwrap();
        }
        distribution("warm_ready_ms", &mut warm);
        let before_history = history(&db);
        let before_queries = queries(&path, fights);
        let before_receipts = rows_hash(&db, "SELECT stream_id,sequence,digest FROM event_receipts UNION ALL SELECT s.stream_id,r.sequence,lower(hex(r.digest)) FROM event_receipts_v2 r JOIN event_streams s USING(stream_key) ORDER BY 1,2");
        let before_heads = rows_hash(&db, "SELECT stream_id,max(sequence) FROM (SELECT stream_id,sequence FROM stream_heads UNION ALL SELECT stream_id,durable_sequence FROM event_streams WHERE durable_sequence IS NOT NULL) GROUP BY stream_id ORDER BY stream_id");
        print_bytes("before_conversion_bytes", &db, &path);
        let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
        let id = owner.request_receipt_conversion(100).unwrap().status.id;
        assert_eq!(
            owner.step().unwrap(),
            Step::Deferred(maxed_store::maintenance::PauseReason::AwaitingBackup)
        );
        let backup = Instant::now();
        let mut backup_steps = 1_u64;
        let info = owner
            .backup_receipt_conversion(&Space, &AtomicBool::new(false), &mut || {
                backup_steps += 1;
            })
            .unwrap();
        println!(
            "deferred_backup_ms={:.3} copy_steps={} backup_bytes={}",
            backup.elapsed().as_secs_f64() * 1000.0,
            backup_steps,
            info.bytes
        );
        let conversion = Instant::now();
        let mut batch_ms = Vec::new();
        let mut moved_per_unit = Vec::new();
        let mut previous = 0;
        let mut wal_high_water = file_bytes(&path, "-wal");
        let mut attempts = 0_u64;
        let mut deferrals = 0_u64;
        let mut status_at = Instant::now();
        loop {
            let unit = Instant::now();
            let step = owner.step().unwrap();
            let elapsed = unit.elapsed().as_secs_f64() * 1000.0;
            attempts += 1;
            wal_high_water = wal_high_water.max(file_bytes(&path, "-wal"));
            match step {
                Step::Committed(status) => {
                    batch_ms.push(elapsed);
                    moved_per_unit.push((status.processed_records - previous) as f64);
                    previous = status.processed_records;
                    if status.phase == Phase::Reclaimable {
                        break;
                    }
                }
                Step::Deferred(_) | Step::Idle => {
                    deferrals += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            if status_at.elapsed() >= Duration::from_secs(30) {
                println!(
                    "conversion_progress: units={} converted={} elapsed_s={:.3}",
                    batch_ms.len(),
                    previous,
                    conversion.elapsed().as_secs_f64()
                );
                status_at = Instant::now();
            }
            assert!(
                conversion.elapsed() < Duration::from_secs(1800),
                "synthetic conversion exceeded 30-minute harness guard"
            );
            std::thread::yield_now();
        }
        let seconds = conversion.elapsed().as_secs_f64();
        let status = owner.control(Control::Status(id)).unwrap().status;
        assert_eq!(status.processed_records, receipt_count);
        let throughput = receipt_count as f64 / seconds;
        println!("conversion: attempts={attempts} units={} deferred_or_idle={deferrals} receipts={receipt_count} wall_s={seconds:.3} receipts_per_s={throughput:.3} projected_546000_s={:.3}", batch_ms.len(), 546_000.0/throughput);
        distribution("unit_wall_ms", &mut batch_ms);
        distribution(
            "converted_receipts_per_unit_including_validation_units",
            &mut moved_per_unit,
        );
        let mut moving: Vec<_> = moved_per_unit.into_iter().filter(|n| *n > 0.0).collect();
        distribution("converted_receipts_per_copy_unit", &mut moving);
        println!("maintenance_wal_high_water_bytes={wal_high_water}");
        println!("eligible_records={:?} eligible_bytes={:?} processed_bytes={} blocked_records={:?} blocked_bytes={:?}",
            status.estimates.eligible_records, status.estimates.eligible_bytes, status.processed_bytes,
            status.estimates.blocked_records, status.estimates.blocked_bytes);
        assert_eq!(status.estimates.eligible_records, Some(receipt_count));
        drop(owner);
        print_bytes("after_conversion_bytes", &db, &path);
        let (_, usage) = BackupStore::read_inventory(&path).unwrap();
        println!(
            "backup_usage: retained={:?} temporary={:?}",
            usage.retained_bytes, usage.temporary_bytes
        );
        assert_eq!(
            history(&db),
            before_history,
            "all history bytes must survive conversion"
        );
        assert_eq!(
            queries(&path, fights),
            before_queries,
            "Phase 1 readers must be equal"
        );
        assert_eq!(rows_hash(&db, "SELECT s.stream_id,r.sequence,lower(hex(r.digest)) FROM event_receipts_v2 r JOIN event_streams s USING(stream_key) ORDER BY 1,2"), before_receipts);
        assert_eq!(rows_hash(&db, "SELECT stream_id,durable_sequence FROM event_streams WHERE durable_sequence IS NOT NULL ORDER BY stream_id"), before_heads);
        assert_eq!(scalar(&db, "SELECT count(*) FROM sqlite_schema WHERE name IN ('event_receipts','stream_heads')"), 0);
        for stream in 0..STREAMS {
            let changes = db.total_changes();
            apply_batch(
                &mut db,
                &[capture(stream, 1), capture(stream, RECEIPTS_PER_STREAM)],
            )
            .unwrap();
            assert_eq!(db.total_changes(), changes);
            let mut changed = capture(stream, 1);
            changed.at.0 += 1;
            assert!(matches!(
                apply_batch(&mut db, &[changed]),
                Err(maxed_store::StoreError::InvalidEvent)
            ));
            assert!(matches!(
                apply_batch(&mut db, &[capture(stream, RECEIPTS_PER_STREAM + 3)]),
                Err(maxed_store::StoreError::SequenceGap)
            ));
        }
        installed_schema::assert_integrity(&db);
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        print_bytes("checkpointed_bytes", &db, &path);
        println!("equality: history_tables={} query_results={} receipt_proofs={} head_proofs={} exact_retry_samples={} conflicting_retry_samples={} gap_samples={} integrity=ok",
            before_history.len(), before_queries.len(), before_receipts.0, before_heads.0, STREAMS*2, STREAMS, STREAMS);
        drop(db);
        dir.close().unwrap();
        println!("v{version}: private_fixture_removed=true");
    }
}

fn attribute_query_fixture(db: &Connection) {
    // Explicit synthetic attribution after verifying legacy preservation.
    common::account_scope_fixture::attribute(db);
    db.execute("INSERT INTO observation_characters SELECT 'fight',id,'demo-character' FROM fights WHERE run_id IS NULL ON CONFLICT(kind,observation_id) DO UPDATE SET character_id=excluded.character_id",[]).unwrap();
    db.execute("INSERT INTO identity_spans VALUES('synthetic-query-span',1,'demo-session',0,600000000,'demo-character',?1)",[ACCOUNT]).unwrap();
    db.execute("UPDATE play_sessions SET last_traffic=max(last_traffic,600000000),ended=600000000 WHERE id='demo-session'",[]).unwrap();
}

#[test]
fn account_scope_retention_query_comparisons_have_owned_data_and_elapsed_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = "scale-fight-000001".into();
    fight.run_id = None;
    fight.issues.clear();
    fight.complete_capture = true;
    fight.local_active_micros = Some(30_000_000);
    fight.first_hit = Some(fight.started);
    fight.last_hit = Some(Timestamp(fight.started.0 + 30_000_000));
    fight.ended = fight.last_hit;
    fight
        .players
        .iter_mut()
        .filter(|p| p.is_local)
        .for_each(|p| p.dps = Some(100.));
    apply_batch(
        &mut db,
        &[common::envelope(8, EngineEvent::FightEnded(fight))],
    )
    .unwrap();
    attribute_query_fixture(&db);
    let before = queries(&path, 1);
    drop(db);
    assert_eq!(queries(&path, 1), before);
}

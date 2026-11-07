//! Synthetic history acceptance. Packet replay, decoded injection and domain events
//! are deliberately labelled separately; none reads owner recordings.
use super::*;
use maxed_engine::EngineConfig;
use maxed_protocol::{defs::CreateSuccess, DecodedPacket};
use maxed_recording::PcapReader;
use maxed_store::{apply_batch, open_reader, WriterConfig};
use maxed_types::*;
use std::{future::Future, io::Cursor, path::PathBuf};

// Reuse the admitted reconstruction fixture mechanics, not another formula.
#[path = "../../../../crates/maxed-engine/tests/common/mod.rs"]
pub(super) mod reconstruction;

const OFFSET: u64 = 1_772_949_595_000_000;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-runs")
        .join(name)
}

fn ready<T>(future: impl Future<Output = Result<HistoryReply<T>, String>>) -> T {
    match tauri::async_runtime::block_on(future).unwrap() {
        HistoryReply::Ready { value } => value,
        _ => panic!("expected a typed Ready reply"),
    }
}

fn filter() -> RunFilter {
    RunFilter {
        character_id: None,
        dungeon: None,
        outcome: None,
        range: None,
        sort: RunSort::Oldest,
        cursor: None,
        limit: 100,
    }
}

fn frames(name: &str) -> Vec<RawFrame> {
    let mut reader = PcapReader::new(Cursor::new(
        std::fs::read(fixture(&format!("{name}.pcap"))).unwrap(),
    ))
    .unwrap();
    let mut frames = vec![];
    while let Some(frame) = reader.next_frame().unwrap() {
        frames.push(frame);
    }
    assert!(!frames.is_empty());
    frames
}

fn decoded(at: u64, packet: Packet) -> PipelineEvent {
    PipelineEvent::Decoded(DecodeEvent::Packet(DecodedPacket {
        conn: ConnId(1),
        dir: Direction::ServerToClient,
        at: Timestamp(at),
        id: 0,
        len: 0,
        alignment_verified: true,
        packet,
    }))
}

fn adapter(root: &std::path::Path, stream: &str) -> (Arc<CaptureRuntime>, Adapter) {
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let adapter = Adapter::open(
        root.join("live.sqlite3"),
        Arc::new(serde_json::from_slice(&std::fs::read(fixture("catalog.json")).unwrap()).unwrap()),
        EngineConfig {
            stream_id: stream.into(),
            identity_key: [7; 32],
            ..Default::default()
        },
        WriterConfig {
            stream_id: stream.into(),
            ..Default::default()
        },
        false,
        runtime.shared.views.clone(),
        Bridge::default(),
    )
    .unwrap();
    (runtime, adapter)
}

fn flush(adapter: &mut Adapter) {
    adapter.writer_flush_for_test();
    adapter.refresh_store();
    adapter.publish_snapshot();
}

fn assert_durable(runtime: &CaptureRuntime) {
    let status = runtime.store_status();
    let snapshot = runtime.snapshot();
    assert_eq!(
        status.stream_id.as_deref(),
        Some(snapshot.stream_id.as_str())
    );
    assert_eq!(status.durable, Some(snapshot.sequence));
    assert_eq!(status.queued, 0);
    assert_ne!(status.state, StoreState::Failed);
}

fn cardinalities(path: &std::path::Path) -> Vec<i64> {
    let db = rusqlite::Connection::open(path).unwrap();
    let mut counts: Vec<i64> = ["runs", "fights", "drops", "timeline"]
        .into_iter()
        .map(|table| {
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect();
    counts.push(super::receipt_tests::count(&db));
    counts
}

fn counts(label: &str, mut values: Vec<usize>) {
    assert!(
        !values.is_empty(),
        "measurement must discover actual terminal rows"
    );
    values.sort_unstable();
    let median = (values[(values.len() - 1) / 2] + values[values.len() / 2]) as f64 / 2.0;
    println!(
        "C1 {label}: n={} min={} median={median} max={}",
        values.len(),
        values[0],
        values[values.len() - 1]
    );
}

#[test]
fn replay_run_fight_player_matches_stored_sources() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("manifest.json")).unwrap()).unwrap();
    let mut measured = 0;
    for name in ["demo", "complete", "disconnected", "zero-duration"] {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(CaptureRuntime::default());
        let mut options = RuntimeOptions::live(dir.path().into(), None);
        options.namespace = Namespace::Demo;
        let events: Arc<Mutex<Vec<EventEnvelope>>> = Arc::default();
        let received = events.clone();
        runtime
            .replay_reader(
                options,
                Cursor::new(std::fs::read(fixture(&format!("{name}.pcap"))).unwrap()),
                Bridge {
                    event: Arc::new(move |event| received.lock().unwrap().push(event)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_durable(&runtime);
        let before_status = runtime.store_status();
        let page = ready(runtime.list_runs(filter()));
        assert_eq!(page.rows.len(), 1);
        assert!(page.next.is_none());
        let story = ready(runtime.get_run(page.rows[0].id.clone())).unwrap();
        assert_eq!(story.run, page.rows[0]);
        let strip = ready(runtime.get_run_fight_strip(story.run.id.clone(), None));
        let expected_ids: Vec<_> = story
            .fights
            .iter()
            .filter(|f| {
                matches!(
                    f.target_kind,
                    Some(TargetKind::Boss | TargetKind::EventBoss)
                ) || f.boss_type.is_some()
            })
            .map(|f| &f.id)
            .collect();
        assert_eq!(
            strip.iter().map(|f| &f.fight_id).collect::<Vec<_>>(),
            expected_ids
        );
        for row in &strip {
            let fight = ready(runtime.get_fight(row.fight_id.clone())).unwrap();
            let locals: Vec<_> = fight.players.iter().filter(|p| p.is_local).collect();
            let local = (locals.len() == 1)
                .then(|| locals[0])
                .filter(|_| !fight.issues.contains(&CoverageIssue::MissingIdentity));
            assert_eq!(row.damage, local.map(|p| p.damage));
            assert_eq!(row.rank, local.and_then(|p| p.rank));
            assert_eq!(row.share, local.and_then(|p| p.share));
            assert_eq!(row.player_count as usize, fight.players.len());
        }

        let expected = &manifest["expected"][name];
        assert_eq!(story.run.started.0, expected["started"].as_u64().unwrap());
        assert_eq!(
            serde_json::to_value(&story.run.outcome).unwrap(),
            expected["outcome"]
        );
        assert_eq!(
            story.drops.len() as u64,
            expected["drops"].as_u64().unwrap()
        );
        assert_eq!(story.fights.len(), 1);
        let mut players = vec![];
        let mut sources = vec![];
        let mut buckets = vec![];
        for summary in &story.fights {
            let fields = serde_json::to_value(summary).unwrap();
            for forbidden in ["players", "sources", "buckets"] {
                assert!(fields.get(forbidden).is_none());
            }
            assert!(fields
                .as_object()
                .unwrap()
                .values()
                .all(|v| !v.is_array() && !v.is_object()));
            let detail = ready(runtime.get_fight(summary.id.clone())).unwrap();
            assert_eq!(detail.run_id.as_ref(), Some(&story.run.id));
            assert_eq!(summary.total_damage, expected["damage"].as_f64().unwrap());
            assert_eq!(summary.total_damage, detail.total_damage);
            assert_eq!(summary.target_type, detail.target_type);
            assert_eq!(summary.boss_type, detail.boss_type);
            assert_eq!(
                (
                    summary.started,
                    summary.ended,
                    summary.first_hit,
                    summary.last_hit
                ),
                (
                    detail.started,
                    detail.ended,
                    detail.first_hit,
                    detail.last_hit
                )
            );
            assert_eq!(summary.player_count as usize, detail.players.len());
            assert_eq!(
                summary.partial,
                !detail.complete_capture || !detail.issues.is_empty()
            );
            assert_eq!(
                summary.estimated,
                detail
                    .players
                    .iter()
                    .flat_map(|p| &p.sources)
                    .any(|s| s.estimated)
            );
            players.push(detail.players.len());
            buckets.push(detail.buckets.len());
            for player in &detail.players {
                assert_eq!(
                    ready(runtime.get_player_breakdown(detail.id.clone(), player.object_id)),
                    Some(player.clone())
                );
                assert_eq!(
                    player.damage,
                    player.sources.iter().map(|s| s.damage).sum::<f64>()
                );
                assert_eq!(
                    player.hits,
                    player.sources.iter().map(|s| s.hits).sum::<u32>()
                );
                assert_eq!(player.sources.len(), 1);
                let source = &player.sources[0];
                assert_eq!(source.item_type, Some(256));
                assert_eq!(source.kind, SourceKind::Weapon);
                assert!(!source.ability && !source.estimated);
                assert!(source.reasons.is_empty());
                sources.push(player.sources.len());
                if player.is_local {
                    assert_eq!(summary.local_damage, Some(player.damage));
                    assert_eq!(summary.local_dps, player.dps);
                    assert_eq!(summary.local_rank, player.rank);
                    assert_eq!(player.dps, expected["dps"].as_f64());
                    assert_eq!(player.share, Some(1.0));
                }
            }
            let reopened = open_reader(&dir.path().join("demo.sqlite3")).unwrap();
            assert_eq!(
                reopened.get_run(&story.run.id).unwrap(),
                Some(story.clone())
            );
            assert_eq!(
                reopened.get_fight(&detail.id).unwrap(),
                Some(detail.clone())
            );
            for player in &detail.players {
                assert_eq!(
                    reopened
                        .get_player_breakdown(&detail.id, player.object_id)
                        .unwrap(),
                    Some(player.clone())
                );
            }
        }
        counts(&format!("{name} fights/run"), vec![story.fights.len()]);
        counts(&format!("{name} players/fight (run-associated)"), players);
        counts(&format!("{name} sources/player (run-associated)"), sources);
        counts(&format!("{name} buckets/fight (run-associated)"), buckets);
        // Include the separate Realm fight in terminal fight-level measurements.
        let mut terminal = std::collections::BTreeMap::new();
        for event in events.lock().unwrap().iter() {
            if let EngineEvent::FightEnded(fight) = &event.event {
                terminal.insert(fight.id.clone(), fight.clone());
            }
        }
        assert_eq!(terminal.len(), 2);
        counts(
            &format!("{name} players/fight (all terminal)"),
            terminal.values().map(|f| f.players.len()).collect(),
        );
        counts(
            &format!("{name} sources/player (all terminal)"),
            terminal
                .values()
                .flat_map(|f| &f.players)
                .map(|p| p.sources.len())
                .collect(),
        );
        counts(
            &format!("{name} buckets/fight (all terminal)"),
            terminal.values().map(|f| f.buckets.len()).collect(),
        );
        let path = dir.path().join("demo.sqlite3");
        let before = cardinalities(&path);
        assert_eq!(&before[..3], &[1, 2, story.drops.len() as i64]);
        let mut db = rusqlite::Connection::open(&path).unwrap();
        apply_batch(&mut db, &events.lock().unwrap()).unwrap();
        drop(db);
        assert_eq!(cardinalities(&path), before);
        runtime.stop().unwrap();
        assert_eq!(ready(runtime.get_run(story.run.id.clone())), Some(story));
        assert_eq!(runtime.store_status(), before_status);
        // The real decoder permits only these synthetic packet kinds. All
        // identity-bearing stats must match the reviewed invented identities.
        let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
        let mut packets = 0;
        let mut identities = std::collections::BTreeSet::new();
        for frame in frames(name) {
            for event in pipeline.push(frame) {
                if let PipelineEvent::Decoded(DecodeEvent::Packet(packet)) = event {
                    assert!(packet.alignment_verified);
                    packets += 1;
                    match packet.packet {
                        Packet::Update(update) => {
                            for object in update.newObjects {
                                for stat in object.status.stats {
                                    if matches!(stat.statTypeNum, 31 | 38) {
                                        if let Some(value) = stat.stringStatValue {
                                            identities.insert(value);
                                        }
                                    }
                                }
                            }
                        }
                        Packet::MapInfo(_)
                        | Packet::CreateSuccess(_)
                        | Packet::NewTick(_)
                        | Packet::ServerPlayerShoot(_)
                        | Packet::Damage(_)
                        | Packet::Notification(_) => {}
                        _ => panic!("unadmitted synthetic packet kind"),
                    }
                }
            }
        }
        assert!(packets > 0);
        assert_eq!(
            identities,
            ["DemoHero", "DemoAccount", "DemoAlly", "DemoAllyAccount"]
                .map(String::from)
                .into_iter()
                .collect()
        );
        measured += 1;
    }
    assert_eq!(measured, 4, "nonzero discovered fixture count");
}

fn write_events(
    db: &mut rusqlite::Connection,
    sequence: &mut u64,
    events: impl IntoIterator<Item = EngineEvent>,
) {
    let rows: Vec<_> = events
        .into_iter()
        .map(|event| {
            *sequence += 1;
            EventEnvelope {
                fight_chart_omitted: false,
                stream_id: "synthetic-c1-domain".into(),
                sequence: Sequence(Timestamp(*sequence)),
                at: Timestamp(*sequence),
                event,
            }
        })
        .collect();
    apply_batch(db, &rows).unwrap();
}

#[test]
fn late_counter_revision_reopens_one_story() {
    // Real decoded base packets, with Victory deliberately omitted. Counter
    // and identity refresh are decoded injection, not encrypted pcap coverage.
    for (label, delay, counter, accepted) in [
        ("eligible", 12_010_000, "AQAAAAAAAAAAAgAAAAAAAAAAAAAB", true),
        ("expired", 43_010_000, "AQAAAAAAAAAAAgAAAAAAAAAAAAAB", false),
        (
            "delta-two",
            12_010_000,
            "AQAAAAAAAAAAAgAAAAAAAAAAAAAC",
            false,
        ),
        ("invalid", 12_010_000, "not valid PcStats!", false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, mut adapter) = adapter(dir.path(), &format!("synthetic-c1-{label}"));
        let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
        for frame in frames("complete") {
            for event in pipeline.push(frame) {
                if matches!(&event, PipelineEvent::Decoded(DecodeEvent::Packet(p)) if matches!(p.packet, Packet::Notification(_)))
                {
                    continue;
                }
                adapter.decoded(event).unwrap();
            }
        }
        flush(&mut adapter);
        assert_durable(&runtime);
        let page = ready(runtime.list_runs(filter()));
        assert_eq!(page.rows.len(), 1);
        let before = ready(runtime.get_run(page.rows[0].id.clone())).unwrap();
        assert_eq!(before.run.outcome, Some(Outcome::Left));
        adapter
            .decoded(decoded(
                OFFSET + delay,
                Packet::CreateSuccess(CreateSuccess {
                    objectId: 1,
                    charId: 1,
                    str: counter.into(),
                }),
            ))
            .unwrap();
        let mut next = Pipeline::new(Arc::new(AtomicBool::new(false)));
        for frame in frames("complete")
            .into_iter()
            .filter(|f| f.at.0 <= OFFSET + 1_020_000)
        {
            for event in next.push(frame) {
                if let PipelineEvent::Decoded(DecodeEvent::Packet(mut packet)) = event {
                    if matches!(packet.packet, Packet::Update(_)) {
                        packet.at = Timestamp(OFFSET + delay + 10_000);
                        adapter
                            .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(packet)))
                            .unwrap();
                    }
                }
            }
        }
        adapter
            .finish(Timestamp(OFFSET + delay + 20_000), FightEnd::CaptureStopped)
            .unwrap();
        adapter.close().unwrap();
        assert_durable(&runtime);
        let after = ready(runtime.get_run(before.run.id.clone())).unwrap();
        let page = ready(runtime.list_runs(filter()));
        assert_eq!(page.rows, vec![after.run.clone()]);
        assert_eq!(after.run.id, before.run.id);
        assert_eq!(
            after.run.outcome,
            Some(if accepted {
                Outcome::Cleared
            } else {
                Outcome::Left
            }),
            "{label}"
        );
        assert_eq!(
            after
                .timeline
                .iter()
                .filter(|t| t.kind == TimelineKind::Completion)
                .count(),
            usize::from(accepted)
        );
        assert_eq!(
            after
                .run
                .evidence
                .iter()
                .filter(|e| matches!(e.evidence, Evidence::CompletionCounter { .. }))
                .count(),
            usize::from(accepted)
        );
        assert_eq!(after.run.completed_at.is_some(), accepted);
        if accepted {
            assert!(after.run.evidence.iter().any(|e| matches!(
                e.evidence,
                Evidence::CompletionCounter {
                    before: 0,
                    after: 1,
                    ..
                }
            )));
        }
        assert!(after.comparison.is_none());
        assert_eq!(
            open_reader(&dir.path().join("live.sqlite3"))
                .unwrap()
                .get_run(&after.run.id)
                .unwrap(),
            Some(after)
        );
    }
    // Typed domain revision: the third earlier observed clear changes another
    // saved story's comparison, without rewriting that current run.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut character = demo::demo_snapshot().character.unwrap();
    character.identity_verified = true;
    character.account_id = Some(format!("account-{}", "a".repeat(64)));
    let mut sequence = 0;
    write_events(
        &mut db,
        &mut sequence,
        [
            EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
            EngineEvent::CharacterUpdated(character.clone()),
            EngineEvent::LocalIdentityChanged {
                at: Timestamp(1_000_000),
                character_id: Some(character.id.clone()),
                account_id: character.account_id.clone(),
            },
        ],
    );
    let mut earlier = vec![];
    for i in 0..4 {
        let mut run = demo::demo_run_story().run;
        run.id = format!("synthetic-comparison-{i}");
        run.character_id = Some(character.id.clone());
        run.started = Timestamp(20_000_000 + i * 20_000_000);
        run.completed_at = (i != 2).then_some(Timestamp(
            run.started.0 + if i == 3 { 8_000_000 } else { 10_000_000 },
        ));
        run.ended = Some(Timestamp(run.started.0 + 12_000_000));
        run.outcome = Some(if i == 2 {
            Outcome::Left
        } else {
            Outcome::Cleared
        });
        run.evidence = run
            .completed_at
            .map(|at| EvidenceAt {
                at,
                evidence: Evidence::Victory,
            })
            .into_iter()
            .collect();
        write_events(
            &mut db,
            &mut sequence,
            [EngineEvent::RunStarted(run.clone())],
        );
        earlier.push(run);
    }
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(dir.path().into()).unwrap();
    assert!(ready(runtime.get_run(earlier[3].id.clone()))
        .unwrap()
        .comparison
        .is_none());
    earlier[2].completed_at = Some(Timestamp(earlier[2].started.0 + 10_000_000));
    earlier[2].outcome = Some(Outcome::Cleared);
    earlier[2].evidence = vec![EvidenceAt {
        at: earlier[2].completed_at.unwrap(),
        evidence: Evidence::Victory,
    }];
    write_events(
        &mut db,
        &mut sequence,
        [EngineEvent::RunUpdated(earlier[2].clone())],
    );
    let current = ready(runtime.get_run(earlier[3].id.clone())).unwrap();
    assert_eq!(current.run, earlier[3]);
    let comparison = current.comparison.as_ref().unwrap();
    assert_eq!(comparison.previous_clears, 3);
    assert_eq!(comparison.mean_micros, 10_000_000.0);
    assert!((comparison.change_fraction + 0.2).abs() < 1e-12);
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_run(&current.run.id)
            .unwrap(),
        Some(current)
    );
}

#[test]
fn saved_death_and_partial_story_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let snapshot = demo::demo_snapshot();
    let mut sequence = 0;
    write_events(
        &mut db,
        &mut sequence,
        [
            EngineEvent::SessionUpdated(snapshot.session.clone().unwrap()),
            EngineEvent::CharacterUpdated(snapshot.character.clone().unwrap()),
        ],
    );
    let mut cleared = demo::demo_run_story().run;
    cleared.died_at = Some(Timestamp(6_000_000));
    cleared.evidence.push(EvidenceAt {
        at: Timestamp(6_000_000),
        evidence: Evidence::DeathPacket,
    });
    write_events(
        &mut db,
        &mut sequence,
        [
            EngineEvent::RunStarted(cleared.clone()),
            EngineEvent::PlayerDied {
                at: Timestamp(6_000_000),
                character_id: cleared.character_id.clone(),
                run_id: Some(cleared.id.clone()),
            },
            EngineEvent::RunEnded(cleared.clone()),
        ],
    );
    for (index, evidence, issue) in [
        (0, Evidence::CaptureGap, CoverageIssue::StreamGap),
        (1, Evidence::AppClosed, CoverageIssue::MissingCatalog),
    ] {
        let mut run = cleared.clone();
        run.id = format!("synthetic-partial-{index}");
        run.character_id = None;
        run.started = Timestamp(10_000_000 + index * 10_000_000);
        run.ended = Some(Timestamp(run.started.0 + 2_000_000));
        run.completed_at = None;
        run.died_at = None;
        run.outcome = Some(Outcome::Unknown);
        run.party_size = None;
        run.complete_capture = false;
        run.issues = vec![CoverageIssue::MissingIdentity, issue];
        run.evidence = vec![EvidenceAt {
            at: run.ended.unwrap(),
            evidence,
        }];
        let mut fight = snapshot.fights[0].clone();
        fight.id = format!("synthetic-zero-duration-{index}");
        fight.run_id = Some(run.id.clone());
        fight.started = run.started;
        fight.ended = run.ended;
        fight.first_hit = Some(run.started);
        fight.last_hit = fight.first_hit;
        fight.target_type = None;
        fight.boss_type = None;
        fight.end_reason = Some(if index == 0 {
            FightEnd::CaptureStopped
        } else {
            FightEnd::AppClosed
        });
        fight.issues.clone_from(&run.issues);
        fight.buckets.clear();
        fight.players[0].dps = None;
        fight.players[0].rank = None;
        fight.players[0].share = None;
        let mut bag = snapshot.drops[0].clone();
        bag.id = format!("synthetic-unknown-bag-{index}");
        bag.run_id = Some(run.id.clone());
        bag.at = run.started;
        bag.contents.clear();
        bag.items.clear();
        bag.contents_known = false;
        write_events(
            &mut db,
            &mut sequence,
            [
                EngineEvent::RunStarted(run.clone()),
                EngineEvent::FightEnded(fight),
                EngineEvent::BagDropped(bag),
                EngineEvent::RunEnded(run),
            ],
        );
    }
    drop(db);
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(dir.path().into()).unwrap();
    let story = ready(runtime.get_run(cleared.id.clone())).unwrap();
    assert_eq!(story.run, cleared);
    for (kind, at) in [
        (TimelineKind::Completion, 5_000_000),
        (TimelineKind::Death, 6_000_000),
    ] {
        let entries: Vec<_> = story.timeline.iter().filter(|t| t.kind == kind).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].at, Timestamp(at));
        assert_eq!(entries[0].related_id.as_ref(), Some(&cleared.id));
    }
    for index in 0..2 {
        let story = ready(runtime.get_run(format!("synthetic-partial-{index}"))).unwrap();
        assert!(story.character.is_none());
        assert_eq!(story.run.outcome, Some(Outcome::Unknown));
        assert_eq!(
            (
                story.run.completed_at,
                story.run.died_at,
                story.run.party_size
            ),
            (None, None, None)
        );
        assert!(story.run.issues.contains(&CoverageIssue::MissingIdentity));
        assert!(story.run.issues.contains(&if index == 0 {
            CoverageIssue::StreamGap
        } else {
            CoverageIssue::MissingCatalog
        }));
        assert!(story.run.evidence.iter().any(|e| e.evidence
            == if index == 0 {
                Evidence::CaptureGap
            } else {
                Evidence::AppClosed
            }));
        assert!(story
            .timeline
            .iter()
            .all(|t| !matches!(t.kind, TimelineKind::Completion | TimelineKind::Death)));
        assert!(!story.drops[0].contents_known);
        assert!(story.drops[0].contents.is_empty());
        let summary = &story.fights[0];
        assert!(summary.partial);
        assert_eq!(
            (summary.local_damage, summary.local_dps, summary.local_rank),
            (None, None, None)
        );
        assert_eq!((summary.target_type, summary.boss_type), (None, None));
        let player = ready(runtime.get_player_breakdown(summary.id.clone(), 1)).unwrap();
        assert_eq!(player.damage, 300.0);
        assert_eq!((player.dps, player.rank, player.share), (None, None, None));
        assert_eq!(
            open_reader(&path).unwrap().get_run(&story.run.id).unwrap(),
            Some(story)
        );
    }
}

#[test]
fn story_waits_for_writer_retry_and_survives_stop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let (runtime, mut adapter) = adapter(dir.path(), "synthetic-c1-retry");
    let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames("disconnected") {
        for event in pipeline.push(frame) {
            adapter.decoded(event).unwrap();
        }
    }
    flush(&mut adapter);
    assert_durable(&runtime);
    let run = ready(runtime.list_runs(filter())).rows.remove(0);
    let before = ready(runtime.get_run(run.id.clone())).unwrap();
    assert!(before.run.ended.is_none());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(adapter
        .finish(Timestamp(OFFSET + 8_500_000), FightEnd::AppClosed)
        .is_err());
    let failed = runtime.store_status();
    assert_eq!(failed.state, StoreState::Failed);
    assert!(failed.queued > 0);
    assert!(failed.durable.unwrap().0 .0 < runtime.snapshot().sequence.0 .0);
    // Ready describes the read, not freshness; the failed/behind receipt must
    // continue to gate the UI. The actual saved story is still the old one.
    assert_eq!(ready(runtime.get_run(run.id.clone())), Some(before));
    assert_eq!(runtime.store_status(), failed);
    db.execute_batch("ROLLBACK").unwrap();
    adapter.retry_and_close().unwrap();
    assert_durable(&runtime);
    let after = ready(runtime.get_run(run.id.clone())).unwrap();
    assert_eq!(after.run.ended, Some(Timestamp(OFFSET + 8_500_000)));
    assert!(after
        .run
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::AppClosed));
    assert_eq!(
        ready(runtime.list_runs(filter())).rows,
        vec![after.run.clone()]
    );
    let saved_counts = cardinalities(&path);
    assert_eq!(
        after
            .timeline
            .iter()
            .filter(|t| t.kind == TimelineKind::Exit)
            .count(),
        0
    );
    assert_eq!(
        after
            .timeline
            .iter()
            .filter(|t| t.kind == TimelineKind::CaptureIssue)
            .count(),
        1
    );
    runtime.stop().unwrap();
    assert_eq!(ready(runtime.get_run(run.id.clone())), Some(after.clone()));
    let receipt = runtime.store_status();
    let reader = runtime.clone();
    let error = tauri::async_runtime::block_on(runtime.submit_history(
        history::HistoryCaller::NativeTest,
        history::HistoryKey::Run(run.id.clone()),
        move |job| reader.read_history(job, || (), |_| Err(maxed_store::StoreError::Io)),
    ))
    .unwrap_err();
    assert_eq!(error, home::READ_FAILED);
    assert_eq!(runtime.store_status(), receipt);
    assert_eq!(cardinalities(&path), saved_counts);
    assert_eq!(ready(runtime.get_run(run.id)), Some(after.clone()));
    assert_eq!(
        open_reader(&path).unwrap().get_run(&after.run.id).unwrap(),
        Some(after)
    );
}

#[test]
fn reconstruction_counters_do_not_become_run_counts() {
    use reconstruction as r;
    let mut player = r::player();
    player.status.stats.retain(|stat| stat.statTypeNum != 128);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(dir.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let mut adapter = Adapter::open(
        path.clone(),
        Arc::new(r::catalog()),
        EngineConfig {
            stream_id: "synthetic-c1-reconstruction".into(),
            identity_key: [7; 32],
            ..Default::default()
        },
        WriterConfig {
            stream_id: "synthetic-c1-reconstruction".into(),
            ..Default::default()
        },
        false,
        runtime.shared.views.clone(),
        Bridge::default(),
    )
    .unwrap();
    for (at, packet) in [
        (0, r::map("The Void")),
        (1000, r::create(1, 1)),
        (
            2000,
            r::update(vec![player, r::object(2, 300, vec![])], vec![]),
        ),
        (3000, r::shoot(1)),
        (4000, r::hit(1)),
    ] {
        adapter.decoded(decoded(at * 100, packet)).unwrap();
    }
    adapter.tick(Timestamp(450_000)).unwrap();
    flush(&mut adapter);
    let run = ready(runtime.list_runs(filter())).rows.remove(0);
    let before = ready(runtime.get_run(run.id.clone())).unwrap();
    let fight_id = before.fights[0].id.clone();
    let estimated = ready(runtime.get_fight(fight_id.clone())).unwrap();
    assert_eq!(estimated.total_damage, 100.0);
    assert!(estimated.players[0].sources[0].estimated);
    assert!(!estimated.players[0].sources[0].reasons.is_empty());
    // Known invulnerability reconstructs an observed zero; an absent shot is
    // an omission, not another zero hit or another run.
    for (at, packet) in [
        (
            5000,
            r::tick(vec![r::status(2, vec![r::stat(29, 0x1000000)])]),
        ),
        (6000, r::shoot(2)),
        (7000, r::hit(2)),
        (8000, r::hit(99)),
    ] {
        adapter.decoded(decoded(at * 100, packet)).unwrap();
    }
    adapter.tick(Timestamp(850_000)).unwrap();
    flush(&mut adapter);
    let counters = adapter.engine.reconstruction_coverage();
    assert_eq!(counters["attemptedHits"], 3);
    assert_eq!(counters["reconstructedHits"], 2);
    assert_eq!(counters["reconstructedZero"], 1);
    assert_eq!(counters["omittedHits"], 1);
    assert_eq!(counters["missingProjectile"], 1);
    // DEFENSE is absent on both hits; primary condition is absent on the first.
    assert_eq!(counters["assumedTargetStatDefault"], 3);
    assert_eq!(counters["assumedCrucibleDefault"], 2);
    let prior = ready(runtime.get_fight(fight_id.clone())).unwrap();
    assert!(prior.players[0]
        .sources
        .iter()
        .any(|s| s.estimated && s.damage == 100.0 && s.hits == 1));
    // Reconstruction diagnostics count zero and omitted attempts; the existing
    // durable source projection records positive damage contributions only.
    assert_eq!(prior.players[0].sources, estimated.players[0].sources);
    adapter
        .decoded(decoded(900_000, r::damage(1, 1, 321)))
        .unwrap();
    adapter.tick(Timestamp(1_200_000)).unwrap();
    flush(&mut adapter);
    let reconciled = ready(runtime.get_fight(fight_id.clone())).unwrap();
    assert_eq!(adapter.engine.reconstruction_coverage(), counters);
    assert_eq!(reconciled.total_damage, 321.0);
    assert_eq!(
        reconciled.players[0]
            .sources
            .iter()
            .map(|s| s.damage)
            .sum::<f64>(),
        321.0
    );
    assert!(reconciled.players[0]
        .sources
        .iter()
        .any(|s| !s.estimated && s.damage == 321.0));
    assert!(prior.players[0]
        .sources
        .iter()
        .any(|s| s.estimated && s.damage == 100.0));
    assert!(!reconciled.players[0]
        .sources
        .iter()
        .any(|s| s.estimated && s.damage > 0.0));
    assert!(reconciled.players[0]
        .sources
        .iter()
        .all(|s| !s.estimated && s.reasons.is_empty()));
    let mut issues = prior.issues.clone();
    issues.push(CoverageIssue::AmbiguousHit); // Server amount disagrees with reconstruction.
    assert_eq!(reconciled.issues, issues);
    assert_eq!(reconciled.complete_capture, prior.complete_capture);
    adapter
        .finish(Timestamp(1_300_000), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    assert_eq!(adapter.engine.reconstruction_coverage(), counters);
    assert_durable(&runtime);
    let page = ready(runtime.list_runs(filter()));
    assert_eq!(page.rows.len(), 1);
    let story = ready(runtime.get_run(run.id)).unwrap();
    let stored = ready(runtime.get_fight(fight_id)).unwrap();
    assert_eq!(story.fights.len(), 1);
    assert_eq!(
        story.fights[0].estimated,
        stored
            .players
            .iter()
            .flat_map(|p| &p.sources)
            .any(|s| s.estimated)
    );
    let serialized = serde_json::to_string(&(&story, &stored)).unwrap();
    for name in counters.keys() {
        assert!(
            !serialized.contains(name),
            "lifetime counter leaked into run JSON: {name}"
        );
    }
    assert_eq!(
        open_reader(&path).unwrap().get_fight(&stored.id).unwrap(),
        Some(stored)
    );
}

#[test]
fn replay_drop_summary_ready_matches_feed_and_ended_runs() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let mut options = RuntimeOptions::live(dir.path().into(), None);
    options.namespace = Namespace::Demo;
    runtime
        .replay_reader(
            options,
            Cursor::new(std::fs::read(fixture("demo.pcap")).unwrap()),
            Bridge::default(),
        )
        .unwrap();
    let filter = DropFilter {
        min_rank: None,
        place: None,
        item_type: None,
        enchanted_only: false,
        range: None,
        cursor: None,
        limit: MAX_LOOT_PAGE,
    };
    let drops = ready(runtime.list_drops(filter.clone()));
    assert!(!drops.rows.is_empty());
    let summary = ready(runtime.get_drop_summary(
        DropSummaryRequest {
            include_items: true,
            filter,
            days: vec![TimeRange {
                start: Timestamp(0),
                end: Timestamp(i64::MAX as u64),
            }],
        },
        vec![],
    ));
    assert_eq!(summary.totals.bags as usize, drops.rows.len());
    assert_eq!(
        summary.totals.whites as usize,
        drops
            .rows
            .iter()
            .filter(|r| r.drop.bag_rank == Some(255))
            .count()
    );
    assert_eq!(
        summary.totals.first_at,
        drops.rows.iter().map(|r| r.drop.at).min()
    );
    assert_eq!(summary.totals.runs_ended, Some(1));
    assert_eq!(summary.days, [summary.totals]);
    runtime.stop().unwrap();
}

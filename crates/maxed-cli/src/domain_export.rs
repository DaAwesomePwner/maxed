//! Passive, deterministic domain projection. Opaque engine identities stay internal.
use crate::{domain_compare::DomainError, CliError, Pipeline, ReplayReport};
use maxed_engine::{Engine, EngineConfig};
use maxed_protocol::{canonical::lower_hex, DecodeEvent, Diagnostic, Packet};
use maxed_recording::PcapReader;
use maxed_types::{
    CaptureScope, Catalog, EngineEvent, Evidence, FightEnd, FightView, RunView, Timestamp,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Write},
    path::Path,
    sync::Arc,
};

/// Catalog-root provenance covers exact bytes, never domain-number rounding.
pub(crate) fn catalog_sha256(bytes: &[u8]) -> String {
    lower_hex(&Sha256::digest(bytes))
}

/// Compact recursively sorted object keys, preserving Catalog number precision.
pub(crate) fn canonical_catalog_bytes(catalog: &Catalog) -> Result<Vec<u8>, DomainError> {
    let mut value = serde_json::to_value(catalog).map_err(|_| DomainError::Catalog)?;
    value.sort_all_objects();
    serde_json::to_vec(&value).map_err(|_| DomainError::Catalog)
}

/// Round the exact binary f64 value to six significant digits, half-to-even,
/// using the canonical packet formatting method without narrowing to its f32 input type.
pub fn six_significant(value: f64) -> Result<f64, DomainError> {
    if !value.is_finite() {
        return Err(DomainError::NonFinite);
    }
    if value == 0.0 {
        return Ok(0.0);
    }
    let rounded: f64 = format!("{value:.5e}")
        .parse()
        .map_err(|_| DomainError::NonFinite)?;
    if !rounded.is_finite() {
        return Err(DomainError::NonFinite);
    }
    Ok(rounded)
}

fn number(n: f64) -> Result<Value, DomainError> {
    Ok(json!(six_significant(n)?))
}
fn optional(n: Option<f64>) -> Result<Value, DomainError> {
    n.map(number).transpose().map(|n| n.unwrap_or(Value::Null))
}
fn label<T: Serialize>(value: &T) -> String {
    // Only infallibly serializable unit enums are passed here.
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[derive(Default)]
struct Projection {
    maps: Vec<String>,
    current: Option<usize>,
    pending_map: Option<usize>,
    fights: BTreeMap<String, (usize, FightView)>,
    runs: BTreeMap<String, (usize, RunView)>,
    drops: BTreeMap<String, Option<String>>,
    issues: BTreeMap<String, u64>,
    unmapped_fights: BTreeSet<String>,
}
impl Projection {
    fn consume(&mut self, events: Vec<EngineEvent>) -> Result<(), DomainError> {
        for event in events {
            match event {
                EngineEvent::MapEntered { name, .. } => {
                    // The verified MAPINFO reserves the ordinal before apply().
                    let map = self.pending_map.take().ok_or(DomainError::Projection)?;
                    self.maps[map] = name;
                    self.current = Some(map);
                }
                EngineEvent::FightStarted(f)
                | EngineEvent::FightUpdated(f)
                | EngineEvent::FightEnded(f) => {
                    if self.unmapped_fights.contains(&f.id) {
                        continue;
                    } else if let Some(entry) = self.fights.get_mut(&f.id) {
                        entry.1 = f;
                    } else if let Some(map) = self.current {
                        self.fights.insert(f.id.clone(), (map, f));
                    } else {
                        // A midstream report has no verified map ordinal. Do not invent one.
                        self.unmapped_fights.insert(f.id);
                    }
                }
                EngineEvent::RunStarted(r)
                | EngineEvent::RunUpdated(r)
                | EngineEvent::RunEnded(r) => {
                    if let Some(entry) = self.runs.get_mut(&r.id) {
                        entry.1 = r;
                    } else {
                        let map = self.current.ok_or(DomainError::Projection)?;
                        self.runs.insert(r.id.clone(), (map, r));
                    }
                }
                EngineEvent::BagDropped(d) | EngineEvent::DropUpdated(d) => {
                    // Legacy null scope retains v1 behavior; extended capture is
                    // excluded even when a later revision changes the scope.
                    if d.capture_scope == Some(CaptureScope::Extended) {
                        self.drops.remove(&d.id);
                    } else {
                        self.drops.insert(d.id, d.run_id);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn records(&mut self) -> Result<Vec<Value>, DomainError> {
        self.issues.clear();
        let mut fights = Vec::new();
        let mut encounters = Vec::new();
        let mut runs = Vec::new();
        for (map, name) in self.maps.iter().enumerate() {
            let mut targets: Vec<_> = self.fights.values().filter(|f| f.0 == map).collect();
            // The engine's first-hit clock includes zero-amount reports. Targets
            // first hit in the same tick sort by object ID, irrespective of receipt.
            targets.sort_by_key(|(_, f)| (f.first_hit, f.target_id, f.target_generation));
            for (ordinal, (_, fight)) in targets.iter().enumerate() {
                for issue in &fight.issues {
                    *self.issues.entry(label(issue)).or_default() += 1;
                }
                fights.push(json!({"kind":"fight", "key":format!("{map}/{ordinal}"),
                    "value":fight_value(name, ordinal, fight)?}));
            }
            if !targets.is_empty() {
                let first = targets.iter().filter_map(|f| f.1.first_hit).min();
                let last = targets.iter().filter_map(|f| f.1.last_hit).max();
                let damage: f64 = targets.iter().map(|f| f.1.total_damage).sum();
                let unattributed: f64 = targets.iter().map(|f| f.1.unattributed_damage).sum();
                let dps = first
                    .zip(last)
                    .and_then(|(a, b)| maxed_engine::dps(damage, a, b));
                encounters.push(json!({"kind":"encounter", "key":map.to_string(), "value":{
                    "map":name, "firstHit":first.map(|t| t.0), "lastHit":last.map(|t| t.0),
                    "damage":number(damage)?, "unattributed":number(unattributed)?,
                    "dps":optional(dps)?, "targets":targets.len()}}));
            }
            for (_, run) in self.runs.values().filter(|r| r.0 == map) {
                for issue in &run.issues {
                    *self.issues.entry(label(issue)).or_default() += 1;
                }
                let mut evidence: Vec<_> = run.evidence.iter().map(evidence_value).collect();
                evidence.sort_by(|a, b| {
                    a["at"]
                        .as_u64()
                        .cmp(&b["at"].as_u64())
                        .then_with(|| a["kind"].as_str().cmp(&b["kind"].as_str()))
                });
                runs.push(json!({"kind":"run", "key":map.to_string(), "value":{
                    "map":name, "started":run.started.0, "ended":run.ended.map(|t| t.0),
                    "completedAt":run.completed_at.map(|t| t.0), "diedAt":run.died_at.map(|t| t.0),
                    "outcome":run.outcome, "evidence":evidence, "completeCapture":run.complete_capture,
                    "fightCount":targets.iter().filter(|f| f.1.run_id.as_ref() == Some(&run.id)).count(),
                    "dropCount":self.drops.values().filter(|r| r.as_ref() == Some(&run.id)).count()}}));
            }
        }
        fights.extend(encounters);
        fights.extend(runs);
        Ok(fights)
    }
}

fn evidence_value(e: &maxed_types::EvidenceAt) -> Value {
    let (kind, rule, before, after, delay) = match &e.evidence {
        Evidence::Victory => ("victory", None, None, None, None),
        Evidence::CompletionDialogue { rule } => {
            ("completion_dialogue", Some(rule), None, None, None)
        }
        Evidence::CompletionCounter {
            before,
            after,
            delay_micros,
        } => (
            "completion_counter",
            None,
            Some(before),
            Some(after),
            Some(delay_micros),
        ),
        Evidence::DeathPacket => ("death_packet", None, None, None, None),
        Evidence::ObservedExit => ("observed_exit", None, None, None, None),
        Evidence::CaptureGap => ("capture_gap", None, None, None, None),
        Evidence::AppClosed => ("app_closed", None, None, None, None),
    };
    json!({"at":e.at.0,"kind":kind,"rule":rule,"before":before,"after":after,"delayMicros":delay})
}

fn fight_value(map: &str, ordinal: usize, f: &FightView) -> Result<Value, DomainError> {
    // FightView seeds a local zero row for the UI. Domain players represent
    // contributors, like DomainJson's grouping of the reference damage list.
    let mut players: Vec<_> = f.players.iter().filter(|p| p.hits > 0).collect();
    players.sort_by_key(|p| p.object_id);
    let players: Result<Vec<_>, DomainError> = players
        .into_iter()
        .map(|p| {
            let mut sources: Vec<_> = p.sources.iter().collect();
            sources.sort_by_cached_key(|s| {
                (
                    label(&s.kind),
                    s.item_type,
                    label(&s.enchant.state),
                    s.enchant.ids.clone(),
                )
            });
            let sources: Result<Vec<_>, DomainError> = sources
                .into_iter()
                .map(|s| {
                    Ok(json!({
            "kind":s.kind, "itemType":s.item_type, "enchantState":s.enchant.state,
            "enchantIds":s.enchant.ids, "damage":number(s.damage)?, "hits":s.hits,
            "biggestHit":optional(s.biggest_hit)?, "estimated":s.estimated}))
                })
                .collect();
            Ok(
                json!({"objectId":p.object_id, "local":p.is_local, "damage":number(p.damage)?,
            "hits":p.hits, "biggestHit":optional(p.biggest_hit)?, "dps":optional(f.first_hit.zip(f.last_hit).and_then(|(first, last)| maxed_engine::dps(p.damage, first, last)))?,
            "share":optional(p.share)?, "sources":sources?}),
            )
        })
        .collect();
    Ok(
        json!({"map":map, "targetType":f.target_type, "targetOrdinal":ordinal,
        "firstHit":f.first_hit.map(|t| t.0), "lastHit":f.last_hit.map(|t| t.0),
        "damage":number(f.total_damage)?, "unattributed":number(f.unattributed_damage)?,
        "completeCapture":f.complete_capture, "players":players?}),
    )
}

fn consume(
    engine: &mut Engine,
    projection: &mut Projection,
    report: &mut ReplayReport,
    events: Vec<DecodeEvent>,
    observed: bool,
) -> Result<(), CliError> {
    for event in events {
        report.observe(&event);
        let events = match event {
            DecodeEvent::Packet(p) if p.alignment_verified => {
                if let Packet::MapInfo(map) = &p.packet {
                    projection.pending_map = Some(projection.maps.len());
                    projection.maps.push(map.name.clone());
                }
                if observed {
                    engine.apply_observed(p)
                } else {
                    engine.apply(p)
                }
            }
            DecodeEvent::Packet(p) => {
                engine.diagnostic(p.conn, p.dir, p.at, Diagnostic::Recovering)
            }
            DecodeEvent::Diagnostic {
                conn,
                dir,
                at,
                kind,
            } => engine.diagnostic(conn, dir, at, kind),
        };
        projection.consume(events)?;
        // A retired connection can carry a verified MAPINFO that consumes an
        // ordinal but is deliberately ignored by the engine's active world.
        projection.pending_map = None;
    }
    Ok(())
}

// Hash the bytes delivered to PcapReader, never a separate pre-pass.
struct HashingReader<'a> {
    file: File,
    hash: &'a mut Sha256,
}
impl Read for HashingReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let n = self.file.read(buffer)?;
        self.hash.update(&buffer[..n]);
        Ok(n)
    }
}

/// Export to two CREATE_NEW files. Failure removes only files this invocation created.
/// No packet values, account identities, stream IDs or local paths are serialized.
pub fn export_domain(input: &Path, catalog: &Path, output: &Path) -> Result<(), CliError> {
    let output = crate::record::private_output_path(output)?;
    let output = output.as_path();
    let meta_path = crate::record::private_output_path(&crate::decode::metadata_path(output))?;
    let catalog_file = File::open(catalog)?;
    let mut bytes = Vec::new();
    catalog_file
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(DomainError::Catalog.into());
    }
    let catalog_json: Value = serde_json::from_slice(&bytes).map_err(|_| DomainError::Catalog)?;
    // Provenance covers the exact input file, including whitespace and key order.
    // Catalog-root writers canonicalize before writing; readers never reserialize.
    let catalog_hash = catalog_sha256(&bytes);
    let catalog: Catalog =
        serde_json::from_value(catalog_json).map_err(|_| DomainError::Catalog)?;
    let mut engine = Engine::new(EngineConfig::default(), Arc::new(catalog))
        .map_err(|_| DomainError::Catalog)?;
    let mut hasher = Sha256::new();
    let mut reader = PcapReader::new(BufReader::new(HashingReader {
        file: File::open(input)?,
        hash: &mut hasher,
    }))?;
    let mut pipeline = Pipeline::default();
    let mut projection = Projection::default();
    let mut report = ReplayReport::default();
    let mut at = Timestamp(0);
    while let Some(frame) = reader.next_frame()? {
        at = Timestamp(at.0.max(frame.at.0));
        report.frames += 1;
        let frame_at = frame.at;
        let output = pipeline.push_frame(frame);
        if output.trusted_traffic {
            projection.consume(engine.observe_traffic(frame_at))?;
        }
        consume(
            &mut engine,
            &mut projection,
            &mut report,
            output.events,
            true,
        )?;
    }
    consume(
        &mut engine,
        &mut projection,
        &mut report,
        pipeline.finish(),
        false,
    )?;
    projection.consume(engine.finish(at, FightEnd::CaptureStopped))?;
    drop(reader);
    let input_hash = lower_hex(&hasher.finalize());
    let records = projection.records()?;
    let header = json!({"schema":"maxed-domain-v1", "inputSha256":input_hash,
        "catalogSha256":catalog_hash, "clock":"tick-receipt-ms-v1"});
    let metadata = json!({"inputSha256":input_hash, "catalogSha256":catalog_hash,
        "fieldFailures":report.field_failures, "trailingBytePackets":report.trailing_byte_packets,
        "unknownIdPackets":report.unknown_id_packets, "wrongDirectionPackets":report.wrong_direction_packets,
        "unverifiedPackets":report.unverified_packets, "truncated":false,
        "decodeFailures":report.failures, "unsupportedFrames":pipeline.unsupported_frames(),
        "unmappedFights":projection.unmapped_fights.len(), "issues":projection.issues,
        "reconstruction":engine.reconstruction_coverage()});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut meta_owned = false;
    let result = (|| -> Result<(), CliError> {
        let mut meta = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&meta_path)?;
        meta_owned = true;
        for line in std::iter::once(&header).chain(records.iter()) {
            serde_json::to_writer(&mut file, line).map_err(std::io::Error::other)?;
            file.write_all(b"\n")?;
        }
        serde_json::to_writer(&mut meta, &metadata).map_err(std::io::Error::other)?;
        meta.write_all(b"\n")?;
        meta.sync_all()?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(output);
        if meta_owned {
            let _ = fs::remove_file(meta_path);
        }
    }
    if result.is_ok() {
        for (reason, count) in engine.reconstruction_coverage() {
            eprintln!("coverage=reconstruction.{reason} count={count}");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::{EventEnvelope, EvidenceAt, Outcome};

    #[test]
    fn hashing_reader_tracks_mutated_bytes_as_they_are_read() {
        use std::io::{Seek, SeekFrom};
        struct RemoveFile(std::path::PathBuf);
        impl Drop for RemoveFile {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let path = std::env::temp_dir().join(format!("maxed-domain-hash-{}", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        // Install the guard only after CREATE_NEW succeeds; never remove another file.
        let cleanup = RemoveFile(path.clone());
        // Shadow the handle so it closes before cleanup during unwinding (Windows).
        let mut file = file;
        file.write_all(b"old-prefix-old-tail").unwrap();
        let mut hash = Sha256::new();
        let mut reader = HashingReader {
            file: File::open(&path).unwrap(),
            hash: &mut hash,
        };
        let mut observed = vec![0; 11];
        reader.read_exact(&mut observed).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"new-prefix-new-tail").unwrap();
        reader.read_to_end(&mut observed).unwrap();
        drop(reader);
        assert_eq!(observed, b"old-prefix-new-tail");
        assert_eq!(hash.finalize(), Sha256::digest(&observed));
        drop(file);
        drop(cleanup);
    }

    fn events(stream: &str) -> Vec<EventEnvelope> {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scrubbed/maxed-demo");
        let catalog =
            serde_json::from_slice(&fs::read(root.join("catalog.json")).unwrap()).unwrap();
        crate::demo::replay_engine(
            &root.join("story.pcap"),
            Arc::new(catalog),
            EngineConfig {
                stream_id: stream.into(),
                identity_key: [7; 32],
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn project(events: Vec<EventEnvelope>) -> Projection {
        let mut projection = Projection::default();
        for envelope in events {
            if let EngineEvent::MapEntered { ref name, .. } = envelope.event {
                projection.pending_map = Some(projection.maps.len());
                projection.maps.push(name.clone());
            }
            projection.consume(vec![envelope.event]).unwrap();
        }
        projection
    }

    #[test]
    fn domain_export_excludes_extended_bags_from_drop_count() {
        use maxed_types::{demo::demo_snapshot, CaptureScope};
        let snapshot = demo_snapshot();
        let mut projection = project(events("scope-test"));
        let run_id = projection.runs.keys().next().unwrap().clone();
        projection.drops.clear();
        for (index, scope) in [
            None,
            Some(CaptureScope::RealmShark),
            Some(CaptureScope::Extended),
        ]
        .into_iter()
        .enumerate()
        {
            let mut drop = snapshot.drops[0].clone();
            drop.id = format!("synthetic-drop-{index}");
            drop.run_id = Some(run_id.clone());
            drop.capture_scope = scope;
            projection
                .consume(vec![
                    EngineEvent::BagDropped(drop.clone()),
                    EngineEvent::DropUpdated(drop),
                ])
                .unwrap();
        }
        let rows = projection.records().unwrap();
        let run = rows.iter().find(|r| r["kind"] == "run").unwrap();
        assert_eq!(run["value"]["dropCount"], 2);

        // A later revision must replace scope as well as the run assignment.
        let mut revised = snapshot.drops[0].clone();
        revised.id = "synthetic-drop-0".into();
        revised.run_id = Some(run_id);
        revised.capture_scope = Some(CaptureScope::Extended);
        projection
            .consume(vec![EngineEvent::DropUpdated(revised)])
            .unwrap();
        let rows = projection.records().unwrap();
        assert_eq!(
            rows.iter().find(|r| r["kind"] == "run").unwrap()["value"]["dropCount"],
            1
        );
    }

    #[test]
    fn domain_export_ignores_instance_events() {
        use maxed_types::{InstanceEventKind, InstanceEventView};
        let mut projection = project(events("instance-test"));
        let before = projection.records().unwrap();
        let run_id = projection.runs.keys().next().cloned();
        for kind in [
            InstanceEventKind::PlayerDeath {
                player_id: None,
                player_name: "SynthAlpha".into(),
                level: Some(20),
                killed_by: Some("Synth Enemy".into()),
                grave_type: None,
                object_id: Some(123),
                class_id: None,
                is_local: false,
            },
            InstanceEventKind::PlayerLeft {
                object_id: 123,
                player_name: Some("SynthAlpha".into()),
                class_id: None,
            },
            InstanceEventKind::PortalOpened {
                opener_is_local: None,
                opener: Some("SynthBravo".into()),
                portal_type: Some(456),
                dungeon: Some("Synth Cave".into()),
            },
            InstanceEventKind::RealmEventKilled {
                boss_key: "Synth Boss".into(),
                object_type: Some(789),
                killer: Some("SynthBravo".into()),
            },
        ] {
            projection
                .consume(vec![EngineEvent::InstanceEvent(InstanceEventView {
                    id: "synthetic-instance-event".into(),
                    session_id: "synthetic-session".into(),
                    run_id: run_id.clone(),
                    map: Some("Synth Cave".into()),
                    at: Timestamp(42),
                    kind,
                    provisional: true,
                })])
                .unwrap();
        }
        assert_eq!(projection.records().unwrap(), before);
        let text = serde_json::to_string(&before).unwrap();
        assert!(!text.contains("SynthAlpha"));
        assert!(!text.contains("SynthBravo"));
    }

    #[test]
    fn damage_totals_use_float_canonicalization_even_when_integral() {
        // Domain damage is f64; Java must widen integer accumulators before
        // calling CanonicalJson.scalar, while hits and timestamps stay integers.
        assert_eq!(number(1_234_567.).unwrap(), json!(1_234_570.));
        assert_eq!(number(2_345_685.).unwrap(), json!(2_345_680.));
    }

    #[test]
    fn unknown_outcome_and_capture_evidence_remain_available_in_export() {
        let mut projection = project(events("synthetic"));
        let mut run = projection.runs.values().next().unwrap().1.clone();
        run.outcome = Some(Outcome::Unknown);
        run.completed_at = None;
        run.died_at = None;
        run.ended = Some(Timestamp(3000));
        run.complete_capture = false;
        run.evidence = vec![
            EvidenceAt {
                at: Timestamp(2000),
                evidence: Evidence::CaptureGap,
            },
            EvidenceAt {
                at: Timestamp(3000),
                evidence: Evidence::ObservedExit,
            },
        ];
        projection
            .consume(vec![EngineEvent::RunUpdated(run)])
            .unwrap();
        let records = projection.records().unwrap();
        let run = &records.last().unwrap()["value"];
        assert_eq!(run["outcome"], "unknown");
        assert_eq!(run["ended"], 3000);
        assert_eq!(run["evidence"][0]["kind"], "capture_gap");
        assert_eq!(run["evidence"][1]["kind"], "observed_exit");
    }

    #[test]
    fn review1_export_uses_reference_window_independent_of_product_dps() {
        let mut fight = maxed_types::demo::demo_snapshot().fights.remove(0);
        fight.first_hit = Some(Timestamp(1_000_000));
        fight.last_hit = Some(Timestamp(31_000_000));
        fight.players[0].damage = 2000.;
        for product in [Some(200.), None] {
            fight.players[0].dps = product;
            let value = fight_value("Synthetic", 0, &fight).unwrap();
            assert_eq!(value["players"][0]["dps"], json!(66.6667));
            assert_eq!(fight.players[0].dps, product);
        }
        fight.last_hit = Some(Timestamp(1_400_000));
        assert_eq!(
            fight_value("Synthetic", 0, &fight).unwrap()["players"][0]["dps"],
            json!(5000.)
        );
    }

    #[test]
    fn domain_contributors_omit_ui_zero_rows_and_keep_source_evidence() {
        let projection = project(events("synthetic"));
        let mut fight = projection.fights.values().next().unwrap().1.clone();
        let mut remote = fight.players.iter().find(|p| p.hits > 0).unwrap().clone();
        remote.object_id = 30;
        remote.is_local = false;
        remote.damage = 80.;
        remote.hits = 2;
        remote.biggest_hit = Some(50.);
        remote.dps = Some(40.);
        remote.share = Some(0.8);
        let mut source = remote.sources[0].clone();
        source.kind = maxed_types::SourceKind::Other;
        source.item_type = None;
        source.enchant = maxed_engine::parse_enchant(None);
        source.damage = 80.;
        source.hits = 2;
        source.biggest_hit = Some(50.);
        source.estimated = false;
        remote.sources = vec![source];
        let mut local = remote.clone();
        local.object_id = 10;
        local.is_local = true;
        local.damage = 0.;
        local.hits = 0;
        local.biggest_hit = None;
        local.dps = Some(0.);
        local.share = Some(0.);
        local.sources.clear();
        fight.players = vec![remote, local];
        fight.first_hit = Some(maxed_types::Timestamp(1_000_000));
        fight.last_hit = Some(maxed_types::Timestamp(3_000_000));
        fight.total_damage = 100.;
        fight.unattributed_damage = 20.;
        let value = fight_value("Synthetic", 0, &fight).unwrap();
        assert_eq!(value["players"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["players"][0],
            json!({
                "objectId":30, "local":false, "damage":80., "hits":2,
                "biggestHit":50., "dps":40., "share":0.8,
                "sources":[{"kind":"other", "itemType":null, "enchantState":"missing",
                    "enchantIds":[], "damage":80., "hits":2, "biggestHit":50., "estimated":false}]
            })
        );
        assert_eq!(value["damage"], 100.);
        assert_eq!(value["unattributed"], 20.);
        assert_eq!(fight.players.len(), 2); // UI contract remains intact.
        fight.players.remove(0);
        assert_eq!(
            fight_value("Synthetic", 0, &fight).unwrap()["players"],
            json!([])
        );
    }

    #[test]
    fn opaque_ids_and_stream_names_do_not_change_export_and_late_run_revisions_win() {
        let mut a = project(events("first-namespace"));
        let mut b = project(events("unrelated-namespace"));
        assert_eq!(a.records().unwrap(), b.records().unwrap());
        let mut run = a.runs.values().next().unwrap().1.clone();
        run.outcome = Some(Outcome::Died);
        run.died_at = Some(Timestamp(10_000_000));
        run.evidence.push(EvidenceAt {
            at: Timestamp(10_000_000),
            evidence: Evidence::DeathPacket,
        });
        run.evidence.push(EvidenceAt {
            at: Timestamp(13_000_000),
            evidence: Evidence::CompletionCounter {
                before: 3,
                after: 4,
                delay_micros: 1_000_000,
            },
        });
        a.consume(vec![EngineEvent::RunUpdated(run)]).unwrap();
        let records = a.records().unwrap();
        let run = records.last().unwrap();
        assert_eq!(run["key"], "2");
        assert_eq!(run["value"]["outcome"], "died");
        assert_eq!(run["value"]["evidence"][0]["kind"], "death_packet");
        assert_eq!(run["value"]["evidence"][1]["kind"], "victory");
        assert_eq!(run["value"]["evidence"][3]["kind"], "completion_counter");
        assert_eq!(run["value"]["evidence"][3]["before"], 3);
        assert_eq!(run["value"]["evidence"][3]["delayMicros"], 1_000_000);
    }

    #[test]
    fn real_damage_reports_in_one_tick_order_by_object_id_including_zero_damage() {
        use maxed_protocol::{
            defs::{Damage, ObjectData, ObjectStatusData, WorldPosData},
            DecodedPacket,
        };
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scrubbed/maxed-demo");
        let catalog =
            serde_json::from_slice(&fs::read(root.join("catalog.json")).unwrap()).unwrap();
        let mut engine = Engine::new(EngineConfig::default(), Arc::new(catalog)).unwrap();
        let mut reader =
            PcapReader::new(BufReader::new(File::open(root.join("story.pcap")).unwrap())).unwrap();
        let mut pipeline = Pipeline::default();
        let mut projection = Projection::default();
        let mut report = ReplayReport::default();
        while let Some(frame) = reader.next_frame().unwrap() {
            let at = frame.at;
            let batch = pipeline.push_frame(frame);
            if batch.trusted_traffic {
                projection.consume(engine.observe_traffic(at)).unwrap();
            }
            for mut event in batch.events {
                if let DecodeEvent::Packet(p) = &mut event {
                    if let Packet::Update(update) = &mut p.packet {
                        // The demo's Realm target is object 4. Add a lower-ID target.
                        if update.newObjects.iter().any(|o| o.status.objectId == 4) {
                            update.newObjects.push(ObjectData {
                                objectType: 258,
                                status: ObjectStatusData {
                                    objectId: 2,
                                    pos: WorldPosData { x: 0., y: 0. },
                                    stats: vec![],
                                },
                            });
                        }
                    }
                    if let Packet::Damage(damage) = &mut p.packet {
                        assert!(p.alignment_verified);
                        assert_eq!(damage.targetId, 4);
                        damage.damageAmount = 0;
                        let second = DecodedPacket {
                            conn: p.conn,
                            dir: p.dir,
                            at: Timestamp(p.at.0 + 10_000),
                            id: p.id,
                            len: p.len,
                            alignment_verified: true,
                            packet: Packet::Damage(Damage {
                                targetId: 2,
                                effects: vec![],
                                damageAmount: 50,
                                damageProperties: false,
                                bulletId: damage.bulletId,
                                objectId: damage.objectId,
                            }),
                        };
                        consume(&mut engine, &mut projection, &mut report, vec![event], true)
                            .unwrap();
                        let first = &engine.snapshot().fights[0];
                        assert_eq!(first.target_id, 4);
                        assert_eq!(first.total_damage, 0.);
                        assert_eq!(first.first_hit, Some(Timestamp(2_000_000)));
                        consume(
                            &mut engine,
                            &mut projection,
                            &mut report,
                            vec![DecodeEvent::Packet(second)],
                            true,
                        )
                        .unwrap();
                        assert!(engine
                            .snapshot()
                            .fights
                            .iter()
                            .all(|f| f.first_hit == Some(Timestamp(2_000_000))));
                        projection
                            .consume(engine.finish(Timestamp(2_040_000), FightEnd::CaptureStopped))
                            .unwrap();
                        let rows = projection.records().unwrap();
                        assert_eq!(rows[0]["key"], "1/0");
                        assert_eq!(rows[0]["value"]["damage"], 50.);
                        assert_eq!(rows[1]["key"], "1/1");
                        assert_eq!(rows[1]["value"]["damage"], 0.);
                        assert_eq!(rows[0]["value"]["firstHit"], rows[1]["value"]["firstHit"]);
                        return;
                    }
                }
                consume(&mut engine, &mut projection, &mut report, vec![event], true).unwrap();
            }
        }
        panic!("demo fixture did not emit its first DAMAGE report");
    }

    #[test]
    fn first_hit_ties_reentry_and_new_generations_define_ordinals() {
        let source = project(events("template"));
        let template = source.fights.values().next().unwrap().1.clone();
        let mut projection = Projection {
            maps: vec!["Realm".into()],
            current: Some(0),
            ..Default::default()
        };
        for (id, object, generation, first_hit, damage) in [
            ("late", 9, 1, 2000, 9.),
            ("tie-b", 2, 1, 1000, 0.),
            ("tie-a", 1, 1, 1000, 1.),
            ("new-generation", 2, 2, 1000, 2.),
        ] {
            let mut fight = template.clone();
            fight.id = id.into();
            fight.target_id = object;
            fight.target_generation = generation;
            fight.first_hit = Some(Timestamp(first_hit));
            fight.total_damage = damage;
            projection
                .consume(vec![EngineEvent::FightStarted(fight)])
                .unwrap();
        }
        let mut reentry = projection.fights["tie-b"].1.clone();
        reentry.total_damage = 20.;
        projection
            .consume(vec![EngineEvent::FightUpdated(reentry)])
            .unwrap();
        let rows = projection.records().unwrap();
        assert_eq!(rows.len(), 5);
        for (i, damage) in [1., 20., 2., 9.].into_iter().enumerate() {
            assert_eq!(rows[i]["key"], format!("0/{i}"));
            assert_eq!(rows[i]["value"]["targetOrdinal"], i);
            assert_eq!(rows[i]["value"]["damage"], damage);
        }
        assert_eq!(rows[4]["value"]["targets"], 4);
        assert_eq!(rows[4]["value"]["damage"], 32.);
    }

    #[test]
    fn midstream_target_is_not_reassigned_to_the_next_verified_map() {
        let source = project(events("template"));
        let template = source.fights.values().next().unwrap().1.clone();
        let mut projection = Projection::default();
        projection
            .consume(vec![EngineEvent::FightStarted(template.clone())])
            .unwrap();
        projection.maps.push("Realm".into());
        projection.current = Some(0);
        projection
            .consume(vec![EngineEvent::FightEnded(template)])
            .unwrap();
        assert!(projection.records().unwrap().is_empty());
        assert_eq!(projection.unmapped_fights.len(), 1);
    }

    #[test]
    fn source_snapshots_sort_by_the_declared_tuple_and_players_by_object_id() {
        use maxed_types::{EnchantState, SourceKind};
        let projection = project(events("template"));
        let mut fight = projection.fights.values().next().unwrap().1.clone();
        let mut player = fight.players[0].clone();
        let source = player.sources[0].clone();
        player.sources.clear();
        for (kind, item, state, ids) in [
            (
                SourceKind::Weapon,
                Some(256),
                EnchantState::Recorded,
                vec![1, 2],
            ),
            (
                SourceKind::Weapon,
                Some(256),
                EnchantState::Recorded,
                vec![1],
            ),
            (SourceKind::Weapon, Some(256), EnchantState::Invalid, vec![]),
            (SourceKind::Weapon, None, EnchantState::Missing, vec![]),
            (
                SourceKind::Ability,
                Some(999),
                EnchantState::Recorded,
                vec![],
            ),
        ] {
            let mut s = source.clone();
            s.kind = kind;
            s.item_type = item;
            s.enchant.state = state;
            s.enchant.ids = ids;
            player.sources.push(s);
        }
        player.object_id = 9;
        let mut other = player.clone();
        other.object_id = 2;
        other.sources.clear();
        fight.players = vec![player, other];
        let value = fight_value("Realm", 0, &fight).unwrap();
        assert_eq!(value["players"][0]["objectId"], 2);
        let sources = value["players"][1]["sources"].as_array().unwrap();
        assert_eq!(sources[0]["kind"], "ability");
        assert!(sources[1]["itemType"].is_null());
        assert_eq!(sources[2]["enchantState"], "invalid");
        assert_eq!(sources[3]["enchantIds"], json!([1]));
        assert_eq!(sources[4]["enchantIds"], json!([1, 2]));
    }
}

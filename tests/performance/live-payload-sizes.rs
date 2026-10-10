//! Fast synthetic clock replay for wire-size evidence, not process-memory evidence.
use maxed_app::bridge::payload::{presentation_event, SnapshotEncoder};
use maxed_engine::{perf, Engine, EngineConfig};
use maxed_protocol::{DecodedPacket, Packet};
use maxed_types::*;
use std::sync::Arc;

fn wire_bytes(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).unwrap().len().max(1024)
}

fn apply(engine: &mut Engine, at: u64, packet: Packet) -> Vec<EngineEvent> {
    engine.apply(DecodedPacket {
        conn: ConnId(1),
        dir: Direction::ServerToClient,
        at: Timestamp(at),
        id: 0,
        len: 0,
        alignment_verified: true,
        packet,
    })
}
fn main() {
    let mut engine = Engine::new(
        EngineConfig {
            stream_id: "perf-live".into(),
            ..Default::default()
        },
        Arc::new(perf::catalog()),
    )
    .unwrap();
    let mut sequence = 0;
    for packet in perf::seed() {
        sequence += apply(&mut engine, 1_000_000, packet).len() as u64;
    }
    let mut codec = SnapshotEncoder::default();
    engine.set_watermark(Sequence(Timestamp(sequence)));
    let initial = engine.live_snapshot();
    assert_eq!(engine.snapshot().fights.len(), 1536);
    assert_eq!(initial.drops.len(), 320);
    let initial_bytes = wire_bytes(&codec.encode(initial));
    let mut snapshots = Vec::new();
    let (mut event_count, mut old_bytes, mut new_bytes, mut old_peak, mut new_peak) =
        (0u64, 0u64, 0u64, 0usize, 0usize);
    for step in 0..2440 {
        for packet in perf::hits(step) {
            for event in apply(&mut engine, 1_000_000 + step * 250_000, packet) {
                sequence += 1;
                if matches!(
                    &event,
                    EngineEvent::FightStarted(_)
                        | EngineEvent::FightUpdated(_)
                        | EngineEvent::FightEnded(_)
                ) {
                    let envelope = EventEnvelope {
                        fight_chart_omitted: false,
                        stream_id: "perf-live".into(),
                        sequence: Sequence(Timestamp(sequence)),
                        at: Timestamp(1_000_000 + step * 250_000),
                        event,
                    };
                    let old = serde_json::to_vec(&envelope).unwrap().len();
                    let new = wire_bytes(&presentation_event(envelope));
                    event_count += 1;
                    old_bytes += old as u64;
                    new_bytes += new as u64;
                    old_peak = old_peak.max(old);
                    new_peak = new_peak.max(new);
                }
            }
        }
        engine.set_watermark(Sequence(Timestamp(sequence)));
        if step % 240 == 0 || step == 2439 {
            let full = engine.snapshot();
            let live = engine.live_snapshot();
            let count = live.fights.len();
            let changed = codec.encode(live);
            let bytes = wire_bytes(&changed);
            let unchanged = {
                let mut next = engine.live_snapshot();
                next.sequence = Sequence(Timestamp(sequence + 1));
                codec.encode(next)
            };
            snapshots.push(serde_json::json!({"seconds": step as f64 / 4., "engine_fights": full.fights.len(), "live_fights": count,
                "full_snapshot_bytes": serde_json::to_vec(&full).unwrap().len(), "delta_bytes": bytes,
                "unchanged_delta_bytes": wire_bytes(&unchanged)}));
        }
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"synthetic_clock_only":true,"initial_filtered_full_bytes":initial_bytes,
        "snapshots":snapshots,"fight_events":{"count":event_count,"before_total_bytes":old_bytes,"after_total_bytes":new_bytes,
        "before_peak_bytes":old_peak,"after_peak_bytes":new_peak,"before_mean_bytes":old_bytes as f64/event_count as f64,
        "after_mean_bytes":new_bytes as f64/event_count as f64}})).unwrap());
}

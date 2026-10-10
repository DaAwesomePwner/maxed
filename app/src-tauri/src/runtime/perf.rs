//! Explicit perf feature only; no command or injection path in normal builds.
use super::*;
use maxed_protocol::DecodedPacket;
use maxed_types::{ConnId, Direction};

const STREAM: &str = "live-perf";

fn busy_steps(before: bool, short_diagnostic: bool) -> u64 {
    if before && short_diagnostic {
        240
    } else {
        2440
    }
}

pub fn enabled() -> bool {
    matches!(
        std::env::var("MAXED_PERF_LIVE").as_deref(),
        Ok("before" | "after")
    )
}
pub fn before() -> bool {
    std::env::var("MAXED_PERF_LIVE").as_deref() == Ok("before")
}

fn writer_config() -> maxed_store::WriterConfig {
    maxed_store::WriterConfig {
        stream_id: STREAM.into(),
        ..Default::default()
    }
}

impl CaptureRuntime {
    pub fn performance_realm(self: &Arc<Self>, app: tauri::AppHandle) -> Result<(), String> {
        let root = app.path().app_data_dir().map_err(|_| WORKER_FAILED)?;
        std::fs::create_dir_all(&root).map_err(|_| WORKER_FAILED)?;
        let root = root.canonicalize().map_err(|_| WORKER_FAILED)?;
        let temporary = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| WORKER_FAILED)?;
        if !root.starts_with(temporary) {
            return Err("Perf requires an isolated temporary profile".into());
        }
        let publisher = self.publisher(app)?;
        let status = CaptureStatus {
            state: CaptureState::Live,
            ..Default::default()
        };
        *self.shared.status.lock().map_err(|_| WORKER_FAILED)? = status.clone();
        publisher.publish(Publication::Capture(status.clone()));
        let mut adapter = Adapter::open(
            root.join(format!("perf-live-{}.sqlite3", std::process::id())),
            Arc::new(maxed_engine::perf::catalog()),
            maxed_engine::EngineConfig {
                stream_id: STREAM.into(),
                ..Default::default()
            },
            writer_config(),
            false,
            self.shared.views.clone(),
            Self::domain_bridge(publisher.clone()),
        )?;
        adapter.live = true;
        adapter.capture(status)?;
        let feed = |adapter: &mut Adapter, at, packet| {
            adapter.decoded(PipelineEvent::Decoded(DecodeEvent::Packet(DecodedPacket {
                conn: ConnId(1),
                dir: Direction::ServerToClient,
                at: Timestamp(at),
                id: 0,
                len: 0,
                alignment_verified: true,
                packet,
            })))
        };
        let clock = Instant::now();
        for (index, packet) in maxed_engine::perf::seed().into_iter().enumerate() {
            feed(&mut adapter, 1_000_000, packet)?;
            adapter.publish_due(clock.elapsed());
            publisher.flush()?;
            // Give the initial stream query room to establish its baseline.
            // A seed is preparation, not an instantaneous 1,500-hit burst.
            thread::sleep(Duration::from_millis(if index < 4 { 100 } else { 10 }));
        }
        adapter.publish_snapshot();
        let check = adapter.engine.snapshot();
        if check.fights.len() != maxed_engine::perf::FIGHTS || check.drops.len() != 320 {
            return Err("Synthetic workload shape mismatch".into());
        }
        drop(check);
        let steps = busy_steps(
            before(),
            std::env::var("MAXED_PERF_SHORT").as_deref() == Ok("1"),
        );
        eprintln!(
            "PERF_LIVE busy fights=1536 players=24 bags=320 hits_per_second={} seconds={} utc={}",
            maxed_engine::perf::HITS_PER_SECOND,
            steps / 4,
            chrono::Utc::now().to_rfc3339()
        );
        let start = Instant::now();
        for step in 0..steps {
            for packet in maxed_engine::perf::hits(step) {
                feed(&mut adapter, 1_000_000 + step * 250_000, packet)?;
            }
            adapter.publish_due(clock.elapsed());
            publisher.flush()?;
            let due = Duration::from_millis((step + 1) * 250);
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                thread::sleep(wait);
            }
        }
        let left_at = steps * 250_000 + 2_000_000;
        feed(&mut adapter, left_at, maxed_engine::perf::leave())?;
        adapter.publish_snapshot();
        publisher.flush()?;
        eprintln!(
            "PERF_LIVE idle busy_wall_seconds={:.3} utc={}",
            start.elapsed().as_secs_f64(),
            chrono::Utc::now().to_rfc3339()
        );
        // Keep the live engine and writer alive during the measured Nexus idle.
        thread::sleep(Duration::from_secs(180));
        adapter.finish(Timestamp(left_at + 180_000_000), FightEnd::CaptureStopped)?;
        publisher.flush()?;
        eprintln!("PERF_LIVE complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_before_diagnostics_can_shorten_the_acceptance_schedule() {
        assert_eq!(busy_steps(true, true), 240);
        assert_eq!(busy_steps(true, false), 2440);
        assert_eq!(busy_steps(false, true), 2440);
        assert_eq!(busy_steps(false, false), 2440);
    }

    #[test]
    fn synthetic_capture_uses_the_same_engine_and_writer_namespace() {
        let directory = tempfile::tempdir().unwrap();
        let mut adapter = Adapter::open(
            directory.path().join("synthetic.sqlite3"),
            Arc::new(maxed_engine::perf::catalog()),
            maxed_engine::EngineConfig {
                stream_id: STREAM.into(),
                ..Default::default()
            },
            writer_config(),
            false,
            Arc::new(Views::default()),
            Bridge::default(),
        )
        .unwrap();
        adapter.live = true;
        adapter
            .capture(CaptureStatus {
                state: CaptureState::Live,
                ..Default::default()
            })
            .unwrap();
        for packet in maxed_engine::perf::seed() {
            adapter
                .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(DecodedPacket {
                    conn: ConnId(1),
                    dir: Direction::ServerToClient,
                    at: Timestamp(1_000_000),
                    id: 0,
                    len: 0,
                    alignment_verified: true,
                    packet,
                })))
                .unwrap();
        }
        let snapshot = adapter.engine.snapshot();
        assert_eq!(snapshot.fights.len(), 1536);
        assert_eq!(snapshot.drops.len(), 320);
        adapter
            .finish(Timestamp(2_000_000), FightEnd::CaptureStopped)
            .unwrap();
    }
}

//! Admitted synthetic resources and an interruptible, injectable replay clock.
use crossbeam_channel::Receiver;
use maxed_recording::PcapReader;
use maxed_types::Catalog;
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{path::BaseDirectory, Manager};

pub const STORY_RESOURCE: &str = "demo/story.pcap";
pub const CATALOG_RESOURCE: &str = "demo/catalog.json";

pub struct DemoResources {
    pub(crate) story: Vec<u8>,
    pub(crate) catalog: Arc<Catalog>,
}
impl DemoResources {
    pub fn load(story: &Path, catalog: &Path) -> Result<Self, String> {
        fn read(path: &Path) -> Result<Vec<u8>, String> {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|_| "Synthetic demo resources are missing")?
                .take(1_048_577)
                .read_to_end(&mut bytes)
                .map_err(|_| "Synthetic demo resources could not be read")?;
            if bytes.len() > 1_048_576 {
                return Err("Synthetic demo resource is unsupported".into());
            }
            Ok(bytes)
        }
        let story = read(story)?;
        let catalog = serde_json::from_slice(&read(catalog)?)
            .map_err(|_| "Synthetic demo catalog is unsupported")?;
        // Reject malformed/truncated or non-monotonic fixtures before replacing a source.
        let mut reader = PcapReader::new(Cursor::new(&story))
            .map_err(|_| "Synthetic demo format is unsupported")?;
        let mut last = None;
        while let Some(frame) = reader
            .next_frame()
            .map_err(|_| "Synthetic demo data is invalid")?
        {
            if last.is_some_and(|at| frame.at < at) {
                return Err("Synthetic demo timestamps are invalid".into());
            }
            last = Some(frame.at);
        }
        if last.is_none() {
            return Err("Synthetic demo has no frames".into());
        }
        Ok(Self {
            story,
            catalog: Arc::new(catalog),
        })
    }

    pub fn resolve(app: &tauri::AppHandle) -> Result<Self, String> {
        let resolve = |path| {
            app.path()
                .resolve(path, BaseDirectory::Resource)
                .map_err(|_| "Synthetic demo resources are unavailable".to_string())
        };
        let story = resolve(STORY_RESOURCE)?;
        let catalog = resolve(CATALOG_RESOURCE)?;
        #[cfg(debug_assertions)]
        if !story.exists() && !catalog.exists() {
            let root = Self::development_root();
            return Self::load(&root.join("story.pcap"), &root.join("catalog.json"));
        }
        Self::load(&story, &catalog)
    }

    pub fn development_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scrubbed/maxed-demo")
    }
}

/// Return false when cancelled. Offsets are relative to the first fixture frame;
/// implementations must remain interruptible by `stop` even during long gaps.
/// A successful wait advances `elapsed()` to at least the requested offset.
pub trait DemoClock: Send + Sync {
    fn wait_until(&self, offset: Duration, stop: &Receiver<()>) -> bool;
    fn elapsed(&self) -> Duration;
}
#[derive(Default)]
pub struct RealtimeClock(Mutex<Option<Instant>>);
impl DemoClock for RealtimeClock {
    fn wait_until(&self, offset: Duration, stop: &Receiver<()>) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_or_insert_with(Instant::now);
        matches!(
            stop.recv_timeout(offset.saturating_sub(self.elapsed())),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        )
    }
    fn elapsed(&self) -> Duration {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .map_or(Duration::ZERO, |start| start.elapsed())
    }
}

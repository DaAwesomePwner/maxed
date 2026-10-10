#![allow(dead_code)]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn submit_with_alert_context(
    writer: &maxed_store::StoreWriter,
    mut event: maxed_types::EventEnvelope,
    context: maxed_store::AlertContext,
) {
    // Full also means a transient worker-lock collision, even for an empty
    // queue. Match the store's own bounded submission helper.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match writer.try_submit_with_alert_context(event, context.clone()) {
            Ok(()) => return,
            Err(maxed_store::SubmitError::Full(returned))
                if std::time::Instant::now() < deadline =>
            {
                event = returned;
                std::thread::yield_now();
            }
            Err(error) => panic!("submit failed: {error:?}"),
        }
    }
}

pub struct PrivateDir(pub PathBuf);
impl PrivateDir {
    pub fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../recordings")
            .join(format!(
                "synthetic-b-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    pub fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let p = self.file(name);
        fs::write(&p, bytes).unwrap();
        p
    }
}
impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

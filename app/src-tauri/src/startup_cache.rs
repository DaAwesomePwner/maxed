//! Advisory startup routing receipt. Full cache validation still runs in the
//! background; this bounded check reads metadata and never hashes game assets.
use crate::assets_bridge::{local_path, resources_path};
use maxed_assets::CachedCatalog;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

const RECEIPT: &str = "startup-assets.json";
#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Stamp {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}
impl Stamp {
    pub(crate) fn read(path: &Path) -> Option<Self> {
        let path = local_path(path).ok()?;
        let metadata = fs::metadata(&path).ok()?;
        metadata.is_file().then_some(Self {
            path,
            bytes: metadata.len(),
            modified: metadata.modified().ok()?,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u8,
    source: Stamp,
    fingerprint: String,
    manifest: Stamp,
    catalog: Stamp,
}
fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
}

pub(crate) fn recorded(root: &Path, source: &Path) -> bool {
    (|| {
        let file = local_path(&root.join(RECEIPT)).ok()?;
        let mut bytes = Vec::new();
        fs::File::open(file)
            .ok()?
            .take(65537)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 65536 {
            return None;
        }
        let receipt: Receipt = serde_json::from_slice(&bytes).ok()?;
        if receipt.version != 1 || !valid_fingerprint(&receipt.fingerprint) {
            return None;
        }
        let source = resources_path(source).ok()?;
        let generation = root.join("assets/v1").join(&receipt.fingerprint);
        (Stamp::read(&source)? == receipt.source
            && Stamp::read(&generation.join("manifest.json"))? == receipt.manifest
            && Stamp::read(&generation.join("catalog.json"))? == receipt.catalog)
            .then_some(())
    })()
    .is_some()
}

/// Called only after successful extraction/cache validation and settings save.
/// A failed advisory write leaves startup conservative; it cannot fail capture.
pub(crate) fn record(root: &Path, before: Option<Stamp>, cache: &CachedCatalog) {
    let _ = (|| -> Option<()> {
        let source = before?;
        if Stamp::read(&source.path)? != source
            || !valid_fingerprint(&cache.catalog.client_fingerprint)
        {
            return None;
        }
        // Build normal local paths from app data; CachedCatalog.root may carry
        // Windows' canonical device prefix, which user-facing path checks reject.
        let generation = root
            .join("assets/v1")
            .join(&cache.catalog.client_fingerprint);
        let receipt = Receipt {
            version: 1,
            source,
            fingerprint: cache.catalog.client_fingerprint.clone(),
            manifest: Stamp::read(&generation.join("manifest.json"))?,
            catalog: Stamp::read(&generation.join("catalog.json"))?,
        };
        let root = local_path(root).ok()?;
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let staging = root.join(format!(
            "startup-assets-{}-{nonce}.pending",
            std::process::id()
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .ok()?;
        let result = (|| {
            file.write_all(&serde_json::to_vec(&receipt).ok()?).ok()?;
            file.sync_all().ok()?;
            drop(file);
            fs::rename(&staging, root.join(RECEIPT)).ok()
        })();
        if result.is_none() {
            let _ = fs::remove_file(staging);
        }
        result
    })();
}

//! Read-only file metadata; never opens SQLite or checkpoints its WAL.
use maxed_types::*;
use std::{io::ErrorKind, path::Path};

pub(crate) fn status(root: &Path, recording: RecordingStatus) -> StorageStatus {
    let physical = recording_bytes(&root.join("recordings"));
    let accounted = (recording.retained_bytes != 0
        || recording.state != RecordingState::Disabled
        || physical == Some(0))
    .then_some(StorageCount(u64::from(recording.retained_bytes)));
    StorageStatus {
        measured_at: crate::runtime::maintenance::now(),
        database: maxed_store::storage_usage::measure_database(&root.join("live.sqlite3")),
        backups: BackupStorage::default(),
        recordings: RecordingStorage {
            physical_bytes: physical.map(StorageCount),
            accounted_bytes: accounted,
            quota_bytes: Some(StorageCount(u64::from(recording.limit_bytes))),
        },
        receipt_estimate: StorageEstimate::default(),
        maintenance: None,
        failure: None,
    }
}

pub(crate) fn merge_logical(target: &mut DatabaseStorage, logical: DatabaseStorage) {
    if target.state != StorageDatabaseState::Present {
        return;
    }
    target.page_size = logical.page_size;
    target.page_count = logical.page_count;
    target.freelist_count = logical.freelist_count;
    target.allocated_bytes = logical.allocated_bytes;
    target.reusable_bytes = logical.reusable_bytes;
    target.auto_vacuum = logical.auto_vacuum;
}

/// Metadata only, with the recording owner's inventory bound. Unknown or
/// redirected entries make the observation unavailable; no raw file is opened.
fn recording_bytes(root: &Path) -> Option<u64> {
    match std::fs::symlink_metadata(root) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Some(0),
        Ok(meta) if meta.is_dir() && plain(&meta) => (),
        _ => return None,
    }
    #[cfg(windows)]
    let _guard = {
        use std::os::windows::fs::OpenOptionsExt;
        let guard = std::fs::OpenOptions::new()
            .access_mode(0)
            .share_mode(3)
            .custom_flags(0x0220_0000)
            .open(root)
            .ok()?;
        if !plain(&guard.metadata().ok()?) {
            return None;
        }
        guard
    };
    let mut total = 0_u64;
    for (index, entry) in std::fs::read_dir(root).ok()?.enumerate() {
        if index >= 4096 {
            return None;
        }
        let entry = entry.ok()?;
        let meta = std::fs::symlink_metadata(entry.path()).ok()?;
        if !meta.is_file() || !plain(&meta) {
            return None;
        }
        let name = entry.file_name();
        let name = name.to_str()?;
        if name != ".maxed-recording-v1.lease" {
            let path = entry.path();
            let stem = path.file_stem()?.to_str()?.strip_prefix("maxed-v2-")?;
            let (digits, random) = stem.split_once('-')?;
            if digits.len() != 20
                || digits.parse::<u64>().is_err()
                || !digits.bytes().all(|b| b.is_ascii_digit())
                || random.len() != 32
                || !random
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                || !matches!(
                    path.extension()?.to_str()?,
                    "pcap" | "partial" | "interrupted" | "owner"
                )
            {
                return None;
            }
        }
        total = total.checked_add(meta.len())?;
    }
    Some(total)
}

fn plain(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        !meta.is_symlink() && meta.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    {
        !meta.is_symlink()
    }
}

pub(crate) fn database_size(root: &Path) -> Result<u64, String> {
    let mut total = 0_u64;
    for name in ["live.sqlite3", "live.sqlite3-wal"] {
        let size = match std::fs::metadata(root.join(name)) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Err(error) if error.kind() == ErrorKind::NotFound => 0,
            _ => return Err("Database size unavailable.".into()),
        };
        total = total
            .checked_add(size)
            .ok_or("Database size unavailable.")?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_inventory_reports_physical_bytes_without_reading_recordings() {
        let root = tempfile::tempdir().unwrap();
        let recordings = root.path().join("recordings");
        assert_eq!(recording_bytes(&recordings), Some(0));
        std::fs::create_dir(&recordings).unwrap();
        let stem = "maxed-v2-00000000000000000001-0123456789abcdef0123456789abcdef";
        std::fs::write(
            recordings.join(format!("{stem}.pcap")),
            b"not a capture; metadata only",
        )
        .unwrap();
        std::fs::write(recordings.join(format!("{stem}.owner")), b"metadata").unwrap();
        assert_eq!(recording_bytes(&recordings), Some(36));
        std::fs::write(recordings.join("unexpected"), b"private").unwrap();
        assert_eq!(recording_bytes(&recordings), None);
        let mut absent = DatabaseStorage::default();
        merge_logical(
            &mut absent,
            DatabaseStorage {
                page_size: Some(4096),
                ..Default::default()
            },
        );
        assert_eq!(absent.page_size, None);
    }

    #[test]
    fn counts_only_live_database_and_wal_without_creating_files() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(database_size(root.path()).unwrap(), 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        for (name, size) in [
            ("live.sqlite3", 2048),
            ("live.sqlite3-wal", 1024),
            ("live.sqlite3-shm", 512),
            ("demo.sqlite3", 4096),
        ] {
            std::fs::write(root.path().join(name), vec![0; size]).unwrap();
        }
        assert_eq!(database_size(root.path()).unwrap(), 3072);
        std::fs::remove_file(root.path().join("live.sqlite3-wal")).unwrap();
        assert_eq!(database_size(root.path()).unwrap(), 2048);
    }

    #[test]
    fn unavailable_metadata_is_not_reported_as_zero() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("live.sqlite3")).unwrap();
        assert_eq!(
            database_size(root.path()),
            Err("Database size unavailable.".into())
        );
    }
}

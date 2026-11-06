//! Read-only storage observations; never opens the writer or performs migration.
use crate::{
    backup::BackupUsage,
    maintenance::{JobStatus, PauseReason, Phase},
    StoreReader,
};
use maxed_types::*;
use rusqlite::Connection;
use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

/// Bounded metadata/header measurement for the caller's live namespace. No table scans,
/// schema checks, migration, checkpoint or expensive estimates. Sample time is
/// supplied by the caller in StorageStatus. File lengths may race live writes.
pub fn measure_database(path: &Path) -> DatabaseStorage {
    let mut usage = DatabaseStorage::default();
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => {
            usage.state = StorageDatabaseState::Present;
            usage.main_bytes = Some(StorageCount(meta.len()));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            usage.state = StorageDatabaseState::Absent;
            return usage;
        }
        _ => return usage,
    }
    usage.wal_bytes = sidecar_bytes(path, "-wal");
    usage.shm_bytes = sidecar_bytes(path, "-shm");
    // READ_ONLY/query_only only protect the main database: opening SQLite can
    // create sidecars, recover the WAL index or checkpoint on close. NO_CKPT_ON_CLOSE
    // alone cannot prevent the first two. immutable=1 is unsafe for this live path:
    // it disables locking/change detection while a writer may change the files.
    // Never open SQLite here. With any WAL (even empty), SHM or rollback journal,
    // leave logical fields unknown rather than interpreting a possibly stale header.
    if !sidecars_absent(path) {
        return usage;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return usage;
    };
    let mut header = [0_u8; 100];
    if file.read_exact(&mut header).is_err() || !sidecars_absent(path) {
        return usage;
    }
    // https://sqlite.org/fileformat.html#the_database_header
    // Only accept a self-consistent, fully checkpointed main-file sample. The
    // sidecar checks bracket the read but do not make concurrent samples atomic.
    let count = |offset| {
        u32::from_be_bytes(
            header[offset..offset + 4]
                .try_into()
                .expect("header offset"),
        )
    };
    let page_size = match u16::from_be_bytes([header[16], header[17]]) {
        1 => 65536,
        n => u32::from(n),
    };
    let pages = count(28);
    let free = count(36);
    let allocated = page_bytes(Some(page_size), Some(u64::from(pages)));
    if &header[..16] != b"SQLite format 3\0"
        || !matches!(&header[18..20], [1, 1] | [2, 2])
        || header[21..24] != [64, 32, 32]
        || !(512..=65536).contains(&page_size)
        || !page_size.is_power_of_two()
        || page_size - u32::from(header[20]) < 480
        || pages == 0
        || count(24) != count(92)
        || free >= pages
        || count(52) > pages
        || allocated != usage.main_bytes
        || file.metadata().ok().map(|m| StorageCount(m.len())) != allocated
    {
        return usage;
    }
    usage.page_size = Some(page_size);
    usage.page_count = Some(StorageCount(u64::from(pages)));
    usage.freelist_count = Some(StorageCount(u64::from(free)));
    usage.allocated_bytes = allocated;
    usage.reusable_bytes = page_bytes(Some(page_size), Some(u64::from(free)));
    usage.auto_vacuum = Some(if count(52) == 0 {
        StorageAutoVacuum::None
    } else if count(64) == 0 {
        StorageAutoVacuum::Full
    } else {
        StorageAutoVacuum::Incremental
    });
    // Version 1 cannot distinguish DELETE/TRUNCATE/PERSIST/MEMORY/OFF.
    usage.journal_mode = (header[18] == 2).then_some(StorageJournalMode::Wal);
    usage
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn sidecars_absent(path: &Path) -> bool {
    ["-wal", "-shm", "-journal"].iter().all(|suffix| {
        matches!(fs::metadata(sidecar_path(path, suffix)), Err(e) if e.kind() == ErrorKind::NotFound)
    })
}

fn sidecar_bytes(path: &Path, suffix: &str) -> Option<StorageCount> {
    match fs::metadata(sidecar_path(path, suffix)) {
        Ok(meta) if meta.is_file() => Some(StorageCount(meta.len())),
        Err(error) if error.kind() == ErrorKind::NotFound => Some(StorageCount(0)),
        _ => None,
    }
}

/// Overflow is unknown, never saturated bytes or a wrapped disk-saving claim.
pub fn page_bytes(page_size: Option<u32>, pages: Option<u64>) -> Option<StorageCount> {
    u64::from(page_size?).checked_mul(pages?).map(StorageCount)
}

/// Potentially expensive: schedule on the history reader, never the cheap path.
/// SUM uses SQLite integers (overflow is an error), and BLOB lengths count bytes.
/// Missing/retired tables and failed reads are unknown, not inferred savings.
pub fn estimate_legacy_receipt_bytes(conn: &Connection, measured_at: Timestamp) -> StorageEstimate {
    let bytes: rusqlite::Result<i64> = conn.query_row(
        "SELECT coalesce(sum(length(CAST(stream_id AS BLOB)) +
         length(CAST(digest AS BLOB)) + 8), 0) FROM event_receipts",
        [],
        |r| r.get(0),
    );
    StorageEstimate {
        bytes: bytes
            .ok()
            .and_then(|n| u64::try_from(n).ok())
            .map(StorageCount),
        basis: StorageEstimateBasis::LegacyReceiptLogicalBytes,
        measured_at: Some(measured_at),
    }
}

impl StoreReader {
    pub fn maintenance_statuses(&self) -> Result<Vec<JobStatus>, crate::StoreError> {
        crate::maintenance::statuses(&self.conn)
    }

    /// Cheap logical counters from the already-owned history connection. Opening
    /// a connection just for these counters can create or recover WAL sidecars.
    pub fn logical_storage(&self) -> Result<DatabaseStorage, crate::StoreError> {
        let page_size: u32 = self
            .conn
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        let page_count = self
            .conn
            .pragma_query_value(None, "page_count", |r| crate::maintenance::sql_count(r, 0))?;
        let freelist_count = self.conn.pragma_query_value(None, "freelist_count", |r| {
            crate::maintenance::sql_count(r, 0)
        })?;
        let auto_vacuum = match self
            .conn
            .pragma_query_value(None, "auto_vacuum", |r| r.get::<_, u32>(0))?
        {
            0 => Some(StorageAutoVacuum::None),
            1 => Some(StorageAutoVacuum::Full),
            2 => Some(StorageAutoVacuum::Incremental),
            _ => None,
        };
        Ok(DatabaseStorage {
            state: StorageDatabaseState::Present,
            page_size: Some(page_size),
            page_count: Some(StorageCount(page_count)),
            freelist_count: Some(StorageCount(freelist_count)),
            allocated_bytes: page_bytes(Some(page_size), Some(page_count)),
            reusable_bytes: page_bytes(Some(page_size), Some(freelist_count)),
            auto_vacuum,
            ..Default::default()
        })
    }

    pub fn estimate_legacy_receipt_bytes(&self, measured_at: Timestamp) -> StorageEstimate {
        estimate_legacy_receipt_bytes(&self.conn, measured_at)
    }
}

/// Project already-observed job status; never performs SQL or exposes job keys.
/// Totals include all supplied jobs, even finished/cancelled ones, except superseded
/// snapshots. Callers must select the jobs they want counted as current progress.
pub fn maintenance_usage(jobs: &[JobStatus]) -> StorageMaintenance {
    let jobs: Vec<_> = jobs.iter().map(job_progress).collect();
    let totals = StorageProgressCounters {
        eligible_records: total(&jobs, |c| c.eligible_records),
        eligible_bytes: total(&jobs, |c| c.eligible_bytes),
        processed_records: total(&jobs, |c| c.processed_records),
        processed_bytes: total(&jobs, |c| c.processed_bytes),
        blocked_records: total(&jobs, |c| c.blocked_records),
        blocked_bytes: total(&jobs, |c| c.blocked_bytes),
    };
    let last_success_at = jobs
        .iter()
        .filter(|job| {
            job.phase == StorageMaintenancePhase::Reclaimable && !job.paused && job.reason.is_none()
        })
        .map(|job| job.updated_at)
        .max();
    StorageMaintenance {
        jobs,
        totals,
        last_success_at,
    }
}

fn total(
    jobs: &[StorageJobProgress],
    field: fn(&StorageProgressCounters) -> Option<StorageCount>,
) -> Option<StorageCount> {
    jobs.iter()
        .filter(|job| job.reason != Some(StorageMaintenanceReason::Superseded))
        .try_fold(0_u64, |sum, job| sum.checked_add(field(&job.counters)?.0))
        .map(StorageCount)
}

fn job_progress(job: &JobStatus) -> StorageJobProgress {
    StorageJobProgress {
        phase: match job.phase {
            Phase::AwaitingBackup => StorageMaintenancePhase::AwaitingBackup,
            Phase::CopyingReceipts => StorageMaintenancePhase::CopyingReceipts,
            Phase::Verifying => StorageMaintenancePhase::Verifying,
            Phase::Deleting => StorageMaintenancePhase::Deleting,
            Phase::Reclaimable => StorageMaintenancePhase::Reclaimable,
        },
        paused: job.paused,
        needs_resume: matches!(
            job.reason,
            Some(PauseReason::Cancelled | PauseReason::BlockedStream | PauseReason::BackupPoisoned)
        ),
        reason: job.reason.map(|reason| match reason {
            PauseReason::Cancelled => StorageMaintenanceReason::Cancelled,
            PauseReason::AwaitingBackup => StorageMaintenanceReason::AwaitingBackup,
            PauseReason::Capture => StorageMaintenanceReason::Capture,
            PauseReason::Budget => StorageMaintenanceReason::Budget,
            PauseReason::Wal => StorageMaintenanceReason::Wal,
            PauseReason::Busy => StorageMaintenanceReason::Busy,
            PauseReason::Full => StorageMaintenanceReason::Full,
            PauseReason::Io => StorageMaintenanceReason::Io,
            PauseReason::InvalidUnit => StorageMaintenanceReason::InvalidUnit,
            PauseReason::StoppedRequired => StorageMaintenanceReason::StoppedRequired,
            PauseReason::BlockedStream => StorageMaintenanceReason::BlockedStream,
            PauseReason::Superseded => StorageMaintenanceReason::Superseded,
            PauseReason::BackupPoisoned => StorageMaintenanceReason::BackupPoisoned,
        }),
        requested_at: Timestamp(job.requested_at),
        updated_at: Timestamp(job.updated_at),
        counters: StorageProgressCounters {
            eligible_records: job.estimates.eligible_records.map(StorageCount),
            eligible_bytes: job.estimates.eligible_bytes.map(StorageCount),
            processed_records: Some(StorageCount(job.processed_records)),
            processed_bytes: Some(StorageCount(job.processed_bytes)),
            blocked_records: job.estimates.blocked_records.map(StorageCount),
            blocked_bytes: job.estimates.blocked_bytes.map(StorageCount),
        },
    }
}

pub fn backup_usage(usage: BackupUsage) -> BackupStorage {
    BackupStorage {
        retained_bytes: usage.retained_bytes.map(StorageCount),
        temporary_bytes: usage.temporary_bytes.map(StorageCount),
    }
}

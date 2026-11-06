//! Aggregate storage observations only. No identity, paths or observation content.
use crate::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

/// Exact nonnegative counts, including bytes and unbounded record/page counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, TS)]
pub struct StorageCount(#[ts(type = "string")] pub u64);

impl Serialize for StorageCount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for StorageCount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.is_empty()
            || (text.len() > 1 && text.starts_with('0'))
            || !text.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(serde::de::Error::custom("Invalid storage count."));
        }
        text.parse::<u64>()
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageDatabaseState {
    Present,
    Absent,
    #[default]
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageAutoVacuum {
    None,
    Full,
    Incremental,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageJournalMode {
    Delete,
    Truncate,
    Persist,
    Memory,
    Wal,
    Off,
}

/// File lengths and SQLite logical allocation are separate observations. A live
/// writer can change them between reads; these are not an atomic disk snapshot.
/// File-only measurements leave logical fields unavailable with sidecars or an
/// unvalidated header. The existing history reader can supply live page counters.
/// The header identifies WAL mode only; other journal modes remain unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DatabaseStorage {
    pub state: StorageDatabaseState,
    pub main_bytes: Option<StorageCount>,
    pub wal_bytes: Option<StorageCount>,
    pub shm_bytes: Option<StorageCount>,
    pub page_size: Option<u32>,
    pub page_count: Option<StorageCount>,
    pub freelist_count: Option<StorageCount>,
    pub allocated_bytes: Option<StorageCount>,
    pub reusable_bytes: Option<StorageCount>,
    pub auto_vacuum: Option<StorageAutoVacuum>,
    pub journal_mode: Option<StorageJournalMode>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackupStorage {
    pub retained_bytes: Option<StorageCount>,
    pub temporary_bytes: Option<StorageCount>,
}

/// Supplied by the recording owner; database measurements never scan recordings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecordingStorage {
    pub physical_bytes: Option<StorageCount>,
    pub accounted_bytes: Option<StorageCount>,
    pub quota_bytes: Option<StorageCount>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageEstimateBasis {
    /// UTF-8 stream bytes + digest bytes + eight sequence bytes per legacy row.
    /// This is input size, not allocated pages, eligible detail or disk savings.
    LegacyReceiptLogicalBytes,
}

/// A null measured_at means not measured; a timestamp with null bytes means
/// the attempt failed. Neither state supplies a zero-byte estimate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StorageEstimate {
    pub bytes: Option<StorageCount>,
    pub basis: StorageEstimateBasis,
    pub measured_at: Option<Timestamp>,
}
impl Default for StorageEstimate {
    fn default() -> Self {
        Self {
            bytes: None,
            basis: StorageEstimateBasis::LegacyReceiptLogicalBytes,
            measured_at: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageMaintenancePhase {
    AwaitingBackup,
    CopyingReceipts,
    Verifying,
    Deleting,
    Reclaimable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageMaintenanceReason {
    Cancelled,
    AwaitingBackup,
    Capture,
    Budget,
    Wal,
    Busy,
    Full,
    Io,
    InvalidUnit,
    StoppedRequired,
    BlockedStream,
    Superseded,
    BackupPoisoned,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StorageProgressCounters {
    pub eligible_records: Option<StorageCount>,
    pub eligible_bytes: Option<StorageCount>,
    pub processed_records: Option<StorageCount>,
    pub processed_bytes: Option<StorageCount>,
    pub blocked_records: Option<StorageCount>,
    pub blocked_bytes: Option<StorageCount>,
}

/// needs_resume is true for cancelled, blocked_stream or backup_poisoned reasons,
/// regardless of paused. The Resume action uses this derived field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StorageJobProgress {
    pub phase: StorageMaintenancePhase,
    pub paused: bool,
    pub needs_resume: bool,
    pub reason: Option<StorageMaintenanceReason>,
    pub requested_at: Timestamp,
    pub updated_at: Timestamp,
    pub counters: StorageProgressCounters,
}

/// Totals include all supplied jobs (finished/cancelled included), excluding
/// superseded snapshots. Callers select the jobs counted as current progress.
/// Unknown inputs/overflow stay null.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StorageMaintenance {
    pub jobs: Vec<StorageJobProgress>,
    pub totals: StorageProgressCounters,
    pub last_success_at: Option<Timestamp>,
}

/// Phase 2 detail counts, eligible-detail bytes and rewrite-needed are omitted
/// until supported. They can be added without changing Phase 1 field meanings.
/// Null maintenance is unavailable; a measured empty job list means no jobs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StorageStatus {
    pub measured_at: Timestamp,
    pub database: DatabaseStorage,
    pub backups: BackupStorage,
    pub recordings: RecordingStorage,
    pub receipt_estimate: StorageEstimate,
    pub maintenance: Option<StorageMaintenance>,
    pub failure: Option<StorageFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StorageMaintenanceChanged {
    pub measured_at: Timestamp,
    pub maintenance: Option<StorageMaintenance>,
    pub failure: Option<StorageFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageFailure {
    SpaceUnknown,
    InsufficientSpace,
    Cancelled,
    Busy,
    Io,
    Full,
    InvalidUnit,
    BlockedStream,
    OwnerPoisoned,
    VerificationFailed,
    Superseded,
    WalPressure,
    InUse,
    StoppedRequired,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HistoryUpgradePhase {
    Required,
    BackingUp,
    Upgrading,
    Ready,
    Blocked,
}

/// Backup paths are provided only by the separate local Settings command.
/// last_failure records the previous action failure without overriding the probe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HistoryUpgradeStatus {
    pub phase: HistoryUpgradePhase,
    pub schema_version: Option<u32>,
    pub required_bytes: Option<StorageCount>,
    pub free_bytes: Option<StorageCount>,
    pub failure: Option<StorageFailure>,
    pub last_failure: Option<StorageFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackupRole {
    Initial,
    Rolling,
}

/// Local Settings inventory only. IDs never enter notices or diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ManagedStorageBackup {
    pub id: String,
    pub role: StorageBackupRole,
    pub completed_at: Option<Timestamp>,
    pub bytes: Option<StorageCount>,
    pub ready: bool,
    pub in_use: bool,
}

/// Command admission is distinct from completed maintenance work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageCommandOutcome {
    Accepted,
    Queued,
}

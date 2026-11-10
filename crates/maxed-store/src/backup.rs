//! Private, explicitly requested backups. Nothing in this module runs at writer startup.
use crate::{maintenance::Job, migrate::check_schema, writer::WriterLease, StoreError};
use rusqlite::{
    backup::{Backup, StepResult},
    params, Connection, OpenFlags, OptionalExtension, Transaction,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestructiveClass {
    LegacyUpgrade,
    ReceiptConversion,
    DetailReplacement,
    SealedReceiptPruning,
}

impl DestructiveClass {
    pub fn tag(self) -> &'static str {
        match self {
            Self::LegacyUpgrade => "legacy_upgrade",
            Self::ReceiptConversion => "receipt_conversion",
            Self::DetailReplacement => "detail_replacement",
            Self::SealedReceiptPruning => "sealed_receipt_pruning",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Initial,
    Rolling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(pub i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupId(pub i64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    pub id: BackupId,
    pub slot: Slot,
    pub schema: i64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupUsage {
    pub retained_bytes: Option<u64>,
    pub temporary_bytes: Option<u64>,
}

/// Local managed inventory, including incomplete images. No filesystem paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedBackup {
    pub id: BackupId,
    pub slot: Slot,
    pub completed_at: Option<u64>,
    pub bytes: Option<u64>,
    pub ready: bool,
    pub in_use: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupPreflight {
    pub required_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
}
impl BackupPreflight {
    pub fn check(self) -> Result<(), BackupError> {
        match (self.required_bytes, self.free_bytes) {
            (Some(required), Some(free)) if free >= required => Ok(()),
            (Some(_), Some(_)) => Err(BackupError::InsufficientSpace),
            _ => Err(BackupError::SpaceUnknown),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryPreflight {
    pub schema: Option<i64>,
    pub space: BackupPreflight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BackupError {
    #[error("Backup space is unavailable")]
    SpaceUnknown,
    #[error("Not enough space for a verified history backup")]
    InsufficientSpace,
    #[error("History backup is cancelled")]
    Cancelled,
    #[error("History backup is waiting for storage")]
    Busy,
    #[error("History backup I/O failed")]
    Io,
    #[error("History backup owner failed; reopen storage before retrying")]
    OwnerPoisoned,
    #[error("History backup verification failed")]
    Invalid,
    #[error("History backup no longer verifies; request a new backup")]
    VerificationFailed,
    #[error("History backup request was superseded; request a new backup")]
    Superseded,
    #[error("History backup is waiting for WAL pressure to subside")]
    WalPressure,
    #[error("History backup is in use")]
    InUse,
}

impl From<rusqlite::Error> for BackupError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                Self::Busy
            }
            Some(rusqlite::ErrorCode::DiskFull) => Self::InsufficientSpace,
            _ => Self::Io,
        }
    }
}
impl From<std::io::Error> for BackupError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
impl From<StoreError> for BackupError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Busy => Self::Busy,
            StoreError::Full => Self::InsufficientSpace,
            StoreError::Open | StoreError::Io => Self::Io,
            StoreError::BackupPoisoned => Self::OwnerPoisoned,
            _ => Self::Invalid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    Copy,
    Verify,
    FileFlush,
    Publish,
    Cleanup,
    CleanupRemoved,
    DeleteChecked,
}

pub trait BackupEnvironment {
    fn available_space(&self, directory: &Path) -> Option<u64>;
    fn timestamp(&self) -> u64;
    fn boundary(&self, _: Boundary) -> Result<(), BackupError> {
        Ok(())
    }
}

// The manifest is a small, separate FULL-sync SQLite database. Its transaction
// publishes the ready slot, request association and causal counter together.
// Legacy sources are never modified to record consent or lineage.
const MANIFEST: &str = "
CREATE TABLE IF NOT EXISTS lineage(id INTEGER PRIMARY KEY CHECK(id=1), identity TEXT NOT NULL,
 generation TEXT NOT NULL, sequence INTEGER NOT NULL, first_request INTEGER);
CREATE TABLE IF NOT EXISTS requests(id INTEGER PRIMARY KEY AUTOINCREMENT, class TEXT NOT NULL,
 requested_at INTEGER NOT NULL, requested_order INTEGER NOT NULL, first_order INTEGER,
 invalidated INTEGER NOT NULL DEFAULT 0 CHECK(invalidated IN (0,1)));
CREATE TABLE IF NOT EXISTS backups(id INTEGER PRIMARY KEY AUTOINCREMENT, request_id INTEGER NOT NULL REFERENCES requests,
 slot TEXT NOT NULL CHECK(slot IN ('initial','rolling')), state TEXT NOT NULL CHECK(state IN ('candidate','ready','retired')),
 completed_at INTEGER, completed_order INTEGER, bytes INTEGER, checksum TEXT, schema_version INTEGER, schema_checksum TEXT, heads_checksum TEXT);
CREATE UNIQUE INDEX IF NOT EXISTS ready_slot ON backups(slot) WHERE state='ready';
CREATE UNIQUE INDEX IF NOT EXISTS one_candidate ON backups(state) WHERE state='candidate';
CREATE TABLE IF NOT EXISTS heads(backup_id INTEGER NOT NULL REFERENCES backups ON DELETE CASCADE,
 stream_id TEXT NOT NULL, sequence INTEGER NOT NULL, PRIMARY KEY(backup_id,stream_id));
CREATE TABLE IF NOT EXISTS bindings(job_identity TEXT PRIMARY KEY, request_id INTEGER NOT NULL REFERENCES requests);
PRAGMA user_version=1;";

pub struct BackupStore {
    manifest: Connection,
    source: PathBuf,
    source_connection_paths: [PathBuf; 2],
    root: PathBuf,
    generation: String,
    // A verified file stays immutable while used for transaction authorization.
    // Windows sharing denies writes/deletes; deletion additionally checks jobs.
    verified: BTreeMap<i64, File>,
    _source: File,
    _directories: Vec<File>,
    _lease: File,
}

impl BackupStore {
    /// The source is an owner-resolved database, never a client-supplied path.
    /// All further operations accept manifest IDs only.
    pub fn open(source: &Path) -> Result<Self, BackupError> {
        plain_file(source)?;
        let canonical = fs::canonicalize(source)?;
        let source_connection_paths = [
            connection_path(Path::new(
                readonly(source)?.path().ok_or(BackupError::Invalid)?,
            ))
            .into_owned(),
            connection_path(&canonical).into_owned(),
        ];
        let source = canonical;
        plain_file(&source)?;
        let parent = source.parent().ok_or(BackupError::Invalid)?;
        let root = backup_root(&source)?;
        for directory in [parent.join("backups"), root.clone()] {
            private_directory(&directory)?;
            plain_directory(&directory)?;
        }
        let root = fs::canonicalize(root)?;
        if !root.starts_with(parent) {
            return Err(BackupError::Invalid);
        }
        let directories = guard_directories(&root)?;
        let source_file = open_shared(&source, false)?;
        let identity = source_identity(&source, &source_file)?;
        let lock_path = managed(&root, "owner.lock")?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lease.try_lock().map_err(|_| BackupError::InUse)?;
        let manifest_path = managed(&root, "manifest.sqlite3")?;
        for suffix in ["-journal", "-wal", "-shm"] {
            managed(&root, &format!("manifest.sqlite3{suffix}"))?;
        }
        let manifest = Connection::open(manifest_path)?;
        manifest.busy_timeout(Duration::from_millis(1))?;
        manifest.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;",
        )?;
        let version: i64 = manifest.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            return Err(BackupError::Invalid);
        }
        if version == 0 {
            let count: i64 = manifest.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            if count != 0 {
                return Err(BackupError::Invalid);
            }
            manifest.execute_batch(&format!("BEGIN IMMEDIATE;{MANIFEST}COMMIT;"))?;
        }
        manifest.execute(
            "INSERT OR IGNORE INTO lineage VALUES(1,?1,lower(hex(randomblob(16))),0,NULL)",
            [&identity],
        )?;
        let (saved, generation): (String, String) = manifest.query_row(
            "SELECT identity,generation FROM lineage WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if saved != identity {
            return Err(BackupError::Invalid);
        }
        Ok(Self {
            manifest,
            source,
            source_connection_paths,
            root,
            generation,
            verified: BTreeMap::new(),
            _source: source_file,
            _directories: directories,
            _lease: lease,
        })
    }

    /// Private generation for JobRequest.input_generation; never a public DTO.
    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn current_request(
        &self,
        class: DestructiveClass,
    ) -> Result<Option<RequestId>, BackupError> {
        Ok(self
            .manifest
            .query_row(
                "SELECT id FROM requests WHERE class=?1 ORDER BY id DESC LIMIT 1",
                [class.tag()],
                |r| r.get(0),
            )
            .optional()?
            .map(RequestId))
    }

    pub(crate) fn request_invalidated(&self, request: RequestId) -> Result<bool, BackupError> {
        Ok(self.manifest.query_row(
            "SELECT invalidated FROM requests WHERE id=?1",
            [request.0],
            |r| r.get(0),
        )?)
    }

    /// Explicit consent/renewal. Ordinary batches reuse the existing request.
    pub fn request(&mut self, class: DestructiveClass, at: u64) -> Result<RequestId, BackupError> {
        if let Some(current) = self.current_request(class)? {
            let id: Option<i64> = self
                .manifest
                .query_row(
                    "SELECT id FROM backups WHERE request_id=?1 AND state='ready'",
                    [current.0],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = id {
                if self.in_use(&readonly(&self.source)?, BackupId(id))? {
                    return Err(BackupError::InUse);
                }
            }
        }
        let tx = self.manifest.transaction()?;
        let order = advance(&tx)?;
        tx.execute(
            "INSERT INTO requests(class,requested_at,requested_order) VALUES(?1,?2,?3)",
            params![class.tag(), at.min(i64::MAX as u64) as i64, order],
        )?;
        let id = RequestId(tx.last_insert_rowid());
        tx.commit()?;
        Ok(id)
    }

    /// Persist the class request association before this job can be authorized.
    /// Binding a renewed request to an existing job is deliberately forbidden.
    pub fn bind_job(
        &self,
        request: RequestId,
        job: &crate::maintenance::JobRequest,
    ) -> Result<(), BackupError> {
        let class = self.request_class(request)?;
        if job.kind != class || job.input_generation != self.generation {
            return Err(BackupError::Invalid);
        }
        let key = job_identity(job)?;
        self.manifest.execute(
            "INSERT OR IGNORE INTO bindings VALUES(?1,?2)",
            params![key, request.0],
        )?;
        let bound: i64 = self.manifest.query_row(
            "SELECT request_id FROM bindings WHERE job_identity=?1",
            [key],
            |r| r.get(0),
        )?;
        if bound != request.0 {
            return Err(BackupError::Invalid);
        }
        Ok(())
    }

    fn request_class(&self, request: RequestId) -> Result<String, BackupError> {
        let row: Option<(String, bool, bool)> = self.manifest.query_row("SELECT class,invalidated,id=(SELECT max(r.id) FROM requests r WHERE r.class=requests.class) FROM requests WHERE id=?1", [request.0], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        match row {
            Some((class, false, true)) => Ok(class),
            Some(_) => Err(BackupError::Superseded),
            None => Err(BackupError::Invalid),
        }
    }

    pub fn list_managed(&self) -> Result<Vec<ManagedBackup>, BackupError> {
        let source = readonly(&self.source)?;
        list_managed(&self.manifest, &source, &self.root, &self.generation)
    }

    /// Metadata-only observation, without acquiring a writer or backup-owner
    /// lease. Concurrent changes may make file sizes unavailable.
    pub fn read_inventory(source: &Path) -> Result<(Vec<ManagedBackup>, BackupUsage), BackupError> {
        let root = backup_root(source)?;
        if !root.try_exists()? {
            return Ok((
                vec![],
                BackupUsage {
                    retained_bytes: Some(0),
                    temporary_bytes: Some(0),
                },
            ));
        }
        plain_directory(root.parent().ok_or(BackupError::Invalid)?)?;
        plain_directory(&root)?;
        let root = fs::canonicalize(root)?;
        let _directories = guard_directories(&root)?;
        let manifest_path = managed(&root, "manifest.sqlite3")?;
        if !manifest_path.try_exists()? {
            return Ok((
                vec![],
                BackupUsage {
                    retained_bytes: Some(0),
                    temporary_bytes: Some(0),
                },
            ));
        }
        for suffix in ["-journal", "-wal", "-shm"] {
            managed(&root, &format!("manifest.sqlite3{suffix}"))?;
        }
        let manifest = readonly(&manifest_path)?;
        let manifest = manifest.unchecked_transaction()?;
        if manifest.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? != 1 {
            return Err(BackupError::Invalid);
        }
        plain_file(source)?;
        let source = fs::canonicalize(source)?;
        let file = open_shared(&source, false)?;
        let (identity, generation): (String, String) = manifest.query_row(
            "SELECT identity,generation FROM lineage WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut inventory = list_metadata(&manifest)?;
        // Release the manifest snapshot before filesystem work or source schema
        // validation so an observation cannot pin backup publication.
        drop(manifest);
        if identity != source_identity(&source, &file)? {
            return Err(BackupError::Invalid);
        }
        let source = readonly(&source)?;
        let source = source.unchecked_transaction()?;
        measure_inventory(&mut inventory, &source, &root, &generation)?;
        let mut usage = BackupUsage {
            retained_bytes: Some(0),
            temporary_bytes: Some(0),
        };
        for backup in &inventory {
            let total = if backup.ready {
                &mut usage.retained_bytes
            } else {
                &mut usage.temporary_bytes
            };
            *total = total.and_then(|n| backup.bytes.and_then(|bytes| n.checked_add(bytes)));
        }
        Ok((inventory, usage))
    }
}

pub(crate) fn backup_root(source: &Path) -> Result<PathBuf, BackupError> {
    Ok(source
        .parent()
        .ok_or(BackupError::Invalid)?
        .join("backups")
        .join(source.file_stem().ok_or(BackupError::Invalid)?))
}

fn list_managed(
    manifest: &Connection,
    source: &Connection,
    root: &Path,
    generation: &str,
) -> Result<Vec<ManagedBackup>, BackupError> {
    let mut inventory = list_metadata(manifest)?;
    measure_inventory(&mut inventory, source, root, generation)?;
    Ok(inventory)
}

fn measure_inventory(
    inventory: &mut [ManagedBackup],
    source: &Connection,
    root: &Path,
    generation: &str,
) -> Result<(), BackupError> {
    for backup in inventory {
        backup.bytes = backup_file(root, backup.id)
            .ok()
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.len());
        backup.in_use = in_use(source, generation, backup.id)?;
    }
    Ok(())
}

fn list_metadata(manifest: &Connection) -> Result<Vec<ManagedBackup>, BackupError> {
    let rows = manifest
        .prepare("SELECT id,slot,completed_at,state FROM backups ORDER BY id")?
        .query_map([], |r| {
            Ok((
                BackupId(r.get(0)?),
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, slot, completed_at, state)| {
            Ok(ManagedBackup {
                id,
                slot: match slot.as_str() {
                    "initial" => Slot::Initial,
                    "rolling" => Slot::Rolling,
                    _ => return Err(BackupError::Invalid),
                },
                completed_at: completed_at.and_then(|n| u64::try_from(n).ok()),
                bytes: None,
                ready: state == "ready",
                in_use: false,
            })
        })
        .collect()
}

impl BackupStore {
    pub fn list(&self) -> Result<Vec<BackupInfo>, BackupError> {
        let rows = self
            .manifest
            .prepare(
                "SELECT id,slot,schema_version,bytes FROM backups WHERE state='ready' ORDER BY id",
            )?
            .query_map([], |r| {
                Ok((
                    BackupId(r.get(0)?),
                    r.get::<_, String>(1)?,
                    r.get(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, slot, schema, bytes)| {
                Ok(BackupInfo {
                    id,
                    slot: match slot.as_str() {
                        "initial" => Slot::Initial,
                        "rolling" => Slot::Rolling,
                        _ => return Err(BackupError::Invalid),
                    },
                    schema,
                    bytes: bytes as u64,
                })
            })
            .collect()
    }

    /// Physical image bytes, including a partial candidate or retired image
    /// awaiting cleanup. Measurement failures remain unknown, never zero.
    pub fn usage(&self) -> Result<BackupUsage, BackupError> {
        backup_usage(&self.manifest, &self.root)
    }

    fn file(&self, id: BackupId) -> Result<PathBuf, BackupError> {
        backup_file(&self.root, id)
    }
}

fn backup_usage(manifest: &Connection, root: &Path) -> Result<BackupUsage, BackupError> {
    let mut usage = BackupUsage {
        retained_bytes: Some(0),
        temporary_bytes: Some(0),
    };
    let mut statement = manifest.prepare("SELECT id,state FROM backups")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id = BackupId(row.get(0)?);
        let total = if row.get::<_, String>(1)? == "ready" {
            &mut usage.retained_bytes
        } else {
            &mut usage.temporary_bytes
        };
        let bytes = backup_file(root, id)
            .ok()
            .and_then(|path| fs::metadata(path).ok())
            .map(|m| m.len());
        *total = total.and_then(|n| bytes.and_then(|bytes| n.checked_add(bytes)));
    }
    Ok(usage)
}

fn backup_file(root: &Path, id: BackupId) -> Result<PathBuf, BackupError> {
    if id.0 <= 0 {
        return Err(BackupError::Invalid);
    }
    managed(root, &format!("backup-{:020}.sqlite3", id.0))
}

impl BackupStore {
    /// Recover an interrupted candidate or a published rotation's retired file.
    /// Cleanup must finish before another copy can allocate a third ready image.
    pub fn recover(&mut self, env: &dyn BackupEnvironment) -> Result<(), BackupError> {
        let ids: Vec<i64> = self
            .manifest
            .prepare("SELECT id FROM backups WHERE state<>'ready' ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for id in ids {
            env.boundary(Boundary::Cleanup)?;
            self.verified.remove(&id);
            let file = self.file(BackupId(id))?;
            for suffix in ["", "-journal", "-wal", "-shm"] {
                let name = format!(
                    "{}{suffix}",
                    file.file_name()
                        .ok_or(BackupError::Invalid)?
                        .to_str()
                        .ok_or(BackupError::Invalid)?
                );
                let path = managed(&self.root, &name)?;
                match fs::remove_file(path) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
            env.boundary(Boundary::CleanupRemoved)?;
            self.manifest
                .execute("DELETE FROM backups WHERE id=?1 AND state<>'ready'", [id])?;
        }
        Ok(())
    }

    fn check_source(&self, source: &Connection) -> Result<(), BackupError> {
        let path = source.path().ok_or(BackupError::Invalid)?;
        if fs::canonicalize(path)? != self.source {
            return Err(BackupError::Invalid);
        }
        Ok(())
    }

    fn check_space(
        &self,
        source: &Connection,
        env: &dyn BackupEnvironment,
    ) -> Result<(), BackupError> {
        backup_preflight(source, &self.source, &self.root, env)?.check()
    }

    /// Copy in 128-page steps from a dedicated read snapshot. Concurrent WAL
    /// commits do not restart it. Cancellation or WAL pressure releases the
    /// snapshot and abandons only the candidate; capture remains writable.
    pub fn create(
        &mut self,
        source: &Connection,
        request: RequestId,
        env: &dyn BackupEnvironment,
        cancel: &AtomicBool,
        between_steps: &mut dyn FnMut(),
    ) -> Result<BackupInfo, BackupError> {
        self.check_source(source)?;
        self.request_class(request)?;
        self.recover(env)?;
        if cancel.load(Ordering::Acquire) {
            return Err(BackupError::Cancelled);
        }
        let existing: bool = self.manifest.query_row(
            "SELECT EXISTS(SELECT 1 FROM backups WHERE request_id=?1 AND state='ready')",
            [request.0],
            |r| r.get(0),
        )?;
        if existing {
            let id = self.ready(request)?;
            return self
                .list()?
                .into_iter()
                .find(|b| b.id == id)
                .ok_or(BackupError::Invalid);
        }
        let mut snapshot = readonly(&self.source)?;
        let snapshot = snapshot.transaction()?;
        let before = metadata(&snapshot)?;
        let schema = before.schema;
        if schema == 0 {
            return Err(BackupError::Invalid);
        }
        let first: Option<i64> =
            self.manifest
                .query_row("SELECT first_request FROM lineage", [], |r| r.get(0))?;
        let slot = if first.is_none() {
            Slot::Initial
        } else {
            Slot::Rolling
        };
        if self.request_class(request)? == DestructiveClass::LegacyUpgrade.tag()
            && slot != Slot::Initial
        {
            return Err(BackupError::Invalid);
        }
        let slot_tag = if slot == Slot::Initial {
            "initial"
        } else {
            "rolling"
        };
        let replaced: Option<(i64, Option<i64>)> = self
            .manifest
            .query_row(
                "SELECT b.id,r.first_order FROM backups b JOIN requests r ON r.id=b.request_id WHERE b.slot=?1 AND b.state='ready'",
                [slot_tag],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, first_order)) = replaced {
            if (slot == Slot::Initial && first_order.is_some())
                || self.in_use(source, BackupId(id))?
            {
                return Err(BackupError::InUse);
            }
        }
        self.check_space(&snapshot, env)?;
        let wal_monitor = Connection::open_with_flags(
            &self.source,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        wal_monitor.busy_timeout(Duration::from_millis(1))?;
        self.manifest.execute(
            "INSERT INTO backups(request_id,slot,state) VALUES(?1,?2,'candidate')",
            params![request.0, slot_tag],
        )?;
        let id = BackupId(self.manifest.last_insert_rowid());
        let path = self.file(id)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut destination = Connection::open(&path)?;
        destination.busy_timeout(Duration::from_millis(1))?;
        destination.execute_batch("PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;")?;
        {
            let backup = Backup::new(&snapshot, &mut destination)?;
            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err(BackupError::Cancelled);
                }
                env.boundary(Boundary::Copy)?;
                self.check_wal_pressure(&wal_monitor)?;
                match backup.step(128)? {
                    StepResult::Done => break,
                    StepResult::More => {
                        between_steps();
                        std::thread::yield_now();
                    }
                    _ => return Err(BackupError::Busy),
                }
            }
        }
        drop(snapshot);
        destination.execute_batch("PRAGMA journal_mode=DELETE;")?;
        destination.close().map_err(|(_, e)| BackupError::from(e))?;
        env.boundary(Boundary::Verify)?;
        let candidate = readonly(&path)?;
        let captured = metadata(&candidate)?;
        if captured != before {
            return Err(BackupError::Invalid);
        }
        validate(&candidate)?;
        let bytes = fs::metadata(&path)?.len();
        if bytes != database_bytes(&candidate)? {
            return Err(BackupError::Invalid);
        }
        let checksum = checksum(&mut File::open(&path)?)?;
        {
            let tx = self.manifest.transaction()?;
            for (stream, sequence) in crate::receipts::heads(&candidate)? {
                tx.execute(
                    "INSERT INTO heads VALUES(?1,?2,?3)",
                    params![id.0, stream, sequence],
                )?;
            }
            tx.commit()?;
        }
        candidate.close().map_err(|(_, e)| BackupError::from(e))?;
        env.boundary(Boundary::FileFlush)?;
        OpenOptions::new().write(true).open(&path)?.sync_all()?;
        if cancel.load(Ordering::Acquire) {
            return Err(BackupError::Cancelled);
        }
        env.boundary(Boundary::Publish)?;
        let tx = self.manifest.transaction()?;
        let order = advance(&tx)?;
        if let Some((replaced, _)) = replaced {
            tx.execute("UPDATE requests SET invalidated=1 WHERE id IN (SELECT request_id FROM backups WHERE id=?1)",[replaced])?;
            tx.execute("UPDATE backups SET state='retired' WHERE id=?1", [replaced])?;
        }
        tx.execute("UPDATE backups SET state='ready',completed_at=?2,completed_order=?3,bytes=?4,checksum=?5,schema_version=?6,schema_checksum=?7,heads_checksum=?8 WHERE id=?1", params![id.0,env.timestamp().min(i64::MAX as u64) as i64,order,bytes as i64,checksum,captured.schema,captured.schema_checksum,captured.heads_checksum])?;
        tx.commit()?;
        self.recover(env)?;
        Ok(BackupInfo {
            id,
            slot,
            schema,
            bytes,
        })
    }

    fn check_wal_pressure(&self, monitor: &Connection) -> Result<(), BackupError> {
        let budget = crate::maintenance::Limits::default().wal_bytes;
        let mut wal = self.source.as_os_str().to_os_string();
        wal.push("-wal");
        if file_bytes(Path::new(&wal))? < budget {
            return Ok(());
        }
        // A reusable large WAL file is not pressure. PASSIVE never waits for the
        // capture writer, and reports frames retained by this or other readers.
        let (busy, frames, checkpointed): (i64, i64, i64) =
            monitor.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        let page_size: u32 = monitor.pragma_query_value(None, "page_size", |r| r.get(0))?;
        if busy != 0
            || frames < 0
            || checkpointed < 0
            || frames.saturating_sub(checkpointed).max(0) as u64 * (u64::from(page_size) + 24)
                >= budget
        {
            return Err(BackupError::WalPressure);
        }
        Ok(())
    }

    fn in_use(&self, source: &Connection, id: BackupId) -> Result<bool, BackupError> {
        in_use(source, &self.generation, id)
    }

    fn reference(&self, id: BackupId) -> String {
        format!("{}:{}", self.generation, id.0)
    }

    /// Explicit managed-ID deletion. It invalidates dependent authorization;
    /// another class's ready backup is never substituted.
    pub fn delete_managed(
        &mut self,
        id: BackupId,
        env: &dyn BackupEnvironment,
    ) -> Result<(), BackupError> {
        let source = Connection::open_with_flags(&self.source, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        source.busy_timeout(Duration::from_millis(1))?;
        self.delete(&source, id, env)
    }

    /// The supplied connection must be writable so deletion can serialize with
    /// source job control. Native callers use delete_managed instead.
    pub fn delete(
        &mut self,
        source: &Connection,
        id: BackupId,
        env: &dyn BackupEnvironment,
    ) -> Result<(), BackupError> {
        self.check_source(source)?;
        // Serialize the check and manifest invalidation against Resume and every
        // authorized unit, both of which write the source. The backup owner also
        // excludes publication/rotation. No copy or verification runs here.
        let source_guard =
            rusqlite::Transaction::new_unchecked(source, rusqlite::TransactionBehavior::Immediate)?;
        if self.in_use(&source_guard, id)? {
            return Err(BackupError::InUse);
        }
        env.boundary(Boundary::DeleteChecked)?;
        let tx = self.manifest.transaction()?;
        tx.execute("UPDATE requests SET invalidated=1 WHERE id IN (SELECT request_id FROM backups WHERE id=?1 AND state='ready')",[id.0])?;
        if tx.execute(
            "UPDATE backups SET state='retired' WHERE id=?1 AND state='ready'",
            [id.0],
        )? != 1
        {
            return Err(BackupError::Invalid);
        }
        tx.commit()?;
        drop(source_guard);
        self.recover(env)
    }

    /// Verification is repeated on reopening the owner. The immutable file
    /// handle avoids rescanning history inside each bounded writer transaction.
    fn ready(&mut self, request: RequestId) -> Result<BackupId, BackupError> {
        self.request_class(request)?;
        let (id,bytes,sum,schema,schema_sum,heads_sum): (i64,i64,String,i64,String,String) = self.manifest.query_row("SELECT b.id,b.bytes,b.checksum,b.schema_version,b.schema_checksum,b.heads_checksum FROM backups b JOIN requests r ON r.id=b.request_id WHERE b.request_id=?1 AND b.state='ready' AND b.completed_order>r.requested_order AND (r.first_order IS NULL OR b.completed_order<r.first_order)", [request.0], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?.ok_or(BackupError::Invalid)?;
        if !self.verified.contains_key(&id) {
            let path = self
                .file(BackupId(id))
                .map_err(|_| BackupError::VerificationFailed)?;
            plain_file(&path).map_err(|_| BackupError::VerificationFailed)?;
            let mut file = open_shared(&path, true).map_err(|_| BackupError::VerificationFailed)?;
            if file.metadata()?.len() != bytes as u64 || checksum(&mut file)? != sum {
                return Err(BackupError::VerificationFailed);
            }
            let db = readonly(&path).map_err(|_| BackupError::VerificationFailed)?;
            validate(&db).map_err(|_| BackupError::VerificationFailed)?;
            if metadata(&db)?
                != (Metadata {
                    schema,
                    schema_checksum: schema_sum,
                    heads_checksum: heads_sum,
                })
            {
                return Err(BackupError::VerificationFailed);
            }
            drop(db);
            self.verified.insert(id, file);
        }
        Ok(BackupId(id))
    }

    fn mark_first(&mut self, request: RequestId) -> Result<(), BackupError> {
        let tx = self.manifest.transaction()?;
        let first: Option<i64> =
            tx.query_row("SELECT first_request FROM lineage", [], |r| r.get(0))?;
        if first.is_none() {
            let initial: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM backups WHERE request_id=?1 AND state='ready' AND slot='initial')",[request.0],|r|r.get(0))?;
            if !initial {
                return Err(BackupError::Invalid);
            }
        }
        let marked: bool = tx.query_row(
            "SELECT first_order IS NOT NULL FROM requests WHERE id=?1",
            [request.0],
            |r| r.get(0),
        )?;
        if !marked {
            let order = advance(&tx)?;
            tx.execute(
                "UPDATE requests SET first_order=?2 WHERE id=?1",
                params![request.0, order],
            )?;
            tx.execute(
                "UPDATE lineage SET first_request=coalesce(first_request,?1)",
                [request.0],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Task 5 supplies this predicate to MaintenanceService. Prepare/verify the
    /// request outside the writer first; every unit still checks its association.
    pub fn prepare_authorization(&mut self, request: RequestId) -> Result<BackupId, BackupError> {
        self.ready(request)
    }

    pub fn authorize_job(&mut self, tx: &Transaction<'_>, job: &Job) -> Result<bool, StoreError> {
        let result = (|| -> Result<bool, BackupError> {
            // SQLite's resolved filename is cached when this owner opens. No
            // filesystem canonicalization or file verification on the writer.
            if !tx.path().map(Path::new).is_some_and(|path| {
                self.source_connection_paths
                    .iter()
                    .any(|saved| saved == connection_path(path).as_ref())
            }) {
                return Ok(false);
            }
            if job.request.input_generation != self.generation {
                return Ok(false);
            }
            let request: Option<i64> = self
                .manifest
                .query_row(
                    "SELECT request_id FROM bindings WHERE job_identity=?1",
                    [job_identity(&job.request)?],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(request) = request.map(RequestId) else {
                return Ok(false);
            };
            if self.request_class(request)? != job.request.kind {
                return Ok(false);
            }
            // Never do a full file verification on the capture writer.
            let id: Option<i64> = self
                .manifest
                .query_row(
                    "SELECT id FROM backups WHERE request_id=?1 AND state='ready'",
                    [request.0],
                    |r| r.get(0),
                )
                .optional()?;
            if id.is_none_or(|id| !self.verified.contains_key(&id)) {
                return Ok(false);
            }
            let id = self.ready(request)?;
            self.mark_first(request)?;
            let completed: i64 = self.manifest.query_row(
                "SELECT completed_at FROM backups WHERE id=?1",
                [id.0],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO backup_generations VALUES(?1,?2,?3)",
                params![self.reference(id), self.generation, completed],
            )?;
            if job
                .backup_id
                .as_ref()
                .is_some_and(|bound| bound != &self.reference(id))
            {
                return Ok(false);
            }
            tx.execute(
                "UPDATE maintenance_jobs SET backup_id=?2 WHERE job_id=?1 AND backup_id IS NULL",
                params![job.status.id, self.reference(id)],
            )?;
            Ok(true)
        })();
        match result {
            Ok(value) => Ok(value),
            Err(
                BackupError::Invalid | BackupError::VerificationFailed | BackupError::Superseded,
            ) => Ok(false),
            Err(BackupError::Busy | BackupError::InUse | BackupError::WalPressure) => {
                Err(StoreError::Busy)
            }
            Err(BackupError::InsufficientSpace) => Err(StoreError::Full),
            Err(_) => Err(StoreError::Io),
        }
    }
}

fn in_use(source: &Connection, generation: &str, id: BackupId) -> Result<bool, BackupError> {
    if check_schema(source).map_err(BackupError::from)? < 16 {
        return Ok(false);
    }
    // Dormant jobs release the backup; every resumed unit rechecks authorization.
    Ok(source.query_row(
        "SELECT EXISTS(SELECT 1 FROM maintenance_jobs WHERE backup_id=?1
        AND phase<>'reclaimable' AND paused=0
        AND coalesce(safe_error,'') NOT IN ('blocked_stream','superseded','backup_poisoned'))",
        [format!("{generation}:{}", id.0)],
        |r| r.get(0),
    )?)
}

fn connection_path(path: &Path) -> std::borrow::Cow<'_, Path> {
    #[cfg(windows)]
    if let Some(verbatim) = path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        return match verbatim.strip_prefix(r"UNC\") {
            Some(unc) => std::borrow::Cow::Owned(PathBuf::from(format!(r"\\{unc}"))),
            None => std::borrow::Cow::Borrowed(Path::new(verbatim)),
        };
    }
    std::borrow::Cow::Borrowed(path)
}

fn advance(tx: &Transaction<'_>) -> Result<i64, BackupError> {
    Ok(tx.query_row("UPDATE lineage SET sequence=sequence+1 WHERE id=1 AND sequence<9223372036854775807 RETURNING sequence",[],|r|r.get(0))?)
}

fn job_identity(job: &crate::maintenance::JobRequest) -> Result<String, BackupError> {
    let bytes = serde_json::to_vec(&(
        &job.kind,
        &job.key,
        job.format_version,
        &job.input_generation,
        job.input_revision,
    ))
    .map_err(|_| BackupError::Invalid)?;
    Ok(crate::migrate::digest(&bytes))
}

#[derive(PartialEq, Eq)]
struct Metadata {
    schema: i64,
    schema_checksum: String,
    heads_checksum: String,
}
fn metadata(db: &Connection) -> Result<Metadata, BackupError> {
    let schema = check_schema(db).map_err(BackupError::from)?;
    let mut schema_hash = Sha256::new();
    let mut statement =
        db.prepare("SELECT version,checksum FROM schema_migrations ORDER BY version")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        schema_hash.update(row.get::<_, i64>(0)?.to_le_bytes());
        schema_hash.update(row.get::<_, String>(1)?.as_bytes());
    }
    let mut statement = db.prepare(
        "SELECT type,name,tbl_name,coalesce(sql,'') FROM sqlite_schema ORDER BY type,name",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        for column in 0..4 {
            let value: String = row.get(column)?;
            schema_hash.update((value.len() as u64).to_le_bytes());
            schema_hash.update(value.as_bytes());
        }
    }
    let mut hash = Sha256::new();
    for (stream, sequence) in crate::receipts::heads(db)? {
        hash.update((stream.len() as u64).to_le_bytes());
        hash.update(stream.as_bytes());
        hash.update(sequence.to_le_bytes());
    }
    Ok(Metadata {
        schema,
        schema_checksum: format!("{:x}", schema_hash.finalize()),
        heads_checksum: format!("{:x}", hash.finalize()),
    })
}
fn readonly(path: &Path) -> Result<Connection, BackupError> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(Duration::from_millis(1))?;
    db.execute_batch("PRAGMA query_only=ON; PRAGMA foreign_keys=ON;")?;
    Ok(db)
}
fn validate(db: &Connection) -> Result<(), BackupError> {
    let version = check_schema(db).map_err(BackupError::from)?;
    // Explicitly verify configuration bounds as well: read-only integrity_check
    // does not reliably report CHECK violations on all SQLite builds.
    if version >= 20
        && db.query_row(
            "SELECT EXISTS(SELECT 1 FROM crucible_definitions WHERE
         typeof(id)<>'text' OR length(CAST(id AS BLOB)) NOT BETWEEN 1 AND 64
         OR typeof(damage_multiplier) NOT IN ('real','integer')
         OR NOT (damage_multiplier>0 AND damage_multiplier<=100)
         OR typeof(first_observed_at)<>'integer' OR first_observed_at<0
         OR typeof(last_observed_at)<>'integer' OR last_observed_at<0)
         OR (SELECT count(*) FROM crucible_definitions)>4096",
            [],
            |r| r.get::<_, bool>(0),
        )?
    {
        return Err(BackupError::Invalid);
    }
    let mut statement = db.prepare("PRAGMA integrity_check")?;
    let mut rows = statement.query([])?;
    if rows
        .next()?
        .ok_or(BackupError::Invalid)?
        .get::<_, String>(0)?
        != "ok"
        || rows.next()?.is_some()
    {
        return Err(BackupError::Invalid);
    }
    if db
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some()
    {
        return Err(BackupError::Invalid);
    }
    Ok(())
}
fn checksum(file: &mut File) -> Result<String, BackupError> {
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn database_bytes(db: &Connection) -> Result<u64, BackupError> {
    let pages: i64 = db.pragma_query_value(None, "page_count", |r| r.get(0))?;
    let size: i64 = db.pragma_query_value(None, "page_size", |r| r.get(0))?;
    u64::try_from(pages.checked_mul(size).ok_or(BackupError::Invalid)?)
        .map_err(|_| BackupError::Invalid)
}
fn file_bytes(path: &Path) -> Result<u64, BackupError> {
    match fs::metadata(path) {
        Ok(m) => Ok(m.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e.into()),
    }
}
fn plain_file(path: &Path) -> Result<(), BackupError> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || redirected(&m) {
        return Err(BackupError::Invalid);
    }
    Ok(())
}
fn plain_directory(path: &Path) -> Result<(), BackupError> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || redirected(&m) {
        return Err(BackupError::Invalid);
    }
    Ok(())
}
fn redirected(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        m.file_type().is_symlink()
    }
}
fn private_directory(path: &Path) -> Result<(), BackupError> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn managed(root: &Path, name: &str) -> Result<PathBuf, BackupError> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            plain_file(&path)?;
            if fs::canonicalize(&path)?.parent() != Some(root) {
                return Err(BackupError::Invalid);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(path)
}
fn open_shared(path: &Path, immutable: bool) -> Result<File, BackupError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(if immutable { 1 } else { 3 });
    }
    #[cfg(not(windows))]
    let _ = immutable;
    Ok(options.open(path)?)
}
fn guard_directories(root: &Path) -> Result<Vec<File>, BackupError> {
    root.ancestors()
        .take(2)
        .map(|path| {
            plain_directory(path)?;
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options
                    .access_mode(0)
                    .share_mode(3)
                    .custom_flags(0x0220_0000);
            }
            Ok(options.open(path)?)
        })
        .collect()
}
fn source_identity(path: &Path, file: &File) -> Result<String, BackupError> {
    let metadata = file.metadata()?;
    let created = metadata
        .created()?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| BackupError::Invalid)?
        .as_nanos();
    let identity = format!("{}:{created}", path.to_str().ok_or(BackupError::Invalid)?);
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{identity}:{}:{}", metadata.dev(), metadata.ino())
    };
    Ok(crate::migrate::digest(identity.as_bytes()))
}

/// A stopped pre-migration owner shares the writer lease but never calls
/// open_writer: installed versions 1–10 must remain untouched until verified.
pub struct HistoryUpgrade {
    conn: Connection,
    backups: BackupStore,
    _lease: WriterLease,
}
impl HistoryUpgrade {
    /// Read-only probe. Does not create a backup directory, manifest, request or
    /// migration. The native caller supplies an owner-resolved live path.
    pub fn preflight(
        path: &Path,
        env: &dyn BackupEnvironment,
    ) -> Result<HistoryPreflight, BackupError> {
        if !path.try_exists()? {
            return Ok(HistoryPreflight {
                schema: None,
                space: BackupPreflight {
                    required_bytes: None,
                    free_bytes: None,
                },
            });
        }
        plain_file(path)?;
        let conn = readonly(path)?;
        let schema = check_schema(&conn).map_err(BackupError::from)?;
        let root = backup_root(path)?;
        let destination = root
            .ancestors()
            .find(|p| p.exists())
            .ok_or(BackupError::Io)?;
        Ok(HistoryPreflight {
            schema: Some(schema),
            space: backup_preflight(&conn, path, destination, env)?,
        })
    }

    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let lease = WriterLease::acquire(path)?;
        if lease.has_abandoned_events() {
            return Err(StoreError::Busy);
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| StoreError::Open)?;
        if !(1..11).contains(&check_schema(&conn)?) {
            return Err(StoreError::Migration);
        }
        let backups = BackupStore::open(path).map_err(|_| StoreError::Io)?;
        Ok(Self {
            conn,
            backups,
            _lease: lease,
        })
    }

    /// Caller records explicit confirmation by invoking this method, while
    /// holding the native namespace lease with capture and history readers stopped.
    pub fn confirm_and_upgrade(
        &mut self,
        env: &dyn BackupEnvironment,
        cancel: &AtomicBool,
    ) -> Result<(), BackupError> {
        self.confirm_and_upgrade_with(env, cancel, &mut || {})
    }

    /// Called only after backup verification, immediately before migration.
    /// Cancellation is checked again after the callback, before marking first use.
    pub fn confirm_and_upgrade_with(
        &mut self,
        env: &dyn BackupEnvironment,
        cancel: &AtomicBool,
        upgrading: &mut dyn FnMut(),
    ) -> Result<(), BackupError> {
        if !(1..11).contains(&check_schema(&self.conn).map_err(|_| BackupError::Invalid)?) {
            return Err(BackupError::Invalid);
        }
        let existing = self
            .backups
            .current_request(DestructiveClass::LegacyUpgrade)?;
        let request = match existing.filter(|r| self.backups.ready(*r).is_ok()) {
            Some(request) => {
                self.backups.check_space(&self.conn, env)?;
                request
            }
            None => {
                let request = self
                    .backups
                    .request(DestructiveClass::LegacyUpgrade, env.timestamp())?;
                self.backups
                    .create(&self.conn, request, env, cancel, &mut || {})?;
                request
            }
        };
        if cancel.load(Ordering::Acquire) {
            return Err(BackupError::Cancelled);
        }
        let id = self.backups.ready(request)?;
        let original = readonly(&self.backups.file(id)?)?;
        if metadata(&original)? != metadata(&self.conn)? {
            return Err(BackupError::Invalid);
        }
        upgrading();
        if cancel.load(Ordering::Acquire) {
            return Err(BackupError::Cancelled);
        }
        self.backups.mark_first(request)?;
        crate::migrate(&mut self.conn).map_err(|error| match error {
            StoreError::Busy => BackupError::Busy,
            StoreError::Full => BackupError::InsufficientSpace,
            _ => BackupError::Invalid,
        })
    }
}

fn backup_preflight(
    source: &Connection,
    path: &Path,
    destination: &Path,
    env: &dyn BackupEnvironment,
) -> Result<BackupPreflight, BackupError> {
    // No credit for any retained backup's prospective deletion.
    let pages = database_bytes(source)?;
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let required_bytes = pages
        .checked_add(file_bytes(Path::new(&wal))?)
        .and_then(|n| n.checked_add((pages / 4).max(32 * 1024 * 1024)))
        .and_then(|n| n.checked_add(64 * 1024 * 1024));
    Ok(BackupPreflight {
        required_bytes,
        free_bytes: env.available_space(destination),
    })
}

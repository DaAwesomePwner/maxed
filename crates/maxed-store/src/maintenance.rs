//! Durable jobs share the capture writer. Executors must use keyset reads and
//! reserve each record before doing SQL; child cursors need their own bounded
//! units. No executor may commit, retain a transaction, or perform external I/O.
//! The caller holds the application's live namespace lease, as for start_writer.

use crate::{
    backup::{
        BackupEnvironment, BackupError, BackupId, BackupInfo, BackupStore, BackupUsage,
        DestructiveClass, ManagedBackup,
    },
    migrate::{open_writer, BUSY_TIMEOUT},
    writer::WriterLease,
    StoreError,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    AwaitingBackup,
    CopyingReceipts,
    Verifying,
    Deleting,
    Reclaimable,
}

impl Phase {
    fn tag(self) -> &'static str {
        match self {
            Self::AwaitingBackup => "awaiting_backup",
            Self::CopyingReceipts => "copying_receipts",
            Self::Verifying => "verifying",
            Self::Deleting => "deleting",
            Self::Reclaimable => "reclaimable",
        }
    }
    fn next(self) -> Self {
        match self {
            Self::AwaitingBackup => Self::CopyingReceipts,
            Self::CopyingReceipts => Self::Verifying,
            Self::Verifying => Self::Deleting,
            Self::Deleting | Self::Reclaimable => Self::Reclaimable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
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

impl PauseReason {
    fn tag(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::AwaitingBackup => "awaiting_backup",
            Self::Capture => "capture",
            Self::Budget => "budget",
            Self::Wal => "wal",
            Self::Busy => "busy",
            Self::Full => "full",
            Self::Io => "io",
            Self::InvalidUnit => "invalid_unit",
            Self::StoppedRequired => "stopped_required",
            Self::BlockedStream => "blocked_stream",
            Self::Superseded => "superseded",
            Self::BackupPoisoned => "backup_poisoned",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "cancelled" => Self::Cancelled,
            "awaiting_backup" => Self::AwaitingBackup,
            "capture" => Self::Capture,
            "budget" => Self::Budget,
            "wal" => Self::Wal,
            "busy" => Self::Busy,
            "full" => Self::Full,
            "io" => Self::Io,
            "stopped_required" => Self::StoppedRequired,
            "blocked_stream" => Self::BlockedStream,
            "superseded" => Self::Superseded,
            "backup_poisoned" => Self::BackupPoisoned,
            _ => Self::InvalidUnit,
        }
    }
    fn dormant(self) -> bool {
        matches!(
            self,
            Self::BlockedStream | Self::Superseded | Self::BackupPoisoned
        )
    }
}

/// Metadata only: no scope, stream, generation, cursor, SQL text or paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobStatus {
    pub id: i64,
    pub phase: Phase,
    /// Durable pause: cancellation requires explicit Resume. Superseded job
    /// snapshots remain paused permanently and are excluded from active progress.
    pub paused: bool,
    /// Real failures retry with per-job backoff; temporary pressure retries sooner.
    pub reason: Option<PauseReason>,
    /// UTC microseconds, matching the store's existing timestamp convention.
    pub requested_at: u64,
    pub updated_at: u64,
    /// Executor-defined committed work (admissions by default). Receipt conversion
    /// counts only moved legacy rows and their input bytes, including progress
    /// inherited on renewal. Superseded job snapshots are audit-only, not additive.
    pub processed_records: u64,
    pub processed_bytes: u64,
    pub estimates: Estimates,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Estimates {
    pub eligible_records: Option<u64>,
    pub eligible_bytes: Option<u64>,
    pub blocked_records: Option<u64>,
    pub blocked_bytes: Option<u64>,
}

/// A key identifies immutable inputs. Renewal (including after completion or
/// backup invalidation) needs a new key, e.g. one including lineage/generation.
#[derive(Clone, PartialEq, Eq)]
pub struct JobRequest {
    pub kind: String,
    pub key: String,
    pub format_version: u32,
    pub input_generation: String,
    pub input_revision: u64,
}

/// Private storage inputs for the executor/authorization seam, never a UI DTO.
pub struct Job {
    pub request: JobRequest,
    pub cursor: Option<String>,
    pub backup_id: Option<String>,
    pub status: JobStatus,
}

pub enum Control {
    Request(JobRequest),
    Pause(i64),
    Resume(i64),
    Status(i64),
    /// Notify after the authorization inputs change. This never resumes a
    /// cancelled job; authorization is still checked inside every work unit.
    AuthorizationChanged(i64),
    /// An orchestration API encountered a poisoned backup owner. Persist a safe,
    /// non-retrying failure on the current conversion job through the writer.
    BackupOwnerFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestDisposition {
    Created,
    Joined,
    AlreadyComplete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlResult {
    pub status: JobStatus,
    /// Present only for Request, never persisted or included in notices.
    pub request: Option<RequestDisposition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub receipts: u32,
    pub fights: u32,
    pub bytes: u64,
    pub target: Duration,
    pub wal_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            receipts: 1_000,
            fights: 32,
            bytes: 1024 * 1024,
            target: Duration::from_millis(10),
            wal_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Injectable time and WAL observation; no timing assertions require sleeping.
pub trait Environment: Send {
    fn monotonic(&self) -> Duration;
    /// UTC microseconds for persisted request/update metadata.
    fn timestamp(&self) -> u64;
    fn wal_size(&self, conn: &Connection) -> Result<u64, StoreError> {
        let path = conn.path().ok_or(StoreError::Io)?;
        match std::fs::metadata(format!("{path}-wal")) {
            Ok(meta) => Ok(meta.len()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(_) => Err(StoreError::Io),
        }
    }
    fn checkpoint(&self, conn: &Connection) -> Result<bool, StoreError> {
        let (busy, log, done): (i64, i64, i64) =
            conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        Ok(busy == 0 && log == done)
    }
}
pub struct SystemEnvironment(Instant);
impl Default for SystemEnvironment {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Environment for SystemEnvironment {
    fn monotonic(&self) -> Duration {
        self.0.elapsed()
    }
    fn timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
            .min(i64::MAX as u128) as u64
    }
}

#[derive(Clone, Copy)]
pub enum Record {
    Receipt,
    Fight,
}

pub struct UnitBudget<'a> {
    limits: Limits,
    record_ceiling: u64,
    env: &'a dyn Environment,
    started: Duration,
    interrupted: &'a dyn Fn() -> bool,
    stopped: bool,
    receipts: u32,
    fights: u32,
    bytes: u64,
    stopped_required: bool,
}
impl UnitBudget<'_> {
    /// Call before reading/processing each bounded record. One oversized allowed
    /// record can run alone while stopped. The first record within the configured
    /// ceiling always fits, even after adaptive reduction or slow authorization.
    /// Capture interruption still wins. This is cooperative, not a SQL timeout.
    pub fn reserve(&mut self, record: Record, bytes: u64) -> bool {
        let empty = self.receipts == 0 && self.fights == 0;
        if (self.interrupted)()
            || !empty && self.env.monotonic().saturating_sub(self.started) >= self.limits.target
        {
            return false;
        }
        if bytes > self.record_ceiling && empty && !self.stopped {
            self.stopped_required = true;
            return false;
        }
        if bytes > i64::MAX as u64 || !empty && self.bytes.saturating_add(bytes) > self.limits.bytes
        {
            return false;
        }
        match record {
            Record::Receipt if self.receipts < self.limits.receipts => self.receipts += 1,
            Record::Fight if self.fights < self.limits.fights => self.fights += 1,
            _ => return false,
        }
        self.bytes += bytes;
        true
    }
}

#[derive(Default)]
pub struct Processed {
    pub records: u64,
    pub bytes: u64,
}

#[derive(Default)]
pub struct UnitOutcome {
    pub cursor: Option<String>,
    pub phase_complete: bool,
    pub estimates: Estimates,
    pub deferred: Option<PauseReason>,
    /// Committed progress delta, bounded by this unit's reservations. None uses
    /// admitted work; count/validation passes can explicitly report zero.
    pub processed: Option<Processed>,
}
pub trait JobExecutor: Send {
    fn execute(
        &mut self,
        tx: &Transaction<'_>,
        job: &Job,
        budget: &mut UnitBudget<'_>,
    ) -> Result<UnitOutcome, StoreError>;
}
pub type Authorization = dyn FnMut(&Transaction<'_>, &Job) -> Result<bool, StoreError> + Send;

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Deferred(PauseReason),
    Committed(JobStatus),
}

pub struct MaintenanceService {
    executor: Box<dyn JobExecutor>,
    authorize: Box<Authorization>,
    env: Box<dyn Environment>,
    limits: Limits,
    ceiling: Limits,
    // Only job IDs are cached, never receipt inputs/cursors. None bootstraps
    // existing requests once; an empty set means no maintenance SQL while idle.
    runnable: Option<BTreeSet<i64>>,
    waiting: BTreeMap<i64, PauseReason>,
    failures: BTreeMap<i64, (u32, Duration)>,
    last_job: i64,
    observed: Option<JobStatus>,
    pending_notice: Option<JobStatus>,
    last_notice: Option<Duration>,
}

struct NoWork;
impl JobExecutor for NoWork {
    fn execute(
        &mut self,
        _: &Transaction<'_>,
        _: &Job,
        _: &mut UnitBudget<'_>,
    ) -> Result<UnitOutcome, StoreError> {
        Err(StoreError::Maintenance)
    }
}
impl Default for MaintenanceService {
    fn default() -> Self {
        Self::new(
            Box::new(NoWork),
            Box::new(|_, _| Ok(false)),
            Box::<SystemEnvironment>::default(),
            Limits::default(),
        )
        .expect("default maintenance limits")
    }
}
impl MaintenanceService {
    pub fn new(
        executor: Box<dyn JobExecutor>,
        authorize: Box<Authorization>,
        env: Box<dyn Environment>,
        limits: Limits,
    ) -> Result<Self, StoreError> {
        let ceiling = Limits::default();
        if limits.receipts == 0
            || limits.receipts > ceiling.receipts
            || limits.fights == 0
            || limits.fights > ceiling.fights
            || limits.bytes == 0
            || limits.bytes > ceiling.bytes
            || limits.target.is_zero()
            || limits.target > ceiling.target
            || limits.wal_bytes == 0
            || limits.wal_bytes > ceiling.wal_bytes
        {
            return Err(StoreError::Limit);
        }
        Ok(Self {
            executor,
            authorize,
            env,
            limits,
            ceiling: limits,
            runnable: None,
            waiting: BTreeMap::new(),
            failures: BTreeMap::new(),
            last_job: 0,
            observed: None,
            pending_notice: None,
            last_notice: None,
        })
    }

    pub(crate) fn control(
        &mut self,
        conn: &mut Connection,
        control: Control,
    ) -> Result<ControlResult, StoreError> {
        short_wait(conn, |conn| self.control_inner(conn, control))
    }

    fn control_inner(
        &mut self,
        conn: &Connection,
        control: Control,
    ) -> Result<ControlResult, StoreError> {
        let now = self.env.timestamp().min(i64::MAX as u64);
        let mut disposition = None;
        let wake = matches!(
            control,
            Control::Resume(_) | Control::AuthorizationChanged(_)
        );
        let id = match control {
            Control::Request(r) => {
                if [&r.kind, &r.key, &r.input_generation]
                    .iter()
                    .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
                    || r.format_version == 0
                    || r.input_revision > i64::MAX as u64
                {
                    return Err(StoreError::InvalidEvent);
                }
                let tx = conn.unchecked_transaction()?;
                let created = tx.execute("INSERT INTO maintenance_jobs(kind,job_key,format_version,input_generation,input_revision,requested_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(kind,job_key) DO NOTHING", params![r.kind,r.key,r.format_version,r.input_generation,r.input_revision as i64,now as i64])? != 0;
                let id = tx.query_row(
                    "SELECT job_id FROM maintenance_jobs WHERE kind=?1 AND job_key=?2",
                    params![r.kind, r.key],
                    |r| r.get(0),
                )?;
                let job = read_job(&tx, id)?;
                if job.request != r {
                    return Err(StoreError::MaintenanceRequestMismatch);
                }
                if created && r.kind == "receipt_conversion" {
                    // A renewed request is bound once to a new job. Retire its
                    // predecessor atomically with inheriting durable stream work;
                    // even explicit Resume can never reactivate that old job.
                    let paused: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM maintenance_jobs WHERE kind=?1 AND input_generation=?2
                         AND job_id<>?3 AND phase<>'reclaimable' AND paused=1 AND safe_error IS NOT 'superseded')",
                        params![r.kind,r.input_generation,id], |r| r.get(0))?;
                    let progress = crate::receipts::conversion_progress(&tx)?;
                    tx.execute("UPDATE maintenance_jobs SET paused=?2,safe_error=CASE WHEN ?2 THEN 'cancelled' END,
                        processed_records=?3,processed_bytes=?4 WHERE job_id=?1",
                        params![id,paused,sql_value(progress.records)?,sql_value(progress.bytes)?])?;
                    tx.execute("UPDATE maintenance_jobs SET paused=1,safe_error='superseded',updated_at=max(updated_at,?4)
                        WHERE kind=?1 AND input_generation=?2 AND job_id<>?3 AND phase<>'reclaimable' AND safe_error IS NOT 'superseded'",
                        params![r.kind,r.input_generation,id,now as i64])?;
                    retry_blocked_streams(&tx)?;
                }
                tx.commit()?;
                if created && r.kind == "receipt_conversion" {
                    // Rebuild IDs only, without retaining any receipt inputs.
                    self.runnable = None;
                }
                disposition = Some(if created {
                    RequestDisposition::Created
                } else if job.status.phase == Phase::Reclaimable {
                    RequestDisposition::AlreadyComplete
                } else {
                    RequestDisposition::Joined
                });
                id
            }
            Control::Pause(id) => {
                conn.execute("UPDATE maintenance_jobs SET paused=1,safe_error=CASE WHEN safe_error IN ('blocked_stream','backup_poisoned') THEN safe_error ELSE 'cancelled' END,updated_at=max(updated_at,?2) WHERE job_id=?1 AND phase<>'reclaimable' AND paused=0", params![id, now as i64])?;
                id
            }
            Control::Resume(id) => {
                let tx = conn.unchecked_transaction()?;
                let job = read_job(&tx, id)?;
                if job.request.kind == "receipt_conversion"
                    && job.status.reason == Some(PauseReason::BlockedStream)
                {
                    retry_blocked_streams(&tx)?;
                    tx.execute(
                        "UPDATE maintenance_jobs SET phase='copying_receipts' WHERE job_id=?1",
                        [id],
                    )?;
                }
                tx.execute("UPDATE maintenance_jobs SET paused=0,safe_error=NULL,updated_at=max(updated_at,?2) WHERE job_id=?1 AND phase<>'reclaimable' AND safe_error IS NOT 'superseded' AND (paused<>0 OR safe_error IN ('blocked_stream','backup_poisoned'))", params![id, now as i64])?;
                tx.commit()?;
                id
            }
            Control::BackupOwnerFailed => {
                let id = conn.query_row("SELECT job_id FROM maintenance_jobs WHERE kind='receipt_conversion'
                    AND phase<>'reclaimable' AND safe_error IS NOT 'superseded' ORDER BY job_id DESC LIMIT 1", [], |r| r.get(0))?;
                conn.execute("UPDATE maintenance_jobs SET safe_error='backup_poisoned',updated_at=max(updated_at,?2) WHERE job_id=?1", params![id,now as i64])?;
                id
            }
            Control::Status(id) | Control::AuthorizationChanged(id) => id,
        };
        let mut status = read_job(conn, id)?.status;
        if wake {
            self.waiting.remove(&id);
            self.failures.remove(&id);
        }
        if wake && self.observed.as_ref().is_some_and(|s| s.id == id) {
            self.observed = None;
        }
        if let Some(runnable) = &mut self.runnable {
            if status.paused
                || status.phase == Phase::Reclaimable
                || status.reason.is_some_and(PauseReason::dormant)
            {
                runnable.remove(&id);
            } else if wake || disposition.is_some() {
                runnable.insert(id);
            }
        }
        if !status.paused
            && status.phase != Phase::Reclaimable
            && !status.reason.is_some_and(PauseReason::dormant)
        {
            if self
                .runnable
                .as_ref()
                .is_some_and(|jobs| !jobs.contains(&id))
            {
                status.reason = self.waiting.get(&id).copied().or(status.reason);
            } else if let Some(observed) = self.observed.as_ref().filter(|s| {
                s.id == id && s.phase == status.phase && s.updated_at == status.updated_at
            }) {
                status.reason = observed.reason;
            }
        }
        self.observe(status.clone());
        Ok(ControlResult {
            status,
            request: disposition,
        })
    }

    fn observe(&mut self, status: JobStatus) {
        if self.observed.as_ref() != Some(&status) {
            self.pending_notice = Some(status.clone());
            self.observed = Some(status);
        }
    }

    pub(crate) fn step(
        &mut self,
        conn: &mut Connection,
        stopped: bool,
        interrupted: &dyn Fn() -> bool,
    ) -> Result<Step, StoreError> {
        if interrupted() {
            if let Some(mut status) = self
                .observed
                .clone()
                .filter(|s| !s.paused && s.phase != Phase::Reclaimable)
            {
                status.reason = Some(PauseReason::Capture);
                self.observe(status);
            }
            return Ok(Step::Deferred(PauseReason::Capture));
        }
        if !self.has_runnable_jobs() {
            return Ok(Step::Idle);
        }
        let next = short_wait(conn, |conn| self.next_job(conn));
        let Some(mut job) = (match next {
            Err(StoreError::Busy) => return Ok(Step::Deferred(PauseReason::Busy)),
            other => other?,
        }) else {
            return Ok(Step::Idle);
        };
        let id = job.status.id;
        let result = short_wait(conn, |conn| {
            // A fully checkpointed WAL can be reused even if its allocated file
            // remains large. A pinned reader defers further maintenance growth.
            if self.env.wal_size(conn)? >= self.limits.wal_bytes && !self.env.checkpoint(conn)? {
                return Ok(Step::Deferred(PauseReason::Wal));
            }
            if interrupted() {
                return Ok(Step::Deferred(PauseReason::Capture));
            }
            let started = self.env.monotonic();
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            // Default is closed. Recheck every unit so invalidated/deleted
            // backups cannot authorize resumed work.
            if !(self.authorize)(&tx, &job)? {
                return Ok(Step::Deferred(PauseReason::AwaitingBackup));
            }
            if interrupted() {
                return Ok(Step::Deferred(PauseReason::Capture));
            }
            let mut budget = UnitBudget {
                limits: self.limits,
                record_ceiling: self.ceiling.bytes,
                env: self.env.as_ref(),
                started,
                interrupted,
                stopped,
                receipts: 0,
                fights: 0,
                bytes: 0,
                stopped_required: false,
            };
            let work_started = self.env.monotonic();
            let outcome = if job.status.phase == Phase::AwaitingBackup {
                UnitOutcome {
                    phase_complete: true,
                    ..Default::default()
                }
            } else {
                self.executor.execute(&tx, &job, &mut budget)?
            };
            let work_elapsed = self.env.monotonic().saturating_sub(work_started);
            if let Some(reason) = outcome.deferred {
                if reason == PauseReason::BlockedStream {
                    // Deferred executors never commit data work. Annotate the
                    // already committed stream state in a separate transaction
                    // before releasing the backup's in-use reference.
                    drop(tx);
                    let tx =
                        conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    tx.execute("UPDATE maintenance_jobs SET safe_error='blocked_stream',updated_at=max(updated_at,?2),
                        eligible_records=?3,eligible_bytes=?4,blocked_records=?5,blocked_bytes=?6 WHERE job_id=?1",
                        params![id,sql_value(self.env.timestamp())?,outcome.estimates.eligible_records.map(sql_value).transpose()?,
                            outcome.estimates.eligible_bytes.map(sql_value).transpose()?,outcome.estimates.blocked_records.map(sql_value).transpose()?,
                            outcome.estimates.blocked_bytes.map(sql_value).transpose()?])?;
                    job.status = read_job(&tx, id)?.status;
                    tx.commit()?;
                }
                return Ok(Step::Deferred(reason));
            }
            if outcome.cursor.as_ref().is_some_and(|c| c.len() > 4096)
                || !outcome.phase_complete
                    && (budget.receipts > 0 || budget.fights > 0)
                    && (outcome.cursor.is_none() || outcome.cursor == job.cursor)
            {
                return Err(StoreError::Maintenance);
            }
            if budget.receipts == 0 && budget.fights == 0 && !outcome.phase_complete {
                return Ok(Step::Deferred(if budget.stopped_required {
                    PauseReason::StoppedRequired
                } else if interrupted() {
                    PauseReason::Capture
                } else {
                    PauseReason::Budget
                }));
            }
            let phase = if outcome.phase_complete {
                job.status.phase.next()
            } else {
                job.status.phase
            };
            let processed = outcome.processed.unwrap_or(Processed {
                records: u64::from(budget.receipts) + u64::from(budget.fights),
                bytes: budget.bytes,
            });
            if processed.records > u64::from(budget.receipts) + u64::from(budget.fights)
                || processed.bytes > budget.bytes
            {
                return Err(StoreError::Maintenance);
            }
            let records = job
                .status
                .processed_records
                .checked_add(processed.records)
                .ok_or(StoreError::Limit)?;
            let bytes = job
                .status
                .processed_bytes
                .checked_add(processed.bytes)
                .ok_or(StoreError::Limit)?;
            let to_sql = |n: u64| i64::try_from(n).map_err(|_| StoreError::Limit);
            let estimates = &outcome.estimates;
            tx.execute("UPDATE maintenance_jobs SET keyset_cursor=?2,phase=?3,safe_error=NULL,updated_at=max(updated_at,?4),processed_records=?5,processed_bytes=?6,eligible_records=?7,eligible_bytes=?8,blocked_records=?9,blocked_bytes=?10 WHERE job_id=?1", params![id,if outcome.phase_complete {None} else {outcome.cursor},phase.tag(),to_sql(self.env.timestamp())?,to_sql(records)?,to_sql(bytes)?,estimates.eligible_records.map(to_sql).transpose()?,estimates.eligible_bytes.map(to_sql).transpose()?,estimates.blocked_records.map(to_sql).transpose()?,estimates.blocked_bytes.map(to_sql).transpose()?])?;
            let status = read_job(&tx, id)?.status;
            tx.commit()?;
            // Adapt to executor work, excluding fixed transaction/authorization,
            // job-status and FULL-sync commit costs. A slow fsync cannot be made
            // cheaper by shrinking the batch. Admission still uses `started`
            // before BEGIN, so setup latency leaves less time for this turn's
            // records. Only a committed unit changes the next turn's limits.
            if work_elapsed > self.limits.target {
                self.limits.receipts = (self.limits.receipts / 2).max(1);
                self.limits.fights = (self.limits.fights / 2).max(1);
                // Keep a 64 KiB batch floor (or the smaller configured ceiling).
                // reserve additionally admits any first record up to the ceiling.
                self.limits.bytes = (self.limits.bytes / 2).max(self.ceiling.bytes.min(64 * 1024));
            } else if work_elapsed < self.limits.target {
                self.limits.receipts = (self.limits.receipts * 2).min(self.ceiling.receipts);
                self.limits.fights = (self.limits.fights * 2).min(self.ceiling.fights);
                self.limits.bytes = (self.limits.bytes * 2).min(self.ceiling.bytes);
            }
            Ok(Step::Committed(status))
        });
        let result = match result {
            Err(StoreError::Busy) => Ok(Step::Deferred(PauseReason::Busy)),
            other => other,
        };
        match &result {
            Ok(Step::Committed(status)) => {
                self.failures.remove(&id);
                self.waiting.remove(&id);
                if status.phase == Phase::Reclaimable {
                    self.runnable.as_mut().unwrap().remove(&id);
                }
                self.observe(status.clone());
            }
            Ok(Step::Deferred(reason)) => {
                if matches!(
                    reason,
                    PauseReason::AwaitingBackup
                        | PauseReason::StoppedRequired
                        | PauseReason::BlockedStream
                ) {
                    self.runnable.as_mut().unwrap().remove(&id);
                    self.waiting.insert(id, *reason);
                }
                job.status.reason = Some(*reason);
                self.observe(job.status);
            }
            Err(error) => {
                let attempts = self
                    .failures
                    .get(&id)
                    .map_or(1, |(n, _)| n.saturating_add(1));
                let delay = Duration::from_secs(
                    (5_u64.saturating_mul(1_u64 << attempts.min(7).saturating_sub(1))).min(300),
                );
                self.failures
                    .insert(id, (attempts, self.env.monotonic().saturating_add(delay)));
                let reason = match error {
                    StoreError::Full => PauseReason::Full,
                    StoreError::Io | StoreError::Open => PauseReason::Io,
                    StoreError::BackupPoisoned => PauseReason::BackupPoisoned,
                    _ => PauseReason::InvalidUnit,
                };
                if reason == PauseReason::BackupPoisoned {
                    self.runnable.as_mut().unwrap().remove(&id);
                    self.waiting.insert(id, reason);
                }
                let unchanged = job.status.reason == Some(reason);
                job.status.reason = Some(reason);
                if unchanged {
                    self.observe(job.status);
                    return result;
                }
                job.status.updated_at = job
                    .status
                    .updated_at
                    .max(self.env.timestamp().min(i64::MAX as u64));
                // Failures are separate from cancellation and back off per job.
                // If the disk cannot save the annotation, keep
                // the last durable status, exactly as a restart would read it.
                // The step still returns the error; no memory-only pause can
                // block this or another job.
                let saved = short_wait(conn, |conn| {
                    Ok(conn.execute(
                        "UPDATE maintenance_jobs SET safe_error=?2,updated_at=?3 WHERE job_id=?1 AND safe_error IS NOT ?2",
                        params![id, reason.tag(), job.status.updated_at as i64],
                    )?)
                });
                if saved.is_ok() {
                    self.observe(job.status);
                } else {
                    self.observed = None;
                    self.pending_notice = None;
                }
            }
            _ => {}
        }
        result
    }

    fn next_job(&mut self, conn: &Connection) -> Result<Option<Job>, StoreError> {
        if self.runnable.is_none() {
            self.runnable = Some(
                conn.prepare(
                    "SELECT job_id FROM maintenance_jobs WHERE paused=0 AND phase<>'reclaimable'
                     AND coalesce(safe_error,'') NOT IN ('blocked_stream','superseded','backup_poisoned')",
                )?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?,
            );
        }
        let runnable = self.runnable.as_ref().unwrap();
        let due = |id: &&i64| {
            self.failures
                .get(id)
                .is_none_or(|(_, until)| self.env.monotonic() >= *until)
        };
        let id = runnable
            .range((
                std::ops::Bound::Excluded(self.last_job),
                std::ops::Bound::Unbounded,
            ))
            .find(due)
            .or_else(|| runnable.iter().find(due))
            .copied();
        self.last_job = id.unwrap_or(0);
        id.map(|id| read_job(conn, id)).transpose()
    }

    pub(crate) fn has_runnable_jobs(&self) -> bool {
        self.runnable.as_ref().is_none_or(|jobs| !jobs.is_empty())
    }

    /// Coalesces to the newest metadata and emits at most once per second,
    /// including pause/completion notices. Polling status is always immediate.
    pub fn take_notice(&mut self) -> Option<JobStatus> {
        let now = self.env.monotonic();
        if self
            .last_notice
            .is_some_and(|last| now.saturating_sub(last) < Duration::from_secs(1))
        {
            return None;
        }
        let notice = self.pending_notice.take()?;
        self.last_notice = Some(now);
        Some(notice)
    }
}

fn short_wait<T>(
    conn: &mut Connection,
    action: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    conn.busy_timeout(Duration::from_millis(1))?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(conn)))
        .unwrap_or(Err(StoreError::Maintenance));
    conn.busy_timeout(BUSY_TIMEOUT)?;
    result
}

fn retry_blocked_streams(conn: &Connection) -> Result<(), StoreError> {
    // Revalidate only the unconverted suffix; committed copies and their totals
    // survive both renewal and an explicit retry of repaired input.
    conn.execute(
        "UPDATE event_streams SET legacy_state='scanning',legacy_scanned=legacy_moved,
        legacy_count=NULL,legacy_error=NULL WHERE legacy_state='blocked'",
        [],
    )?;
    Ok(())
}

fn read_job(conn: &Connection, id: i64) -> Result<Job, StoreError> {
    Ok(conn.query_row(
        "SELECT kind,job_key,format_version,input_generation,input_revision,
            keyset_cursor,backup_id,phase,paused,safe_error,requested_at,updated_at,
            processed_records,processed_bytes,eligible_records,eligible_bytes,
            blocked_records,blocked_bytes FROM maintenance_jobs WHERE job_id=?1",
        [id],
        |r| {
            let phase = match r.get::<_, String>(7)?.as_str() {
                "awaiting_backup" => Phase::AwaitingBackup,
                "copying_receipts" => Phase::CopyingReceipts,
                "verifying" => Phase::Verifying,
                "deleting" => Phase::Deleting,
                "reclaimable" => Phase::Reclaimable,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(Job {
                request: JobRequest {
                    kind: r.get(0)?,
                    key: r.get(1)?,
                    format_version: r.get(2)?,
                    input_generation: r.get(3)?,
                    input_revision: sql_count(r, 4)?,
                },
                cursor: r.get(5)?,
                backup_id: r.get(6)?,
                status: JobStatus {
                    id,
                    phase,
                    paused: r.get(8)?,
                    reason: r
                        .get::<_, Option<String>>(9)?
                        .as_deref()
                        .map(PauseReason::parse),
                    requested_at: sql_count(r, 10)?,
                    updated_at: sql_count(r, 11)?,
                    processed_records: sql_count(r, 12)?,
                    processed_bytes: sql_count(r, 13)?,
                    estimates: Estimates {
                        eligible_records: sql_optional_count(r, 14)?,
                        eligible_bytes: sql_optional_count(r, 15)?,
                        blocked_records: sql_optional_count(r, 16)?,
                        blocked_bytes: sql_optional_count(r, 17)?,
                    },
                },
            })
        },
    )?)
}

/// Own on an orchestration thread while capture is stopped. The process-local
/// lease also excludes StoreWriter; the native namespace lease remains the
/// caller's responsibility. Each step closes its transaction before returning.
pub struct StoppedMaintenance {
    conn: Connection,
    service: MaintenanceService,
    _lease: WriterLease,
    _thread_bound: PhantomData<Rc<()>>,
    conversion: Option<ConversionAccess>,
}
impl StoppedMaintenance {
    pub fn list_backups(&self) -> Result<Vec<ManagedBackup>, BackupError> {
        self.conversion
            .as_ref()
            .map_or_else(|| Ok(Vec::new()), ConversionAccess::list_backups)
    }
    pub fn backup_usage(&self) -> Result<BackupUsage, BackupError> {
        self.conversion.as_ref().map_or_else(
            || {
                Ok(BackupUsage {
                    retained_bytes: None,
                    temporary_bytes: None,
                })
            },
            ConversionAccess::backup_usage,
        )
    }
    pub fn delete_backup(
        &self,
        id: BackupId,
        env: &dyn BackupEnvironment,
    ) -> Result<(), BackupError> {
        self.conversion
            .as_ref()
            .ok_or(BackupError::Invalid)?
            .delete_backup(id, env)
    }
    pub fn receipt_backup_task(&self) -> Result<ReceiptBackupTask, BackupError> {
        Ok(ReceiptBackupTask(
            self.conversion.clone().ok_or(BackupError::Invalid)?,
        ))
    }
    pub fn statuses(&self) -> Result<Vec<JobStatus>, StoreError> {
        statuses(&self.conn)
    }
    pub fn open(path: &Path, service: MaintenanceService) -> Result<Self, StoreError> {
        let lease = WriterLease::acquire(path)?;
        if lease.has_abandoned_events() {
            return Err(StoreError::Busy);
        }
        Ok(Self {
            conn: open_writer(path)?,
            service,
            _lease: lease,
            _thread_bound: PhantomData,
            conversion: None,
        })
    }
    /// Production conversion owner. Opening never requests a job or a backup.
    pub fn receipt_conversion(path: &Path) -> Result<Self, StoreError> {
        let access = ConversionAccess::new(path.to_path_buf());
        let mut owner = Self::open(path, access.service())?;
        owner.conversion = Some(access);
        Ok(owner)
    }
    /// Exercise the real conversion and backup authorization with deterministic
    /// budget time. Production owners always use SystemEnvironment.
    #[doc(hidden)]
    pub fn receipt_conversion_with_environment_for_tests(
        path: &Path,
        environment: Box<dyn Environment>,
    ) -> Result<Self, StoreError> {
        let mut owner = Self::receipt_conversion(path)?;
        owner.service.env = environment;
        Ok(owner)
    }
    pub fn request_receipt_conversion(&mut self, at: u64) -> Result<ControlResult, BackupError> {
        let request = self
            .conversion
            .as_ref()
            .ok_or(BackupError::Invalid)?
            .request(at);
        if matches!(request, Err(BackupError::OwnerPoisoned)) {
            // Keep the distinct owner failure even if there is no job yet or
            // storage cannot persist its annotation.
            let _ = self.control(Control::BackupOwnerFailed);
        }
        Ok(self.control(Control::Request(request?))?)
    }
    pub fn backup_receipt_conversion(
        &mut self,
        env: &dyn BackupEnvironment,
        cancel: &AtomicBool,
        between_steps: &mut dyn FnMut(),
    ) -> Result<BackupInfo, BackupError> {
        let result = self
            .conversion
            .as_ref()
            .ok_or(BackupError::Invalid)?
            .backup(env, cancel, between_steps);
        if matches!(result, Err(BackupError::OwnerPoisoned)) {
            let _ = self.control(Control::BackupOwnerFailed);
        }
        let (info, id) = result?;
        self.control(Control::AuthorizationChanged(id))?;
        Ok(info)
    }
    pub fn control(&mut self, control: Control) -> Result<ControlResult, StoreError> {
        self.service.control(&mut self.conn, control)
    }
    pub fn step(&mut self) -> Result<Step, StoreError> {
        self.service.step(&mut self.conn, true, &|| false)
    }
    pub fn take_notice(&mut self) -> Option<JobStatus> {
        self.service.take_notice()
    }
}

/// Shares the existing backup owner, without a writer connection or job controls.
/// Run on a backup worker and deliver AuthorizationChanged(id) to the storage
/// owner after success. Join/drop this task before releasing namespace ownership.
pub struct ReceiptBackupTask(ConversionAccess);
impl ReceiptBackupTask {
    pub fn run(
        self,
        env: &dyn BackupEnvironment,
        cancel: &AtomicBool,
        between_steps: &mut dyn FnMut(),
    ) -> Result<(BackupInfo, i64), BackupError> {
        self.0.backup(env, cancel, between_steps)
    }
}

pub(crate) fn statuses(conn: &Connection) -> Result<Vec<JobStatus>, StoreError> {
    let tx = conn.unchecked_transaction()?;
    let ids = tx.prepare("SELECT job_id FROM maintenance_jobs WHERE kind='receipt_conversion' AND safe_error IS NOT 'superseded' ORDER BY job_id")?
        .query_map([], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|id| read_job(&tx, id).map(|j| j.status))
        .collect()
}

/// Lazily opens the private backup owner only on an orchestration-thread API
/// call. The capture writer only tries the mutex and uses prepared authorization.
#[derive(Clone)]
pub(crate) struct ConversionAccess {
    path: PathBuf,
    backups: Arc<Mutex<Option<BackupStore>>>,
}
impl ConversionAccess {
    fn with_existing<T>(
        &self,
        absent: T,
        operation: impl FnOnce(&mut BackupStore) -> Result<T, BackupError>,
    ) -> Result<T, BackupError> {
        let mut guard = self.backups.try_lock().map_err(|e| match e {
            std::sync::TryLockError::WouldBlock => BackupError::Busy,
            std::sync::TryLockError::Poisoned(_) => BackupError::OwnerPoisoned,
        })?;
        if guard.is_none() {
            let manifest = crate::backup::backup_root(&self.path)?.join("manifest.sqlite3");
            if !manifest.try_exists()? {
                return Ok(absent);
            }
            *guard = Some(BackupStore::open(&self.path)?);
        }
        operation(guard.as_mut().unwrap())
    }
    pub(crate) fn list_backups(&self) -> Result<Vec<ManagedBackup>, BackupError> {
        self.with_existing(Vec::new(), |b| b.list_managed())
    }
    pub(crate) fn backup_usage(&self) -> Result<BackupUsage, BackupError> {
        self.with_existing(
            BackupUsage {
                retained_bytes: Some(0),
                temporary_bytes: Some(0),
            },
            |b| b.usage(),
        )
    }
    pub(crate) fn delete_backup(
        &self,
        id: BackupId,
        env: &dyn BackupEnvironment,
    ) -> Result<(), BackupError> {
        self.with_existing(Err(BackupError::Invalid), |b| Ok(b.delete_managed(id, env)))?
    }
    pub(crate) fn backup_task(&self) -> ReceiptBackupTask {
        ReceiptBackupTask(self.clone())
    }
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            backups: Arc::new(Mutex::new(None)),
        }
    }
    pub(crate) fn service(&self) -> MaintenanceService {
        let backups = self.backups.clone();
        MaintenanceService::new(
            Box::new(crate::receipts::ConversionExecutor),
            Box::new(move |tx, job| {
                let mut guard = backups.try_lock().map_err(|e| match e {
                    std::sync::TryLockError::WouldBlock => StoreError::Busy,
                    std::sync::TryLockError::Poisoned(_) => StoreError::BackupPoisoned,
                })?;
                match guard.as_mut() {
                    Some(backups) => backups.authorize_job(tx, job),
                    None => Ok(false),
                }
            }),
            Box::<SystemEnvironment>::default(),
            Limits::default(),
        )
        .expect("default conversion limits")
    }
    pub(crate) fn request(&self, at: u64) -> Result<JobRequest, BackupError> {
        let db =
            Connection::open_with_flags(&self.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        // Completion is durable independently of backup retention. Rejoin it
        // without rebinding an invalidated request or opening/verifying a backup.
        let completed: Option<i64> = db
            .query_row(
                "SELECT job_id FROM maintenance_jobs
            WHERE kind='receipt_conversion' AND phase='reclaimable' ORDER BY job_id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = completed {
            return Ok(read_job(&db, id)?.request);
        }
        let mut guard = self.backups.try_lock().map_err(|e| match e {
            std::sync::TryLockError::WouldBlock => BackupError::Busy,
            std::sync::TryLockError::Poisoned(_) => BackupError::OwnerPoisoned,
        })?;
        if guard.is_none() {
            *guard = Some(BackupStore::open(&self.path)?);
        }
        let backups = guard.as_mut().unwrap();
        let class = DestructiveClass::ReceiptConversion;
        let request = match backups.current_request(class)? {
            Some(request) if !backups.request_invalidated(request)? => request,
            _ => backups.request(class, at)?,
        };
        let job = JobRequest {
            kind: class.tag().into(),
            key: format!("receipts-{}", request.0),
            format_version: 1,
            input_generation: backups.generation().into(),
            input_revision: request.0 as u64,
        };
        backups.bind_job(request, &job)?;
        Ok(job)
    }
    pub(crate) fn backup(
        &self,
        env: &dyn BackupEnvironment,
        cancel: &AtomicBool,
        between_steps: &mut dyn FnMut(),
    ) -> Result<(BackupInfo, i64), BackupError> {
        let mut guard = self
            .backups
            .lock()
            .map_err(|_| BackupError::OwnerPoisoned)?;
        // Reattach a persisted request after restart without enqueuing work.
        // File verification stays on the calling orchestration thread.
        if guard.is_none() {
            *guard = Some(BackupStore::open(&self.path)?);
        }
        let backups = guard.as_mut().ok_or(BackupError::Invalid)?;
        let request = backups
            .current_request(DestructiveClass::ReceiptConversion)?
            .ok_or(BackupError::Invalid)?;
        let db =
            Connection::open_with_flags(&self.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let id = db.query_row("SELECT job_id FROM maintenance_jobs WHERE kind='receipt_conversion' AND job_key=?1 AND input_generation=?2", params![format!("receipts-{}",request.0),backups.generation()], |r| r.get(0))?;
        let info = backups.create(&db, request, env, cancel, between_steps)?;
        backups.prepare_authorization(request)?;
        Ok((info, id))
    }
}

pub(crate) fn sql_count(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn sql_value(n: u64) -> Result<i64, StoreError> {
    i64::try_from(n).map_err(|_| StoreError::Limit)
}
fn sql_optional_count(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|n| u64::try_from(n).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    };

    thread_local! {
        static MILLIS: Cell<u64> = const { Cell::new(0) };
        static COMMIT_MILLIS: Cell<u64> = const { Cell::new(0) };
        static BEGIN_MILLIS: Cell<u64> = const { Cell::new(0) };
        static RECORD_MILLIS: Cell<u64> = const { Cell::new(0) };
        static STATEMENTS: Cell<usize> = const { Cell::new(0) };
    }

    fn trace(event: rusqlite::trace::TraceEvent<'_>) {
        if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
            STATEMENTS.set(STATEMENTS.get() + 1);
            if statement.sql() == "COMMIT" {
                MILLIS.set(MILLIS.get() + COMMIT_MILLIS.get());
            } else if statement.sql() == "BEGIN IMMEDIATE" {
                MILLIS.set(MILLIS.get() + BEGIN_MILLIS.get());
            }
        }
    }
    struct Clock;
    impl Environment for Clock {
        fn monotonic(&self) -> Duration {
            Duration::from_millis(MILLIS.get())
        }
        fn timestamp(&self) -> u64 {
            MILLIS.get()
        }
        fn wal_size(&self, _: &Connection) -> Result<u64, StoreError> {
            Ok(0)
        }
    }
    struct Records(Arc<AtomicU64>);
    impl JobExecutor for Records {
        fn execute(
            &mut self,
            _: &Transaction<'_>,
            job: &Job,
            budget: &mut UnitBudget<'_>,
        ) -> Result<UnitOutcome, StoreError> {
            let mut n: u64 = job.cursor.as_deref().unwrap_or("0").parse().unwrap();
            while budget.reserve(Record::Receipt, self.0.load(Ordering::SeqCst)) {
                n += 1;
                MILLIS.set(MILLIS.get() + RECORD_MILLIS.get());
            }
            Ok(UnitOutcome {
                cursor: Some(n.to_string()),
                ..Default::default()
            })
        }
    }
    fn request() -> Control {
        Control::Request(JobRequest {
            kind: "synthetic".into(),
            key: "scope".into(),
            format_version: 1,
            input_generation: "generation".into(),
            input_revision: 0,
        })
    }
    fn service(bytes: Arc<AtomicU64>, authorized: bool) -> MaintenanceService {
        MaintenanceService::new(
            Box::new(Records(bytes)),
            Box::new(move |_, _| Ok(authorized)),
            Box::new(Clock),
            Limits::default(),
        )
        .unwrap()
    }
    fn connection() -> Connection {
        MILLIS.set(0);
        COMMIT_MILLIS.set(0);
        BEGIN_MILLIS.set(0);
        RECORD_MILLIS.set(0);
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrate(&mut conn).unwrap();
        conn.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(trace),
        );
        conn
    }
    fn committed(step: Step) -> JobStatus {
        match step {
            Step::Committed(status) => status,
            other => panic!("expected commit, got {other:?}"),
        }
    }

    #[test]
    fn deleted_authorization_never_increments_failure_backoff() {
        struct Space;
        impl BackupEnvironment for Space {
            fn available_space(&self, _: &Path) -> Option<u64> {
                Some(u64::MAX)
            }
            fn timestamp(&self) -> u64 {
                100
            }
        }
        let root = tempfile::tempdir().unwrap();
        let mut owner =
            StoppedMaintenance::receipt_conversion(&root.path().join("live.sqlite3")).unwrap();
        let id = owner.request_receipt_conversion(10).unwrap().status.id;
        let backup = owner
            .backup_receipt_conversion(&Space, &AtomicBool::new(false), &mut || {})
            .unwrap();
        owner.control(Control::Pause(id)).unwrap();
        owner.delete_backup(backup.id, &Space).unwrap();
        owner.control(Control::Resume(id)).unwrap();
        let failures = owner.service.failures.clone();
        let reason = read_job(&owner.conn, id).unwrap().status.reason;
        assert_eq!(
            owner.step().unwrap(),
            Step::Deferred(PauseReason::AwaitingBackup)
        );
        assert_eq!(owner.service.failures, failures);
        assert_eq!(read_job(&owner.conn, id).unwrap().status.reason, reason);
    }

    #[test]
    fn statuses_keep_one_snapshot_across_id_and_job_reads() {
        thread_local! {
            static UPDATE: std::cell::RefCell<Option<Connection>> = const { std::cell::RefCell::new(None) };
        }
        fn change_after_ids(event: rusqlite::trace::TraceEvent<'_>) {
            if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
                if statement.sql().starts_with("SELECT kind,job_key") {
                    UPDATE.with(|update| {
                        if let Some(writer) = update.borrow_mut().take() {
                            writer.execute("DELETE FROM maintenance_jobs", []).unwrap();
                        }
                    });
                }
            }
        }
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        let conn = open_writer(&path).unwrap();
        conn.execute("INSERT INTO maintenance_jobs(kind,job_key,format_version,input_generation,input_revision) VALUES('receipt_conversion','synthetic',1,'synthetic',0)", []).unwrap();
        UPDATE.with(|update| *update.borrow_mut() = Some(Connection::open(&path).unwrap()));
        conn.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(change_after_ids),
        );
        assert_eq!(statuses(&conn).unwrap().len(), 1);
        assert!(statuses(&conn).unwrap().is_empty());
    }

    #[test]
    fn conversion_poisoned_backup_owner_is_durable_and_never_busy_retries() {
        struct Space;
        impl BackupEnvironment for Space {
            fn available_space(&self, _: &Path) -> Option<u64> {
                Some(u64::MAX)
            }
            fn timestamp(&self) -> u64 {
                200
            }
        }
        for entry in ["writer", "request", "backup"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("live.sqlite3");
            drop(open_writer(&path).unwrap());
            let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
            let id = owner.request_receipt_conversion(100).unwrap().status.id;
            if entry != "writer" {
                assert_eq!(
                    owner.step().unwrap(),
                    Step::Deferred(PauseReason::AwaitingBackup)
                );
            }
            let backups = owner.conversion.as_ref().unwrap().backups.clone();
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = backups.lock().unwrap();
                panic!("synthetic backup owner failure");
            }));
            assert!(panic.is_err());
            drop(backups);
            match entry {
                "writer" => assert!(matches!(owner.step(), Err(StoreError::BackupPoisoned))),
                "request" => assert_eq!(
                    owner.request_receipt_conversion(300),
                    Err(BackupError::OwnerPoisoned)
                ),
                "backup" => assert_eq!(
                    owner.backup_receipt_conversion(&Space, &AtomicBool::new(false), &mut || {}),
                    Err(BackupError::OwnerPoisoned)
                ),
                _ => unreachable!(),
            }
            let status = owner.control(Control::Status(id)).unwrap().status;
            assert_eq!(status.reason, Some(PauseReason::BackupPoisoned));
            let changes = owner.conn.total_changes();
            assert_eq!(owner.step().unwrap(), Step::Idle);
            assert_eq!(owner.conn.total_changes(), changes);
            owner.control(Control::AuthorizationChanged(id)).unwrap();
            assert_eq!(owner.step().unwrap(), Step::Idle);
            drop(owner);
            let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
            assert_eq!(owner.control(Control::Status(id)).unwrap().status, status);
            assert_eq!(owner.step().unwrap(), Step::Idle);
        }
    }

    #[test]
    fn conversion_stuck_stream_cursor_triggers_no_progress_guard() {
        let mut conn = connection();
        conn.execute("INSERT INTO stream_heads VALUES('synthetic',1)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO event_receipts VALUES('synthetic',1,?1)",
            ["ab".repeat(32)],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER stall BEFORE UPDATE OF legacy_cursor ON event_streams
            BEGIN SELECT RAISE(IGNORE); END;",
        )
        .unwrap();
        let mut service = MaintenanceService::new(
            Box::new(crate::receipts::ConversionExecutor),
            Box::new(|_, _| Ok(true)),
            Box::new(Clock),
            Limits::default(),
        )
        .unwrap();
        let id = service
            .control(
                &mut conn,
                Control::Request(JobRequest {
                    kind: "receipt_conversion".into(),
                    key: "synthetic".into(),
                    format_version: 1,
                    input_generation: "synthetic".into(),
                    input_revision: 0,
                }),
            )
            .unwrap()
            .status
            .id;
        committed(service.step(&mut conn, true, &|| false).unwrap());
        committed(service.step(&mut conn, true, &|| false).unwrap());
        assert!(matches!(
            service.step(&mut conn, true, &|| false),
            Err(StoreError::Maintenance)
        ));
        let status = service
            .control(&mut conn, Control::Status(id))
            .unwrap()
            .status;
        assert_eq!(status.reason, Some(PauseReason::InvalidUnit));
        assert_eq!(status.processed_records, 0);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM event_receipts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn maintenance_stopped_required_leaves_live_runnable_set() {
        let mut conn = connection();
        let mut service = service(Arc::new(AtomicU64::new(2 * 1024 * 1024)), true);
        let id = service.control(&mut conn, request()).unwrap().status.id;
        committed(service.step(&mut conn, false, &|| false).unwrap());
        assert_eq!(
            service.step(&mut conn, false, &|| false).unwrap(),
            Step::Deferred(PauseReason::StoppedRequired)
        );
        assert!(
            !service.has_runnable_jobs(),
            "stopped work must let the live writer idle"
        );
        assert_eq!(
            service
                .control(&mut conn, Control::Status(id))
                .unwrap()
                .status
                .reason,
            Some(PauseReason::StoppedRequired)
        );
        let mut reopened = super::tests::service(Arc::new(AtomicU64::new(2 * 1024 * 1024)), true);
        committed(reopened.step(&mut conn, true, &|| false).unwrap());
    }

    struct Fails(StoreError);
    impl JobExecutor for Fails {
        fn execute(
            &mut self,
            _: &Transaction<'_>,
            job: &Job,
            _: &mut UnitBudget<'_>,
        ) -> Result<UnitOutcome, StoreError> {
            if job.request.key == "healthy" {
                return Ok(UnitOutcome {
                    phase_complete: true,
                    ..Default::default()
                });
            }
            Err(self.0.clone())
        }
    }

    #[test]
    fn maintenance_real_failure_backs_off_and_does_not_rewrite_same_error() {
        for error in [StoreError::Maintenance, StoreError::Full, StoreError::Io] {
            let mut conn = connection();
            let mut service = MaintenanceService::new(
                Box::new(Fails(error)),
                Box::new(|_, _| Ok(true)),
                Box::new(Clock),
                Limits::default(),
            )
            .unwrap();
            service.control(&mut conn, request()).unwrap();
            committed(service.step(&mut conn, true, &|| false).unwrap());
            assert!(service.step(&mut conn, true, &|| false).is_err());
            let changes = conn.total_changes();
            MILLIS.set(1_000);
            assert_eq!(
                service.step(&mut conn, true, &|| false).unwrap(),
                Step::Idle,
                "real failures need per-job backoff"
            );
            MILLIS.set(5_000);
            assert!(service.step(&mut conn, true, &|| false).is_err());
            assert_eq!(
                conn.total_changes(),
                changes,
                "unchanged failure must not rewrite its annotation"
            );
            MILLIS.set(10_000);
            assert_eq!(
                service.step(&mut conn, true, &|| false).unwrap(),
                Step::Idle,
                "repeated failure increases the delay"
            );
            let Control::Request(mut healthy) = request() else {
                unreachable!()
            };
            healthy.key = "healthy".into();
            let id = service
                .control(&mut conn, Control::Request(healthy))
                .unwrap()
                .status
                .id;
            assert_eq!(
                committed(service.step(&mut conn, true, &|| false).unwrap()).id,
                id
            );
            assert_eq!(
                committed(service.step(&mut conn, true, &|| false).unwrap()).id,
                id
            );
        }
    }

    #[test]
    fn maintenance_slow_commits_do_not_shrink_cheap_batches() {
        let mut conn = connection();
        let mut service = service(Arc::new(AtomicU64::new(1)), true);
        service.control(&mut conn, request()).unwrap();
        let mut before = committed(service.step(&mut conn, false, &|| false).unwrap());
        COMMIT_MILLIS.set(25);
        for _ in 0..24 {
            let started = MILLIS.get();
            let after = committed(service.step(&mut conn, false, &|| false).unwrap());
            assert_eq!(
                MILLIS.get() - started,
                25,
                "simulate a slow commit every turn"
            );
            assert_eq!(after.processed_records - before.processed_records, 1000);
            before = after;
        }
        // A cheap record still consumes the original turn deadline. Excluding
        // commit time from adaptation must not relax admission within a turn.
        RECORD_MILLIS.set(1);
        let after = committed(service.step(&mut conn, false, &|| false).unwrap());
        assert_eq!(after.processed_records - before.processed_records, 10);
    }

    #[test]
    fn maintenance_work_overruns_still_admit_a_large_record_live_and_recover_with_slow_commits() {
        let mut conn = connection();
        let bytes = Arc::new(AtomicU64::new(1));
        let mut service = service(bytes.clone(), true);
        service.control(&mut conn, request()).unwrap();
        committed(service.step(&mut conn, false, &|| false).unwrap());
        COMMIT_MILLIS.set(11);
        RECORD_MILLIS.set(11);
        for _ in 0..24 {
            committed(service.step(&mut conn, false, &|| false).unwrap());
        }
        assert_eq!(service.limits.receipts, 1);
        assert_eq!(service.limits.fights, 1);
        bytes.store(600 * 1024, Ordering::SeqCst);
        let before = committed(service.step(&mut conn, false, &|| false).unwrap());
        let after = committed(service.step(&mut conn, false, &|| false).unwrap());
        assert_eq!(after.processed_records - before.processed_records, 1);
        assert_eq!(after.processed_bytes - before.processed_bytes, 600 * 1024);
        // Work becomes cheap again, but FULL-sync commits remain slow.
        RECORD_MILLIS.set(0);
        bytes.store(1, Ordering::SeqCst);
        for _ in 0..24 {
            committed(service.step(&mut conn, false, &|| false).unwrap());
        }
        let recovered = committed(service.step(&mut conn, false, &|| false).unwrap());
        let next = committed(service.step(&mut conn, false, &|| false).unwrap());
        assert_eq!(next.processed_records - recovered.processed_records, 1000);
        bytes.store(512 * 1024, Ordering::SeqCst);
        let recovered_bytes = committed(service.step(&mut conn, false, &|| false).unwrap());
        assert_eq!(
            recovered_bytes.processed_records - next.processed_records,
            2
        );
        bytes.store(1024 * 1024 + 1, Ordering::SeqCst);
        assert_eq!(
            service.step(&mut conn, false, &|| false).unwrap(),
            Step::Deferred(PauseReason::StoppedRequired)
        );
    }

    #[test]
    fn maintenance_idle_and_awaiting_authorization_do_no_sql_or_timeout_changes() {
        let mut conn = connection();
        let mut service = service(Arc::new(AtomicU64::new(1)), false);
        assert_eq!(
            service.step(&mut conn, false, &|| false).unwrap(),
            Step::Idle
        );
        for waiting in [false, true] {
            if waiting {
                service.control(&mut conn, request()).unwrap();
                assert_eq!(
                    service.step(&mut conn, false, &|| false).unwrap(),
                    Step::Deferred(PauseReason::AwaitingBackup)
                );
            }
            conn.busy_timeout(Duration::from_millis(17)).unwrap();
            STATEMENTS.set(0);
            for _ in 0..20 {
                service.step(&mut conn, false, &|| false).unwrap();
            }
            assert_eq!(STATEMENTS.get(), 0, "idle polling must not execute SQL");
            assert_eq!(
                conn.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, u32>(0))
                    .unwrap(),
                17
            );
        }
    }

    #[test]
    fn maintenance_slow_begin_admits_first_live_record_but_capture_still_wins() {
        let mut conn = connection();
        let mut service = service(Arc::new(AtomicU64::new(600 * 1024)), true);
        service.control(&mut conn, request()).unwrap();
        committed(service.step(&mut conn, false, &|| false).unwrap());
        BEGIN_MILLIS.set(10);
        assert_eq!(
            committed(service.step(&mut conn, false, &|| false).unwrap()).processed_records,
            1
        );
        assert_eq!(
            service.step(&mut conn, false, &|| true).unwrap(),
            Step::Deferred(PauseReason::Capture)
        );
    }
}

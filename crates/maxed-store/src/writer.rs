use crate::{
    migrate::{digest, open_writer},
    project::{apply_contexts, json, validate_event},
    StoreError,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use maxed_types::*;
use rusqlite::{Connection, OptionalExtension};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct WriterConfig {
    pub stream_id: String,
    pub capacity: usize,
    pub batch_size: usize,
    pub flush_millis: u64,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            stream_id: "live".into(),
            capacity: 1024,
            batch_size: 128,
            flush_millis: 250,
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Ownership is deliberately returned intact by this frozen API.
pub enum SubmitError {
    Invalid(EventEnvelope),
    Full(EventEnvelope),
    Failed(EventEnvelope),
    Closed(EventEnvelope),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlushReceipt {
    pub stream_id: String,
    pub sequence: Sequence,
}

struct Pending {
    event: Arc<EventEnvelope>,
    context: Arc<crate::AlertContext>,
    bytes: usize,
    ticket: u64,
}
struct Shared {
    queue: VecDeque<Pending>,
    bytes: usize,
    last_ticket: u64,
    urgent_through: u64,
    committed: Sender<()>,
    state: StoreState,
    durable: Option<Sequence>,
    failure: Option<StoreError>,
    accepting: bool,
}
type Reply = Sender<Result<FlushReceipt, StoreError>>;
enum Command {
    Chat(
        crate::chat::ChatMaintenance,
        Sender<Result<u64, StoreError>>,
    ),
    Claim(u16, Sender<Result<Vec<crate::CommittedAlert>, StoreError>>),
    Flush(u64, Reply),
    Retry(u64, Reply),
    Shutdown(u64, Reply),
    #[cfg(test)]
    Inject(crate::project::fault::Point, i32, Sender<()>),
    #[cfg(test)]
    Panic,
}

/// Own on a background orchestration thread. Explicit shutdown is required to
/// receive unsaved events on failure; dropping without shutdown logs an error.
pub struct StoreWriter {
    config: WriterConfig,
    shared: Arc<Mutex<Shared>>,
    commands: Sender<Command>,
    wake: Sender<()>,
    committed: Receiver<()>,
    worker: Option<JoinHandle<()>>,
    _lease: WriterLease,
}

struct WriterLease(PathBuf);
fn writer_paths() -> &'static Mutex<std::collections::HashSet<PathBuf>> {
    static PATHS: std::sync::OnceLock<Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    PATHS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}
impl WriterLease {
    fn acquire(path: &std::path::Path) -> Result<Self, StoreError> {
        let path = std::fs::canonicalize(path).map_err(|_| StoreError::Open)?;
        if !writer_paths()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(path.clone())
        {
            return Err(StoreError::Busy);
        }
        Ok(Self(path))
    }
}
impl Drop for WriterLease {
    fn drop(&mut self) {
        writer_paths()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub fn start_writer(path: PathBuf, config: WriterConfig) -> Result<StoreWriter, StoreError> {
    if config.stream_id.is_empty()
        || config.capacity == 0
        || config.capacity > 1024
        || config.batch_size == 0
        || config.batch_size > 128
        || config.flush_millis == 0
        || config.flush_millis > 60_000
    {
        return Err(StoreError::InvalidEvent);
    }
    let mut conn = open_writer(&path)?;
    let lease = WriterLease::acquire(&path)?;
    recover_interrupted(&mut conn)?;
    conn.execute("UPDATE alert_occurrences SET state='suppressed' WHERE state='pending' AND source_stream_id<>?1", [&config.stream_id])?;
    let durable = conn
        .query_row(
            "SELECT sequence FROM stream_heads WHERE stream_id=?1",
            [&config.stream_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .map(|n| {
            u64::try_from(n)
                .map(|n| Sequence(Timestamp(n)))
                .map_err(|_| StoreError::InvalidEvent)
        })
        .transpose()?;
    let (commit_wake, committed) = bounded(1);
    let shared = Arc::new(Mutex::new(Shared {
        queue: VecDeque::new(),
        bytes: 0,
        last_ticket: 0,
        urgent_through: 0,
        committed: commit_wake,
        state: StoreState::Ready,
        durable,
        failure: None,
        accepting: true,
    }));
    let (commands, command_rx) = bounded(8);
    let (wake, wake_rx) = bounded(1);
    let worker_shared = Arc::clone(&shared);
    let worker_config = config.clone();
    let worker = thread::Builder::new()
        .name("maxed-summary-writer".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                worker_loop(
                    &mut conn,
                    &worker_config,
                    &worker_shared,
                    command_rx,
                    wake_rx,
                )
            }));
            if result.is_err() {
                let mut shared = lock(&worker_shared);
                shared.state = StoreState::Failed;
                shared.failure = Some(StoreError::WorkerFailed { pending: vec![] });
                tracing::error!("Summary worker panicked; pending summaries remain retained");
            }
        })
        .map_err(|_| StoreError::WorkerFailed { pending: vec![] })?;
    Ok(StoreWriter {
        config,
        shared,
        commands,
        wake,
        committed,
        worker: Some(worker),
        _lease: lease,
    })
}

impl StoreWriter {
    /// Coalesced durable-head wake for the single owning orchestration loop.
    /// Receivers share one permit; always read status after waking.
    pub fn committed(&self) -> Receiver<()> {
        self.committed.clone()
    }

    pub fn validate(&self, event: &EventEnvelope) -> Result<(), StoreError> {
        if event.stream_id != self.config.stream_id {
            return Err(StoreError::InvalidEvent);
        }
        validate_event(event).map(|_| ())
    }

    #[allow(clippy::result_large_err)] // The required API returns ownership of the original event.
    pub fn try_submit(&self, event: EventEnvelope) -> Result<(), SubmitError> {
        self.try_submit_with_alert_context(event, crate::AlertContext::default())
    }

    #[allow(clippy::result_large_err)]
    pub fn try_submit_with_alert_context(
        &self,
        event: EventEnvelope,
        context: crate::AlertContext,
    ) -> Result<(), SubmitError> {
        let context_bytes = match context.validate() {
            Ok(n) => n,
            Err(_) => return Err(SubmitError::Invalid(event)),
        };
        if event.stream_id != self.config.stream_id {
            return Err(SubmitError::Invalid(event));
        }
        let bytes = match validate_event(&event) {
            Ok((bytes, _)) => bytes.saturating_add(context_bytes),
            Err(_) => return Err(SubmitError::Invalid(event)),
        };
        // Never wait for the worker, another producer, or SQLite on the submit path.
        let mut shared = match self.shared.try_lock() {
            Ok(shared) => shared,
            Err(std::sync::TryLockError::WouldBlock) => return Err(SubmitError::Full(event)),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(SubmitError::Failed(event)),
        };
        if !shared.accepting {
            return Err(SubmitError::Closed(event));
        }
        if shared.failure.is_some() {
            return Err(SubmitError::Failed(event));
        }
        if shared.queue.len() >= self.config.capacity
            || shared.bytes.saturating_add(bytes) > MAX_QUEUED_BYTES
        {
            shared.state = StoreState::Backpressured;
            return Err(SubmitError::Full(event));
        }
        let Some(ticket) = shared.last_ticket.checked_add(1) else {
            return Err(SubmitError::Invalid(event));
        };
        shared.last_ticket = ticket;
        if context.needs_prompt_commit(&event.event) {
            shared.urgent_through = ticket;
        }
        shared.bytes += bytes;
        shared.queue.push_back(Pending {
            event: Arc::new(event),
            context: Arc::new(context),
            bytes,
            ticket,
        });
        drop(shared);
        let _ = self.wake.try_send(());
        Ok(())
    }

    /// Flushes accepted events first so a clear covers everything submitted before it.
    /// The caller restricts maintenance to the live namespace. Returns the largest
    /// delete count across chat, alert and occurrence tables; repeat full batches.
    pub fn maintain_chat(&self, op: crate::chat::ChatMaintenance) -> Result<u64, StoreError> {
        self.flush()?;
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::Chat(op, send))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    pub fn claim_alerts(&self, limit: u16) -> Result<Vec<crate::CommittedAlert>, StoreError> {
        if !(1..=32).contains(&limit) {
            return Err(StoreError::Limit);
        }
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::Claim(limit, send))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    pub fn status(&self) -> StoreStatus {
        let shared = lock(&self.shared);
        let message = if let Some(failure) = &shared.failure {
            match (shared.queue.front(), shared.queue.back()) {
                (Some(first), Some(last)) => format!(
                    "{failure}; unacknowledged sequence range {}..={}",
                    first.event.sequence.0 .0, last.event.sequence.0 .0
                ),
                _ => failure.to_string(),
            }
        } else {
            match shared.state {
                StoreState::Ready => "Summary writer ready",
                StoreState::Backpressured => {
                    "Summary queue full; retain and retry the returned event"
                }
                StoreState::Closing => "Saving pending summaries",
                StoreState::Closed => "Summary writer closed",
                StoreState::Failed => "Summary writer failed; pending summaries are not saved",
            }
            .into()
        };
        StoreStatus {
            state: shared.state.clone(),
            queued: shared.queue.len() as u32,
            durable: shared.durable,
            stream_id: Some(self.config.stream_id.clone()),
            message,
        }
    }

    fn request(&self, retry: bool) -> Result<FlushReceipt, StoreError> {
        let shared = lock(&self.shared);
        if !shared.accepting {
            return Err(StoreError::Closed);
        }
        let ticket = shared.last_ticket;
        drop(shared);
        let (reply, response) = bounded(1);
        let command = if retry {
            Command::Retry(ticket, reply)
        } else {
            Command::Flush(ticket, reply)
        };
        self.commands
            .send(command)
            .map_err(|_| self.worker_failed())?;
        wait_response(response, self.worker.as_ref().ok_or(StoreError::Closed)?).map_err(|error| {
            match error {
                StoreError::WorkerFailed { .. } => self.worker_failed(),
                other => other,
            }
        })
    }

    fn worker_failed(&self) -> StoreError {
        // A non-consuming request cannot transfer the queue. Return a recovery
        // copy while keeping shutdown's ownership-preserving path intact.
        StoreError::WorkerFailed {
            pending: lock(&self.shared)
                .queue
                .iter()
                .map(|p| (*p.event).clone())
                .collect(),
        }
    }

    pub fn flush(&self) -> Result<FlushReceipt, StoreError> {
        self.request(false)
    }
    pub fn retry(&self) -> Result<(), StoreError> {
        self.request(true).map(|_| ())
    }

    /// Context-preserving recovery for adapters that supplied explicit policies.
    pub fn shutdown_with_alert_context(
        mut self,
    ) -> Result<FlushReceipt, crate::ContextualShutdownError> {
        self.stop().map_err(|error| {
            let pending = lock(&self.shared)
                .queue
                .drain(..)
                .map(|p| crate::ContextualEvent {
                    event: Arc::unwrap_or_clone(p.event),
                    context: Arc::unwrap_or_clone(p.context),
                })
                .collect();
            crate::ContextualShutdownError { error, pending }
        })
    }

    pub fn shutdown(mut self) -> Result<FlushReceipt, StoreError> {
        let result = self.stop();
        if result.is_err() {
            let pending = lock(&self.shared)
                .queue
                .drain(..)
                .map(|p| Arc::unwrap_or_clone(p.event))
                .collect();
            return Err(StoreError::WorkerFailed { pending });
        }
        result
    }

    fn stop(&mut self) -> Result<FlushReceipt, StoreError> {
        let Some(worker) = self.worker.take() else {
            return Err(StoreError::Closed);
        };
        let mut shared = lock(&self.shared);
        shared.accepting = false;
        if shared.failure.is_none() {
            shared.state = StoreState::Closing;
        }
        let ticket = shared.last_ticket;
        drop(shared);
        let (reply, response) = bounded(1);
        let result = if self.commands.send(Command::Shutdown(ticket, reply)).is_ok() {
            wait_response(response, &worker)
        } else {
            Err(StoreError::WorkerFailed { pending: vec![] })
        };
        if worker.join().is_err() {
            return Err(StoreError::WorkerFailed { pending: vec![] });
        }
        result
    }
}

fn wait_response<T>(
    response: Receiver<Result<T, StoreError>>,
    worker: &JoinHandle<()>,
) -> Result<T, StoreError> {
    wait_response_until(response, || worker.is_finished())
}

fn wait_response_until<T>(
    response: Receiver<Result<T, StoreError>>,
    is_finished: impl Fn() -> bool,
) -> Result<T, StoreError> {
    loop {
        match response.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => return result,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(StoreError::WorkerFailed { pending: vec![] })
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) if is_finished() => {
                return response
                    .try_recv()
                    .unwrap_or_else(|_| Err(StoreError::WorkerFailed { pending: vec![] }))
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
}

impl Drop for StoreWriter {
    fn drop(&mut self) {
        if self.worker.is_some() && self.stop().is_err() {
            // Rust Drop cannot return ownership. Keep failed data for process
            // lifetime rather than silently destroying accepted summaries.
            retain_abandoned(Arc::clone(&self.shared));
            tracing::error!("Writer dropped without successful shutdown; unsaved summaries retained for process lifetime");
        }
    }
}

fn abandoned() -> &'static Mutex<Vec<Arc<Mutex<Shared>>>> {
    static ABANDONED: std::sync::OnceLock<Mutex<Vec<Arc<Mutex<Shared>>>>> =
        std::sync::OnceLock::new();
    ABANDONED.get_or_init(|| Mutex::new(Vec::new()))
}
fn retain_abandoned(shared: Arc<Mutex<Shared>>) {
    abandoned()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(shared);
}

/// Recover events retained by writers dropped without explicit shutdown.
/// Normal callers should instead handle `shutdown`'s `WorkerFailed.pending`.
pub fn take_abandoned_events() -> Vec<EventEnvelope> {
    let retained = std::mem::take(&mut *abandoned().lock().unwrap_or_else(|p| p.into_inner()));
    retained
        .into_iter()
        .flat_map(|shared| {
            lock(&shared)
                .queue
                .drain(..)
                .map(|p| Arc::unwrap_or_clone(p.event))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn receipt(config: &WriterConfig, shared: &Mutex<Shared>) -> FlushReceipt {
    FlushReceipt {
        stream_id: config.stream_id.clone(),
        sequence: lock(shared).durable.unwrap_or(Sequence(Timestamp(0))),
    }
}

fn mark_failed(shared: &Mutex<Shared>, error: &StoreError) {
    let mut shared = lock(shared);
    shared.state = StoreState::Failed;
    shared.failure = Some(error.clone());
}

fn commit_next(
    conn: &mut Connection,
    config: &WriterConfig,
    shared: &Mutex<Shared>,
    through: u64,
) -> Result<bool, StoreError> {
    let events: Vec<_> = lock(shared)
        .queue
        .iter()
        .take_while(|p| p.ticket <= through)
        .take(config.batch_size)
        .map(|p| (Arc::clone(&p.event), Arc::clone(&p.context)))
        .collect();
    if events.is_empty() {
        return Ok(false);
    }
    if let Err(error) = apply_contexts(conn, events.iter().map(|(e, c)| (e.as_ref(), c.as_ref()))) {
        mark_failed(shared, &error);
        return Err(error);
    }
    let mut shared = lock(shared);
    for (event, _) in &events {
        if let Some(pending) = shared.queue.pop_front() {
            shared.bytes -= pending.bytes;
        }
        let previous = shared.durable.map_or(0, |s| s.0 .0);
        shared.durable = Some(Sequence(Timestamp(previous.max(event.sequence.0 .0))));
    }
    shared.state = if shared.accepting {
        StoreState::Ready
    } else {
        StoreState::Closing
    };
    let _ = shared.committed.try_send(());
    Ok(true)
}

fn commit_ready(state: &Shared, config: &WriterConfig, elapsed: Duration) -> bool {
    state.failure.is_none()
        && !state.queue.is_empty()
        && (state.queue.len() >= config.batch_size
            || state
                .queue
                .front()
                .is_some_and(|p| p.ticket <= state.urgent_through)
            || elapsed >= Duration::from_millis(config.flush_millis))
}

fn flush_through(
    conn: &mut Connection,
    config: &WriterConfig,
    shared: &Mutex<Shared>,
    through: u64,
) -> Result<FlushReceipt, StoreError> {
    if let Some(error) = &lock(shared).failure {
        return Err(error.clone());
    }
    while commit_next(conn, config, shared, through)? {}
    // PASSIVE never waits for readers or removes history. WAL commits remain
    // durable even if a reader pins pages and prevents a full checkpoint.
    conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")
        .inspect_err(|e| mark_failed(shared, &StoreError::from_sqlite_ref(e)))?;
    Ok(receipt(config, shared))
}

impl StoreError {
    fn from_sqlite_ref(error: &rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                Self::Busy
            }
            Some(rusqlite::ErrorCode::DiskFull) => Self::Full,
            _ => Self::Io,
        }
    }
}

fn worker_loop(
    conn: &mut Connection,
    config: &WriterConfig,
    shared: &Mutex<Shared>,
    commands: Receiver<Command>,
    wake: Receiver<()>,
) {
    let interval = Duration::from_millis(config.flush_millis);
    let mut last_flush = Instant::now();
    loop {
        let ready = {
            let state = lock(shared);
            commit_ready(&state, config, last_flush.elapsed())
        };
        let wait = if ready {
            Duration::ZERO
        } else {
            interval.saturating_sub(last_flush.elapsed())
        };
        crossbeam_channel::select! {
            recv(commands) -> command => {
                let Ok(command) = command else { break; };
                let (through,reply,closing,retry) = match command {
                    Command::Chat(op, reply) => { let _ = reply.send(crate::chat::maintain(conn, op)); continue; }
                    Command::Claim(limit, reply) => { let result = crate::alerts::claim(conn, &config.stream_id, limit); let _ = reply.send(result); continue; }
                    Command::Flush(t,r) => (t,r,false,false), Command::Retry(t,r) => (t,r,false,true), Command::Shutdown(t,r) => (t,r,true,false),
                    #[cfg(test)]
                    Command::Inject(point,code,reply) => { crate::project::fault::arm(point,code); let _=reply.send(()); continue; }
                    #[cfg(test)]
                    Command::Panic => panic!("synthetic worker failure"),
                };
                if retry { let mut state=lock(shared); state.failure=None; state.state=StoreState::Ready; }
                let result=flush_through(conn,config,shared,through);
                if closing && result.is_ok() { lock(shared).state=StoreState::Closed; }
                let _=reply.send(result); last_flush=Instant::now();
                if closing { break; }
            }
            recv(wake) -> _ => {}
            default(wait) => {}
        }
        let ready = {
            let state = lock(shared);
            commit_ready(&state, config, last_flush.elapsed())
        };
        if ready {
            let _ = commit_next(conn, config, shared, u64::MAX);
            last_flush = Instant::now();
        }
        if last_flush.elapsed() >= interval {
            last_flush = Instant::now();
        }
    }
}

fn recover_interrupted(conn: &mut Connection) -> Result<(), StoreError> {
    let sessions = {
        let mut statement=conn.prepare("SELECT json FROM play_sessions WHERE ended IS NULL OR id IN (SELECT session_id FROM runs WHERE ended IS NULL UNION SELECT session_id FROM fights WHERE ended IS NULL UNION SELECT session_id FROM map_visits WHERE ended IS NULL) ORDER BY id")?;
        let strings = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        strings
            .into_iter()
            .map(|s| serde_json::from_str::<PlaySessionView>(&s).map_err(StoreError::from))
            .collect::<Result<Vec<_>, _>>()?
    };
    for mut session in sessions {
        let runs: Vec<RunView> = read_json_rows(
            conn,
            "SELECT json FROM runs WHERE session_id=?1 AND ended IS NULL ORDER BY id",
            &session.id,
        )?;
        let fights: Vec<FightView> = read_json_rows(
            conn,
            "SELECT json FROM fights WHERE session_id=?1 AND ended IS NULL ORDER BY id",
            &session.id,
        )?;
        let stream = format!(
            "store-recovery-{}",
            digest(json(&(&session, &runs, &fights))?.as_bytes())
        );
        let at = Timestamp(
            runs.iter()
                .flat_map(|r| [Some(r.started), r.completed_at, r.died_at])
                .flatten()
                .chain(
                    fights
                        .iter()
                        .flat_map(|f| [Some(f.started), f.last_hit])
                        .flatten(),
                )
                .map(|t| t.0)
                .max()
                .unwrap_or(0)
                .max(session.last_traffic.0)
                .max(session.started.0),
        );
        session.ended = Some(
            session
                .ended
                .map_or(at, |ended| Timestamp(ended.0.max(at.0))),
        );
        let mut events = vec![EngineEvent::SessionUpdated(session)];
        for mut run in runs {
            run.ended = Some(at);
            run.complete_capture = false;
            if !matches!(run.outcome, Some(Outcome::Cleared | Outcome::Died)) {
                run.outcome = Some(Outcome::Unknown);
            }
            run.evidence.push(EvidenceAt {
                at,
                evidence: Evidence::AppClosed,
            });
            if !run.issues.contains(&CoverageIssue::StreamGap) {
                run.issues.push(CoverageIssue::StreamGap);
            }
            events.push(EngineEvent::RunEnded(run));
        }
        for mut fight in fights {
            fight.ended = Some(at);
            fight.end_reason = Some(FightEnd::AppClosed);
            fight.complete_capture = false;
            if !fight.issues.contains(&CoverageIssue::StreamGap) {
                fight.issues.push(CoverageIssue::StreamGap);
            }
            events.push(EngineEvent::FightEnded(fight));
        }
        let envelopes: Vec<_> = events
            .into_iter()
            .enumerate()
            .map(|(i, event)| EventEnvelope {
                fight_chart_omitted: false,
                stream_id: stream.clone(),
                sequence: Sequence(Timestamp(i as u64 + 1)),
                at,
                event,
            })
            .collect();
        for batch in envelopes.chunks(128) {
            crate::apply_batch(conn, batch)?;
        }
    }
    Ok(())
}

fn read_json_rows<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    id: &str,
) -> Result<Vec<T>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map([id], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::fault::Point;

    #[test]
    fn commit_schedule_uses_urgent_ticket_without_advancing_batch_clock() {
        let config = WriterConfig::default();
        let (committed, _) = bounded(1);
        let mut state = Shared {
            queue: VecDeque::new(),
            bytes: 0,
            last_ticket: 3,
            urgent_through: 0,
            committed,
            state: StoreState::Ready,
            durable: None,
            failure: None,
            accepting: true,
        };
        for ticket in 1..=3 {
            state.queue.push_back(Pending {
                event: Arc::new(event(ticket)),
                context: Arc::new(crate::AlertContext::default()),
                bytes: 0,
                ticket,
            });
        }
        let before_tick = Duration::from_millis(249);
        assert!(!commit_ready(&state, &config, Duration::ZERO));
        assert!(!commit_ready(&state, &config, before_tick));
        assert!(commit_ready(&state, &config, Duration::from_millis(250)));
        state.urgent_through = 2;
        assert!(commit_ready(&state, &config, Duration::ZERO));
        state.queue.pop_front();
        assert!(commit_ready(&state, &config, Duration::ZERO));
        state.queue.pop_front();
        assert!(!commit_ready(&state, &config, Duration::ZERO));
        assert!(!commit_ready(&state, &config, before_tick));
        state.urgent_through = 3;
        state.failure = Some(StoreError::Full);
        assert!(!commit_ready(&state, &config, Duration::ZERO));
    }

    #[test]
    fn only_visible_alert_candidates_request_prompt_commit() {
        let mut context = crate::AlertContext {
            chat: None,
            live: false,
            preferences: AlertPreferences::default(),
            key_pop_dungeon: None,
            bag_window: None,
        };
        let death = EngineEvent::PlayerDied {
            at: Timestamp(1),
            character_id: None,
            run_id: None,
        };
        let mut bag = demo::demo_snapshot().drops.remove(0);
        bag.bag_rank = Some(255);
        let white = EngineEvent::BagDropped(bag.clone());
        let updated = EngineEvent::DropUpdated(bag.clone());
        let fight = EngineEvent::FightEnded(crate::alerts_workload::eligible_fight());
        let finalized = EngineEvent::FightUpdated(crate::alerts_workload::eligible_fight());
        assert!(!context.needs_prompt_commit(&death));
        for candidate in [&white, &updated, &fight, &finalized] {
            assert!(context.needs_prompt_commit(candidate));
        }
        context.preferences = AlertPreferences {
            death: AlertLoudness::QuietLogOnly,
            white_bag: AlertLoudness::Off,
            personal_best: AlertLoudness::Off,
            ..Default::default()
        };
        for candidate in [&death, &white, &updated, &fight, &finalized] {
            assert!(!context.needs_prompt_commit(candidate));
        }
        context.preferences = AlertPreferences::default();
        bag.bag_rank = Some(1);
        assert!(!context.needs_prompt_commit(&EngineEvent::BagDropped(bag)));
        assert!(!context.needs_prompt_commit(&event(1).event));
        let mut active = crate::alerts_workload::eligible_fight();
        active.ended = None;
        assert!(!context.needs_prompt_commit(&EngineEvent::FightUpdated(active)));
        let run = EngineEvent::RunEnded(demo::demo_run_story().run);
        assert!(!context.needs_prompt_commit(&run));
        context.live = true;
        assert!(context.needs_prompt_commit(&run));
    }

    #[test]
    fn commit_wake_follows_durability_and_coalesces_without_flushing_normal_work() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt.sqlite3");
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
        )
        .unwrap();
        let committed = writer.committed();
        submit(&writer, event(1));
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        assert!(writer.status().durable.is_none());
        assert!(committed.try_recv().is_err());
        let mut source = event(2);
        let mut bag = demo::demo_snapshot().drops.remove(0);
        bag.run_id = None;
        bag.session_id = "session".into();
        bag.bag_rank = Some(255);
        source.event = EngineEvent::BagDropped(bag);
        writer
            .try_submit_with_alert_context(
                source,
                crate::AlertContext {
                    chat: None,
                    live: false,
                    preferences: AlertPreferences::default(),
                    key_pop_dungeon: None,
                    bag_window: None,
                },
            )
            .unwrap();
        committed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(2))));
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM alert_occurrences WHERE state='pending'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(writer.claim_alerts(32).unwrap().len(), 1);
        for sequence in 3..=4 {
            submit(&writer, event(sequence));
            writer.flush().unwrap();
        }
        // A second commit cannot block the writer behind an unconsumed permit.
        assert_eq!(committed.len(), 1);
        committed.recv().unwrap();
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(4))));
        writer.shutdown().unwrap();
    }

    #[test]
    fn failed_urgent_commit_never_signals_or_claims_until_retry() {
        let dir = tempfile::tempdir().unwrap();
        let writer = start_writer(
            dir.path().join("failed.sqlite3"),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
        )
        .unwrap();
        let committed = writer.committed();
        submit(&writer, event(1));
        writer.flush().unwrap();
        committed.recv().unwrap();
        let (reply, response) = bounded(1);
        writer
            .commands
            .send(Command::Inject(
                Point::Commit,
                rusqlite::ffi::SQLITE_FULL,
                reply,
            ))
            .unwrap();
        response.recv().unwrap();
        let mut source = event(2);
        let mut bag = demo::demo_snapshot().drops.remove(0);
        bag.run_id = None;
        bag.session_id = "session".into();
        bag.bag_rank = Some(255);
        source.event = EngineEvent::BagDropped(bag);
        writer
            .try_submit_with_alert_context(
                source,
                crate::AlertContext {
                    chat: None,
                    live: false,
                    preferences: AlertPreferences::default(),
                    key_pop_dungeon: None,
                    bag_window: None,
                },
            )
            .unwrap();
        assert!(writer.flush().is_err());
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(1))));
        assert!(committed.try_recv().is_err());
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        writer.retry().unwrap();
        committed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(writer.claim_alerts(32).unwrap().len(), 1);
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        writer.shutdown().unwrap();
    }

    #[test]
    fn clear_chat_flushes_accepted_events_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.sqlite3");
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
        )
        .unwrap();
        writer.try_submit(event(1)).unwrap();
        let mut source = event(2);
        source.event = EngineEvent::Chat(ChatMessageView {
            id: "fault-test:chat:2".into(),
            session_id: "session".into(),
            run_id: None,
            map: None,
            at: Timestamp(1),
            channel: ChatChannel::Public,
            direction: None,
            sender_kind: ChatSenderKind::Player,
            sender: "SynthAlpha".into(),
            counterpart: None,
            is_local: false,
            stars: None,
            supporter: false,
            text: "synthetic message".into(),
            truncated: false,
        });
        writer.try_submit(source).unwrap();
        assert_eq!(
            writer.maintain_chat(crate::ChatMaintenance::Clear).unwrap(),
            1
        );
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(2))));
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM chat_messages", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        writer.shutdown().unwrap();
    }

    #[test]
    fn response_arriving_between_timeout_and_worker_exit_is_not_lost() {
        let (reply, response) = bounded(1);
        let expected = FlushReceipt {
            stream_id: "race-test".into(),
            sequence: Sequence(Timestamp(42)),
        };
        // Deterministically deliver the acknowledgment after recv_timeout but
        // before the finished check returns, exactly the shutdown race window.
        let actual = wait_response_until(response, || {
            reply.send(Ok(expected.clone())).unwrap();
            true
        })
        .unwrap();
        assert_eq!(actual, expected);
    }

    fn event(sequence: u64) -> EventEnvelope {
        EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "fault-test".into(),
            sequence: Sequence(Timestamp(sequence)),
            at: Timestamp(1),
            event: EngineEvent::SessionUpdated(PlaySessionView {
                issues: vec![],
                id: "session".into(),
                started: Timestamp(1),
                last_traffic: Timestamp(sequence),
                ended: None,
            }),
        }
    }

    fn submit(writer: &StoreWriter, mut event: EventEnvelope) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match writer.try_submit(event) {
                Ok(()) => break,
                Err(SubmitError::Full(returned)) if Instant::now() < deadline => {
                    event = returned;
                    thread::yield_now();
                }
                Err(error) => panic!("submit failed: {error:?}"),
            }
        }
    }

    #[test]
    fn boundary_fault_matrix_retains_exact_batch_and_retry_commits_once() {
        for point in [Point::Begin, Point::Commit] {
            for code in [
                rusqlite::ffi::SQLITE_FULL,
                rusqlite::ffi::SQLITE_READONLY,
                rusqlite::ffi::SQLITE_BUSY,
                rusqlite::ffi::SQLITE_IOERR,
            ] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("db");
                let writer = start_writer(
                    path.clone(),
                    WriterConfig {
                        stream_id: "fault-test".into(),
                        flush_millis: 60_000,
                        ..WriterConfig::default()
                    },
                )
                .unwrap();
                submit(&writer, event(1));
                writer.flush().unwrap();
                let (reply, response) = bounded(1);
                writer
                    .commands
                    .send(Command::Inject(point, code, reply))
                    .unwrap();
                response.recv().unwrap();
                submit(&writer, event(2));
                submit(&writer, event(3));
                let expected = StoreError::from(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(code),
                    None,
                ));
                for _ in 0..3 {
                    let error = writer.flush().unwrap_err();
                    assert_eq!(
                        std::mem::discriminant(&error),
                        std::mem::discriminant(&expected)
                    );
                    assert!(!writer.worker.as_ref().unwrap().is_finished());
                }
                let status = writer.status();
                assert_eq!(status.state, StoreState::Failed);
                assert_eq!(status.queued, 2);
                assert_eq!(status.durable, Some(Sequence(Timestamp(1))));
                assert!(status.message.contains("2..=3"));
                let db = Connection::open(&path).unwrap();
                assert_eq!(
                    db.query_row("SELECT count(*) FROM event_receipts", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                assert!(matches!(
                    writer.try_submit(event(4)),
                    Err(SubmitError::Failed(_))
                ));
                writer.retry().unwrap();
                assert_eq!(writer.flush().unwrap().sequence.0 .0, 3);
                assert_eq!(
                    db.query_row("SELECT count(*) FROM event_receipts", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    3
                );
                assert_eq!(writer.status().queued, 0);
                writer.shutdown().unwrap();
            }
        }
    }

    #[test]
    fn panic_exposes_unacknowledged_range_and_shutdown_returns_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let writer = start_writer(
            dir.path().join("db"),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..WriterConfig::default()
            },
        )
        .unwrap();
        submit(&writer, event(1));
        writer.commands.send(Command::Panic).unwrap();
        match writer.flush().unwrap_err() {
            StoreError::WorkerFailed { pending } => assert_eq!(pending, vec![event(1)]),
            error => panic!("wrong panic result: {error}"),
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while writer.status().state != StoreState::Failed && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(writer.status().message.contains("1..=1"));
        match writer.shutdown().unwrap_err() {
            StoreError::WorkerFailed { pending } => assert_eq!(pending, vec![event(1)]),
            e => panic!("{e}"),
        }
    }

    #[test]
    fn byte_budget_is_enforced_independently_of_event_count() {
        let dir = tempfile::tempdir().unwrap();
        let writer = start_writer(
            dir.path().join("db"),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..WriterConfig::default()
            },
        )
        .unwrap();
        // This summary text is synthetic; no packet/chat fixture is involved.
        for sequence in 1..=4 {
            let mut e = event(sequence);
            if let EngineEvent::SessionUpdated(s) = &mut e.event {
                s.id = "x".repeat(7 * 1024 * 1024);
            }
            submit(&writer, e);
        }
        let mut e = event(5);
        if let EngineEvent::SessionUpdated(s) = &mut e.event {
            s.id = "x".repeat(7 * 1024 * 1024);
        }
        assert!(matches!(writer.try_submit(e), Err(SubmitError::Full(_))));
        assert_eq!(writer.status().state, StoreState::Backpressured);
        assert_eq!(writer.status().queued, 4);
        // Prevent writing the large fixture; shutdown returns it intact.
        let (reply, response) = bounded(1);
        writer
            .commands
            .send(Command::Inject(
                Point::Begin,
                rusqlite::ffi::SQLITE_FULL,
                reply,
            ))
            .unwrap();
        response.recv().unwrap();
        match writer.shutdown().unwrap_err() {
            StoreError::WorkerFailed { pending } => assert_eq!(pending.len(), 4),
            e => panic!("{e}"),
        }
    }
}

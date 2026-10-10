//! One native history scheduler, shared by Home and every drill-down.
use super::{home::*, CaptureRuntime};
use maxed_types::*;
use std::{
    collections::VecDeque,
    sync::atomic::Ordering,
    sync::{mpsc, Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

pub(super) const INVALID: &str = "History query is invalid; select the view again";
pub(super) const LIMIT: &str = "Saved history exceeds the supported query size limit";
pub(super) const WORKER: &str = "History worker failed; retry";

#[derive(Clone, Copy)]
pub(super) enum HistoryCaller {
    MainWindow,
    #[cfg(test)]
    NativeTest,
}
impl HistoryCaller {
    fn index(self) -> usize {
        match self {
            Self::MainWindow => 0,
            #[cfg(test)]
            Self::NativeTest => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum HistoryKey {
    StatsMaps(StatsFilter),
    StatsSessions(StatsSessionFilter),
    StatsSession(StatsSessionRequest),
    StatsDungeons(StatsFilter),
    StatsExport(StatsExportRequest),
    Home(HomeScope),
    ScopeLoot(HomeScope, Option<String>, u16, Vec<(u16, Option<String>)>),
    DungeonBests,
    Runs(RunFilter),
    Drops(DropFilter),
    DropSummary(DropSummaryRequest, Vec<(u16, Option<String>)>),
    Chat(ChatFilter, Vec<String>, Vec<String>),
    ChatOverview(ChatOverviewRequest),
    ChatSender(ChatSenderRequest),
    WhisperPartners(WhisperPartnerFilter, Vec<String>, Vec<String>),
    Events(EventFilter),
    EventSummary(EventSummaryRequest),
    ItemHistory(ItemHistoryRequest),
    LootStats(LootStatsFilter),
    Collection(CollectionRequest, Vec<(u16, Option<String>)>),
    Alerts(AlertFilter),
    AlertSummary(AlertSummaryRequest),
    Run(String),
    RunFightStrip(String, Option<String>),
    Fight(String),
    Player(String, i32),
}
impl HistoryKey {
    #[cfg(test)]
    pub(super) fn validate_for_tests(&self) -> Result<(), String> {
        self.validate()
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        match self {
            Self::StatsMaps(filter) | Self::StatsDungeons(filter) => validate_stats(filter),
            Self::StatsSessions(filter) => {
                if !(1..=MAX_STATS_SESSION_PAGE).contains(&filter.limit) {
                    return Err(INVALID.into());
                }
                validate_loot_range(filter.range.as_ref())?;
                if let Some(character) = &filter.character_id {
                    identifier(character)?;
                }
                if let Some(cursor) = &filter.cursor {
                    identifier(&cursor.id)?;
                    if cursor.started.0 > i64::MAX as u64 {
                        return Err(INVALID.into());
                    }
                }
                Ok(())
            }
            Self::StatsSession(request) => {
                identifier(&request.id)?;
                request.character_id.as_deref().map_or(Ok(()), identifier)
            }
            Self::StatsExport(request) => {
                validate_stats(&request.filter)?;
                match &request.kind {
                    StatsExportKind::SessionRuns { id } => identifier(id),
                    _ => Ok(()),
                }
            }
            Self::Chat(filter, senders, keywords) => {
                if !(1..=MAX_LOGS_PAGE).contains(&filter.limit)
                    || filter.query.len() > 512
                    || filter.query.chars().count() > MAX_CHAT_QUERY_CHARS
                    || filter.query.contains('\0')
                    || filter
                        .sender
                        .as_ref()
                        .is_some_and(|s| s.is_empty() || s.len() > 256)
                    || senders.len() > MAX_HIDDEN_SENDERS
                    || keywords.len() > MAX_HIDDEN_KEYWORDS
                {
                    return Err(INVALID.into());
                }
                if let Some(run) = &filter.run_id {
                    identifier(run)?;
                }
                if let Some(cursor) = &filter.cursor {
                    identifier(&cursor.id)?;
                    if cursor.at.0 > i64::MAX as u64 {
                        return Err(INVALID.into());
                    }
                }
                Ok(())
            }
            Self::ChatOverview(request) => request.run_id.as_deref().map_or(Ok(()), identifier),
            Self::ChatSender(request) => {
                if request.name.is_empty()
                    || request.name.len() > MAX_HIDDEN_SENDER_BYTES
                    || request.name.chars().any(char::is_control)
                {
                    return Err(INVALID.into());
                }
                Ok(())
            }
            Self::WhisperPartners(filter, senders, keywords) => {
                let settings = ChatSettings {
                    hidden_senders: senders.clone(),
                    hidden_keywords: keywords.clone(),
                    ..ChatSettings::default()
                };
                if !(1..=MAX_WHISPER_PARTNERS).contains(&filter.limit)
                    || filter.query.len() > 512
                    || filter.query.chars().count() > MAX_CHAT_QUERY_CHARS
                    || filter.query.contains('\0')
                    || !settings.valid()
                {
                    return Err(INVALID.into());
                }
                Ok(())
            }
            Self::EventSummary(request) => validate_event_scope(&request.filter),
            Self::Events(filter) => {
                validate_event_scope(filter)?;
                if !(1..=MAX_LOGS_PAGE).contains(&filter.limit)
                    || filter.kinds.len() > 4
                    || filter
                        .dungeon
                        .as_ref()
                        .is_some_and(|d| d.is_empty() || d.len() > 256)
                {
                    return Err(INVALID.into());
                }
                if let Some(run) = &filter.run_id {
                    identifier(run)?;
                }
                if let Some(cursor) = &filter.cursor {
                    identifier(&cursor.id)?;
                    if cursor.at.0 > i64::MAX as u64 {
                        return Err(INVALID.into());
                    }
                }
                Ok(())
            }
            Self::Home(scope) => validate_scope(scope),
            Self::ScopeLoot(scope, cursor, limit, tiers) => {
                if tiers.len() > 65_536
                    || tiers
                        .iter()
                        .any(|(_, tier)| tier.as_ref().is_some_and(|s| s.len() > 16))
                {
                    return Err(INVALID.into());
                }
                maxed_store::validate_scope_loot(scope, cursor.as_deref(), *limit)
                    .map_err(|_| INVALID.into())
            }
            Self::DungeonBests => Ok(()),
            Self::Drops(filter) => {
                validate_loot_page(filter.limit, filter.cursor.as_ref())?;
                validate_loot_range(filter.range.as_ref())?;
                if let Some(LootPlace::Dungeon { name }) = &filter.place {
                    validate_loot_dungeon(name)?;
                }
                Ok(())
            }
            Self::DropSummary(request, tiers) => {
                if request.days.len() > 8
                    || tiers.len() > 65_536
                    || tiers
                        .iter()
                        .any(|(_, tier)| tier.as_ref().is_some_and(|s| s.len() > 16))
                {
                    return Err(INVALID.into());
                }
                validate_loot_range(request.filter.range.as_ref())?;
                if let Some(LootPlace::Dungeon { name }) = &request.filter.place {
                    validate_loot_dungeon(name)?;
                }
                for day in &request.days {
                    validate_loot_range(Some(day))?;
                }
                Ok(())
            }
            Self::ItemHistory(request) => {
                validate_loot_page(request.limit, request.cursor.as_ref())
            }
            Self::LootStats(filter) => validate_loot_range(filter.range.as_ref()),
            Self::Collection(request, tiers) => {
                if request.chase.len() > MAX_CHASE_ITEMS
                    || tiers.len() > 65_536
                    || tiers
                        .iter()
                        .any(|(_, tier)| tier.as_ref().is_some_and(|s| s.len() > 16))
                {
                    return Err(INVALID.into());
                }
                if let Some(dungeon) = &request.dungeon {
                    validate_loot_dungeon(dungeon)?;
                }
                for item in &request.chase {
                    if let Some(dungeon) = &item.dungeon {
                        validate_loot_dungeon(dungeon)?;
                    }
                    if item.pinned_at.0 > i64::MAX as u64 {
                        return Err(INVALID.into());
                    }
                }
                Ok(())
            }
            Self::RunFightStrip(run_id, player_id) => {
                identifier(run_id)?;
                if let Some(id) = player_id {
                    identifier(id)?;
                }
                Ok(())
            }
            Self::Run(id) | Self::Fight(id) | Self::Player(id, _) => identifier(id),
            Self::AlertSummary(request) => validate_loot_range(request.range.as_ref()),
            Self::Alerts(filter) => {
                for kinds in [&filter.kinds, &filter.exclude_kinds] {
                    if kinds.len() > AlertKind::ALL.len()
                        || kinds
                            .iter()
                            .enumerate()
                            .any(|(i, k)| kinds[..i].contains(k))
                    {
                        return Err(INVALID.into());
                    }
                }
                if filter.kind.is_some() && !filter.kinds.is_empty() {
                    return Err(INVALID.into());
                }
                if !(1..=100).contains(&filter.limit)
                    || filter.query.len() > 512
                    || filter.query.chars().count() > 128
                    || filter.query.contains('\0')
                {
                    return Err(INVALID.into());
                }
                if let Some(range) = &filter.range {
                    if range.start >= range.end || range.end.0 > i64::MAX as u64 {
                        return Err(INVALID.into());
                    }
                }
                if let Some(cursor) = &filter.cursor {
                    identifier(&cursor.id)?;
                    if cursor.at.0 > i64::MAX as u64 {
                        return Err(INVALID.into());
                    }
                }
                Ok(())
            }
            Self::Runs(filter) => {
                if !(1..=100).contains(&filter.limit) {
                    return Err(INVALID.into());
                }
                if let Some(dungeon) = &filter.dungeon {
                    identifier(dungeon)?;
                }
                if let Some(range) = &filter.range {
                    if range.start >= range.end || range.end.0 > i64::MAX as u64 {
                        return Err(INVALID.into());
                    }
                }
                if let Some(cursor) = &filter.cursor {
                    identifier(&cursor.id)?;
                    // Canonical decimals are also the route key; bound parsing first.
                    if cursor.sort_value.len() > 20 {
                        return Err(INVALID.into());
                    }
                    let value = cursor.sort_value.parse::<i64>().map_err(|_| INVALID)?;
                    if value.to_string() != cursor.sort_value
                        || (value < 0
                            && !(value == -1 && filter.sort == RunSort::DurationDescending))
                    {
                        return Err(INVALID.into());
                    }
                }
                Ok(())
            }
        }
    }
}
fn validate_event_scope(filter: &EventFilter) -> Result<(), String> {
    validate_loot_range(filter.range.as_ref())?;
    if filter.query.len() > 512
        || filter.query.chars().count() > MAX_CHAT_QUERY_CHARS
        || filter.query.contains('\0')
    {
        return Err(INVALID.into());
    }
    filter.run_id.as_deref().map_or(Ok(()), identifier)
}

fn identifier(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        Err(INVALID.into())
    } else {
        Ok(())
    }
}

fn validate_loot_dungeon(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 256 {
        Err(INVALID.into())
    } else {
        Ok(())
    }
}

fn validate_stats(filter: &StatsFilter) -> Result<(), String> {
    validate_loot_range(filter.range.as_ref())?;
    filter.character_id.as_deref().map_or(Ok(()), identifier)
}

fn validate_loot_range(range: Option<&TimeRange>) -> Result<(), String> {
    if range.is_some_and(|range| range.start >= range.end || range.end.0 > i64::MAX as u64) {
        Err(INVALID.into())
    } else {
        Ok(())
    }
}

fn validate_loot_page(limit: u16, cursor: Option<&DropCursor>) -> Result<(), String> {
    if !(1..=MAX_LOOT_PAGE).contains(&limit) {
        return Err(INVALID.into());
    }
    if let Some(cursor) = cursor {
        identifier(&cursor.id)?;
        if cursor.at.0 > i64::MAX as u64 {
            return Err(INVALID.into());
        }
    }
    Ok(())
}

type Completion = Box<dyn FnOnce(bool) + Send>;
type Notice = Box<dyn FnOnce(bool) + Send>;
trait Operation: Send {
    fn closed(&self) -> bool;
    fn reject(self: Box<Self>, error: Option<String>);
    fn run(self: Box<Self>, job: HomeJob) -> Completion;
}
struct Read<T, F> {
    send: oneshot::Sender<Result<HistoryReply<T>, String>>,
    read: F,
}
impl<T: Send + 'static, F: FnOnce(HomeJob) -> Result<T, String> + Send + 'static> Operation
    for Read<T, F>
{
    fn closed(&self) -> bool {
        self.send.is_closed()
    }
    fn reject(self: Box<Self>, error: Option<String>) {
        let _ = self.send.send(match error {
            Some(error) => Err(error),
            None => Ok(HistoryReply::Superseded),
        });
    }
    fn run(self: Box<Self>, job: HomeJob) -> Completion {
        let Read { send, read } = *self;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read(job)))
            .unwrap_or_else(|_| Err(WORKER.into()));
        if result.is_err() {
            drop_kept_reader();
        }
        Box::new(move |superseded| {
            let _ = send.send(if superseded {
                Ok(HistoryReply::Superseded)
            } else {
                result.map(|value| HistoryReply::Ready { value })
            });
        })
    }
}

#[allow(clippy::large_enum_variant)] // The scheduler admits only one job at a time.
pub(super) enum ReaderMessage {
    Job(Pending),
    Close,
}

pub(super) struct Pending {
    runtime: Arc<CaptureRuntime>,
    key: HistoryKey,
    request: u64,
    namespace_generation: u64,
    context: Option<HomeReadContext>,
    operation: Box<dyn Operation>,
}
struct Active {
    key: HistoryKey,
    request: u64,
    superseded: bool,
}
#[derive(Default)]
pub(super) struct Scheduler {
    active: Option<Active>,
    pending: Option<Pending>,
    preparing: Option<u64>,
    request: u64,
    attempts: [VecDeque<Instant>; 2],
    notices: [Option<Notice>; 2],
    stopped: bool,
    open: bool,
}
impl Scheduler {
    #[cfg(test)]
    pub(super) fn counts(&self) -> (usize, usize, usize, usize) {
        (
            usize::from(self.active.is_some()),
            usize::from(self.pending.is_some()),
            self.notices.iter().filter(|n| n.is_some()).count(),
            self.attempts.iter().map(VecDeque::len).sum(),
        )
    }
    #[cfg(test)]
    pub(super) fn exhaust_requests(&mut self) {
        self.request = u64::MAX;
    }
    pub(super) fn stopped(&self) -> bool {
        self.stopped
    }
    pub(super) fn open(&self) -> bool {
        self.open
    }
    pub(super) fn active(&self) -> bool {
        self.active.is_some()
    }
    pub(super) fn retire(&mut self) {
        if let Some(active) = &mut self.active {
            active.superseded = true;
        }
        if let Some(pending) = self.pending.take() {
            pending.operation.reject(None);
        }
        for notice in &mut self.notices {
            if let Some(notice) = notice.take() {
                notice(true);
            }
        }
        self.preparing = None;
    }
    fn abuse(&mut self, caller: HistoryCaller, now: Instant) -> bool {
        let attempts = &mut self.attempts[caller.index()];
        while attempts
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= Duration::from_secs(1))
        {
            attempts.pop_front();
        }
        // The last 32 attempts suffice to decide whether this is attempt 33+.
        let abusive = attempts.len() == 32;
        if abusive {
            attempts.pop_front();
        }
        attempts.push_back(now);
        abusive
    }
}

fn reader_loop(owner: Weak<HomeReads>, receive: mpsc::Receiver<ReaderMessage>) {
    READER_THREAD.set(true);
    loop {
        let Some(current) = owner.upgrade() else {
            break;
        };
        let idle = *current
            .reader_idle
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        drop(current);
        let message = if reader_is_open() {
            receive.recv_timeout(idle)
        } else {
            receive
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match message {
            Ok(ReaderMessage::Job(pending)) => {
                let Some(owner) = owner.upgrade() else { break };
                owner.run_reader(pending);
            }
            Ok(ReaderMessage::Close) | Err(mpsc::RecvTimeoutError::Timeout) => {
                drop_kept_reader();
                if let Some(owner) = owner.upgrade() {
                    let mut queue = owner.scheduler.lock().unwrap_or_else(|p| p.into_inner());
                    queue.open = false;
                    owner.idle.notify_all();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

impl HomeReads {
    fn dispatch(self: &Arc<Self>, pending: Pending) {
        self.send_reader(ReaderMessage::Job(pending));
    }

    pub(super) fn close_reader(&self) {
        let sender = self
            .reader
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some((_, sender)) = sender {
            let _ = sender.send(ReaderMessage::Close);
        }
    }

    fn send_reader(self: &Arc<Self>, mut message: ReaderMessage) {
        let mut failed = None;
        for _ in 0..2 {
            let sender = {
                let mut reader = self.reader.lock().unwrap_or_else(|p| p.into_inner());
                if reader.as_ref().is_some_and(|(id, _)| Some(*id) == failed) {
                    *reader = None;
                }
                if reader.is_none() {
                    let id =
                        self.reader_spawn
                            .try_update(Ordering::AcqRel, Ordering::Acquire, |id| {
                                id.checked_add(1)
                            });
                    if let Ok(id) = id {
                        let (send, receive) = mpsc::channel();
                        let owner = Arc::downgrade(self);
                        if std::thread::Builder::new()
                            .name("history-reader".into())
                            .spawn(move || reader_loop(owner, receive))
                            .is_ok()
                        {
                            *reader = Some((id + 1, send));
                        }
                    }
                }
                reader.clone()
            };
            if let Some((id, sender)) = sender {
                match sender.send(message) {
                    Ok(()) => return,
                    Err(error) => {
                        failed = Some(id);
                        message = error.0;
                    }
                }
            }
        }
        if let ReaderMessage::Job(mut pending) = message {
            pending.context = None;
            self.run_reader(pending);
        }
    }

    fn run_reader(self: &Arc<Self>, pending: Pending) {
        let owner = self;
        let Pending {
            runtime,
            key,
            request,
            namespace_generation,
            context,
            operation,
        } = pending;
        // Only this active worker owns the reader. A dropped receiver skips SQL.
        let completion = if operation.closed() {
            operation.reject(None);
            None
        } else if let Err(error) = key.validate() {
            operation.reject(Some(error));
            None
        } else if let Some(context) = context {
            Some(operation.run(HomeJob {
                owner: owner.clone(),
                context,
            }))
        } else {
            operation.reject(Some(UNAVAILABLE.into()));
            None
        };
        let idle = *owner.reader_idle.lock().unwrap_or_else(|p| p.into_inner());
        let mut queue = owner.scheduler.lock().unwrap_or_else(|p| p.into_inner());
        let superseded = queue
            .active
            .as_ref()
            .is_none_or(|active| active.request != request || active.superseded)
            || owner.generation.load(Ordering::Acquire) != namespace_generation;
        // Retirement may race ahead of dispatching this job to the channel.
        if idle.is_zero() || superseded || queue.stopped {
            drop_kept_reader();
        }
        queue.active = None;
        queue.open = reader_is_open();
        if let Some(completion) = completion {
            completion(superseded);
        }
        for notice in &mut queue.notices {
            if let Some(notice) = notice.take() {
                notice(false);
            }
        }
        let pending = owner.dispatch_pending(&mut queue);
        owner.idle.notify_all();
        drop(queue);
        if let Some(pending) = pending {
            owner.dispatch(pending);
        }
        drop(runtime);
    }

    fn dispatch_pending(self: &Arc<Self>, queue: &mut Scheduler) -> Option<Pending> {
        if queue.active.is_some() || queue.preparing.is_some() {
            return None;
        }
        if let Some(pending) = queue.pending.take() {
            if queue.stopped
                || pending.operation.closed()
                || pending.namespace_generation != self.generation.load(Ordering::Acquire)
            {
                pending.operation.reject(None);
            } else {
                queue.active = Some(Active {
                    key: pending.key.clone(),
                    request: pending.request,
                    superseded: false,
                });
                return Some(pending);
            }
        }
        None
    }

    fn enqueue(
        self: &Arc<Self>,
        runtime: Arc<CaptureRuntime>,
        key: HistoryKey,
        generation: u64,
        operation: Box<dyn Operation>,
        initial: bool,
    ) -> Result<(), String> {
        // Use the same lock order as schema preparation and namespace retirement.
        let preparation = self.preparation.lock().map_err(|_| UNAVAILABLE)?;
        let current = self.context.lock().map_err(|_| UNAVAILABLE)?;
        let mut queue = self.scheduler.lock().map_err(|_| UNAVAILABLE)?;
        if queue.stopped || generation != self.generation.load(Ordering::Acquire) {
            operation.reject(None);
            return Ok(());
        }
        let context = if initial && preparation.running && preparation.error.is_none() {
            None
        } else {
            if current.is_none() && !preparation.ready {
                operation.reject(Some(preparation.error.unwrap_or(UNAVAILABLE).into()));
                return Ok(());
            }
            let context = current.clone().ok_or(UNAVAILABLE)?;
            if context.namespace == super::Namespace::Live && !preparation.ready {
                operation.reject(Some(preparation.error.unwrap_or(SCHEMA_FAILED).into()));
                return Ok(());
            }
            Some(context)
        };
        let Some(request) = queue.request.checked_add(1) else {
            queue.retire();
            operation.reject(Some(UNAVAILABLE.into()));
            return Ok(());
        };
        queue.request = request;
        if let Some(active) = &mut queue.active {
            if active.key == key {
                active.superseded = true;
            }
        }
        let waiting = context.is_none();
        let pending = Pending {
            runtime: runtime.clone(),
            key,
            request,
            namespace_generation: generation,
            context,
            operation,
        };
        if let Some(previous) = queue.pending.replace(pending) {
            previous.operation.reject(None);
        }
        if waiting {
            if queue.preparing.is_none() {
                queue.preparing = Some(generation);
                let owner = self.clone();
                // One async preparation waiter, never one blocking task per request.
                tauri::async_runtime::spawn(async move {
                    let result = owner.ensure().await;
                    let context = owner
                        .context
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();
                    let mut queue = owner.scheduler.lock().unwrap_or_else(|p| p.into_inner());
                    if queue.preparing != Some(generation) {
                        return;
                    }
                    queue.preparing = None;
                    if let Some(mut pending) = queue.pending.take() {
                        if generation != owner.generation.load(Ordering::Acquire) {
                            pending.operation.reject(None);
                        } else if let Err(error) = result {
                            pending.operation.reject(Some(error));
                        } else {
                            pending.context = context;
                            queue.pending = Some(pending);
                        }
                    }
                    let pending = owner.dispatch_pending(&mut queue);
                    drop(queue);
                    if let Some(pending) = pending {
                        owner.dispatch(pending);
                    }
                    drop(runtime);
                });
            }
        } else {
            let pending = self.dispatch_pending(&mut queue);
            drop(queue);
            drop(current);
            drop(preparation);
            if let Some(pending) = pending {
                self.dispatch(pending);
            }
        }
        Ok(())
    }

    pub(super) fn resume_reads(&self) {
        self.scheduler
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stopped = false;
    }

    pub(super) fn close_idle_reader(&self) {
        self.close_reader();
        let queue = self.scheduler.lock().unwrap_or_else(|p| p.into_inner());
        drop(
            self.idle
                .wait_while(queue, |queue| queue.open())
                .unwrap_or_else(|p| p.into_inner()),
        );
    }

    pub(super) fn shutdown_reads(&self) {
        let mut queue = self.scheduler.lock().unwrap_or_else(|p| p.into_inner());
        queue.stopped = true;
        queue.retire();
        drop(queue);
        self.close_reader();
        let queue = self.scheduler.lock().unwrap_or_else(|p| p.into_inner());
        drop(
            self.idle
                .wait_while(queue, |queue| queue.active() || queue.open())
                .unwrap_or_else(|p| p.into_inner()),
        );
    }
}

impl CaptureRuntime {
    pub async fn scope_loot(
        self: &Arc<Self>,
        scope: HomeScope,
        cursor: Option<String>,
        limit: u16,
        tiers: Vec<(u16, Option<String>)>,
    ) -> Result<HistoryReply<ScopeLootPage>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::ScopeLoot(scope.clone(), cursor.clone(), limit, tiers.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || ScopeLootPage {
                        items: vec![],
                        next: None,
                        total_bags: 0,
                    },
                    |reader| reader.scope_loot(scope, cursor, limit, &tiers),
                )
            },
        )
        .await
    }

    pub async fn dungeon_bests(self: &Arc<Self>) -> Result<HistoryReply<Vec<DungeonBest>>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::DungeonBests,
            move |job| runtime.read_history(job, Vec::new, |reader| reader.dungeon_bests()),
        )
        .await
    }

    pub(super) async fn submit_history<T: Send + 'static>(
        self: &Arc<Self>,
        caller: HistoryCaller,
        key: HistoryKey,
        read: impl FnOnce(HomeJob) -> Result<T, String> + Send + 'static,
    ) -> Result<HistoryReply<T>, String> {
        let owner = &self.shared.home;
        let generation = owner.generation.load(Ordering::Acquire);
        let (send, receive) = oneshot::channel();
        let admitted = {
            let mut queue = owner.scheduler.lock().map_err(|_| UNAVAILABLE)?;
            if queue.stopped {
                return Ok(HistoryReply::Superseded);
            }
            // Capture the rejected attempt's active completion epoch under this lock.
            if queue.abuse(caller, owner.now()) {
                if queue.active.is_some() {
                    let notice: Notice = Box::new(move |superseded| {
                        let _ = send.send(Ok(if superseded {
                            HistoryReply::Superseded
                        } else {
                            HistoryReply::Busy
                        }));
                    });
                    if let Some(previous) = queue.notices[caller.index()].replace(notice) {
                        previous(true);
                    }
                } else {
                    let _ = send.send(Ok(HistoryReply::Busy));
                }
                None
            } else {
                Some(send)
            }
        };
        if let Some(send) = admitted {
            key.validate()?;
            let initial = {
                let preparation = owner.preparation.lock().map_err(|_| UNAVAILABLE)?;
                preparation.running && preparation.error.is_none()
            };
            if !initial {
                let result = self.ensure_history_ready().await;
                if generation != owner.generation.load(Ordering::Acquire) {
                    return Ok(HistoryReply::Superseded);
                }
                result?;
            }
            owner.enqueue(
                self.clone(),
                key,
                generation,
                Box::new(Read { send, read }),
                initial,
            )?;
        }
        receive.await.map_err(|_| WORKER.to_string())?
    }

    pub async fn home_summary(
        self: &Arc<Self>,
        scope: HomeScope,
    ) -> Result<HistoryReply<HomeSummary>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Home(scope.clone()),
            move |job| {
                runtime.read_history(job, empty_history, |reader| reader.home_summary(scope))
            },
        )
        .await
    }
    pub async fn alert_summary(
        self: &Arc<Self>,
        request: AlertSummaryRequest,
    ) -> Result<HistoryReply<AlertSummary>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::AlertSummary(request.clone()),
            move |job| {
                runtime.read_history(job, AlertSummary::default, |reader| {
                    reader.alert_summary(request)
                })
            },
        )
        .await
    }
    pub async fn list_alerts(
        self: &Arc<Self>,
        mut filter: AlertFilter,
    ) -> Result<HistoryReply<AlertPage>, String> {
        filter.query = filter.query.trim().to_owned();
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Alerts(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || AlertPage {
                        rows: vec![],
                        next: None,
                    },
                    |reader| reader.list_alerts(filter),
                )
            },
        )
        .await
    }
    pub async fn list_runs(
        self: &Arc<Self>,
        filter: RunFilter,
    ) -> Result<HistoryReply<RunPage>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Runs(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || RunPage {
                        details: Some(Vec::new()),
                        summary: None,
                        rows: vec![],
                        next: None,
                    },
                    |reader| reader.list_runs(filter),
                )
            },
        )
        .await
    }
    pub async fn list_drops(
        self: &Arc<Self>,
        filter: DropFilter,
    ) -> Result<HistoryReply<DropPage>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Drops(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || DropPage {
                        rows: vec![],
                        next: None,
                    },
                    |reader| reader.list_drops(filter),
                )
            },
        )
        .await
    }

    pub async fn get_drop_summary(
        self: &Arc<Self>,
        request: DropSummaryRequest,
        tiers: Vec<(u16, Option<String>)>,
    ) -> Result<HistoryReply<DropSummary>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::DropSummary(request.clone(), tiers.clone()),
            move |job| {
                let day_count = request.days.len();
                let empty = DropTotals {
                    items_counted: request.include_items,
                    ut_items: request.include_items.then_some(0),
                    st_items: request.include_items.then_some(0),
                    enchanted_items: request.include_items.then_some(0),
                    runs_ended: (!matches!(request.filter.place, Some(LootPlace::Outside)))
                        .then_some(0),
                    ..Default::default()
                };
                runtime.read_history(
                    job,
                    || DropSummary {
                        totals: empty.clone(),
                        days: vec![empty; day_count],
                    },
                    |reader| reader.drop_summary(request, &tiers),
                )
            },
        )
        .await
    }

    pub async fn get_item_history(
        self: &Arc<Self>,
        request: ItemHistoryRequest,
    ) -> Result<HistoryReply<ItemHistory>, String> {
        let runtime = self.clone();
        let item_type = request.item_type;
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::ItemHistory(request.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || ItemHistory {
                        item_type,
                        totals: ItemTotals {
                            appearances: 0,
                            bags: 0,
                            taken: 0,
                            first_at: None,
                            last_at: None,
                        },
                        enchant_variants: vec![],
                        by_dungeon: vec![],
                        by_source: vec![],
                        bags: DropPage {
                            rows: vec![],
                            next: None,
                        },
                    },
                    |reader| reader.item_history(request),
                )
            },
        )
        .await
    }

    pub async fn get_loot_stats(
        self: &Arc<Self>,
        filter: LootStatsFilter,
    ) -> Result<HistoryReply<LootStats>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::LootStats(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || LootStats {
                        dungeons: vec![],
                        sources: vec![],
                        totals: vec![],
                        whites: 0,
                    },
                    |reader| reader.loot_stats(filter),
                )
            },
        )
        .await
    }

    pub async fn get_collection(
        self: &Arc<Self>,
        request: CollectionRequest,
        tiers: Vec<(u16, Option<String>)>,
    ) -> Result<HistoryReply<Collection>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Collection(request.clone(), tiers.clone()),
            move |job| {
                let empty_request = request.clone();
                runtime.read_history(
                    job,
                    || Collection {
                        chase: empty_request
                            .chase
                            .into_iter()
                            .map(|item| ChaseRow {
                                item,
                                seen: 0,
                                last_at: None,
                                dry_runs: 0,
                            })
                            .collect(),
                        grid_dungeon: empty_request.dungeon,
                        grid: vec![],
                    },
                    |reader| reader.collection(request, &tiers),
                )
            },
        )
        .await
    }

    pub async fn get_run(
        self: &Arc<Self>,
        id: String,
    ) -> Result<HistoryReply<Option<RunStory>>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Run(id.clone()),
            move |job| runtime.read_history(job, || None, |reader| reader.get_run(&id)),
        )
        .await
    }
    pub async fn get_run_fight_strip(
        self: &Arc<Self>,
        run_id: String,
        player_id: Option<String>,
    ) -> Result<HistoryReply<Vec<RunFightStripRow>>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::RunFightStrip(run_id.clone(), player_id.clone()),
            move |job| {
                runtime.read_history(job, Vec::new, |reader| {
                    reader.run_fight_strip(&run_id, player_id.as_deref())
                })
            },
        )
        .await
    }
    pub async fn get_fight(
        self: &Arc<Self>,
        id: String,
    ) -> Result<HistoryReply<Option<FightView>>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Fight(id.clone()),
            move |job| runtime.read_history(job, || None, |reader| reader.get_fight(&id)),
        )
        .await
    }
    pub async fn get_player_breakdown(
        self: &Arc<Self>,
        fight_id: String,
        object_id: i32,
    ) -> Result<HistoryReply<Option<PlayerDamage>>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::Player(fight_id.clone(), object_id),
            move |job| {
                runtime.read_history(
                    job,
                    || None,
                    |reader| reader.get_player_breakdown(&fight_id, object_id),
                )
            },
        )
        .await
    }
}

pub(super) fn read_error(error: maxed_store::StoreError) -> String {
    match error {
        maxed_store::StoreError::Limit => LIMIT,
        _ => READ_FAILED,
    }
    .into()
}

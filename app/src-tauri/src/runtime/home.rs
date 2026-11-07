//! Home readers own no capture locks and never cross a thread boundary.
use super::history::{read_error, ReaderMessage, Scheduler};
use super::{join::Bridge, Adapter, CaptureRuntime, Namespace, RuntimeOptions, Shared};
use maxed_store::SCHEMA_VERSION;
use maxed_types::{Catalog, HomeScope, HomeSummary};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

pub(super) const UNAVAILABLE: &str = "Home history is unavailable; retry after storage is ready";
pub(super) const CHANGED: &str = "Home history changed; retry for the current source";
pub(super) const READ_FAILED: &str =
    "Home history could not be read; retry or repair local storage";
pub(super) const SCHEMA_FAILED: &str = "History schema upgrade failed; close other Maxed instances or resolve local storage access, then Retry.";
pub(super) const HISTORY_UPGRADE_REQUIRED: &str = "History upgrade required";
pub(super) const SCHEMA_NEWER: &str =
    "History schema is newer than this Maxed build; update Maxed, then Retry.";
const INVALID_SCOPE: &str = "Home history scope is invalid; select a session or day again";

#[derive(Clone)]
pub(super) struct HomeReadContext {
    pub database: PathBuf,
    pub namespace: Namespace,
    pub generation: u64,
    pub stream_id: Option<String>,
    pub absent: bool,
    pub account: Option<String>,
    pub account_revision: u64,
    pub identity_sequence: Option<maxed_types::Sequence>,
}

#[cfg(test)]
type RecoveryBatchHook = Arc<dyn Fn(u16) + Send + Sync>;

type AccountNotice = Arc<dyn Fn(maxed_types::AccountHistoryChanged) + Send + Sync>;

pub(super) struct HomeReads {
    pub(super) context: Mutex<Option<HomeReadContext>>,
    pub(super) generation: AtomicU64,
    pub(super) account_revision: AtomicU64,
    pub(super) resolved_account: Mutex<Option<(Namespace, Option<String>)>>,
    pub(super) account_notice: Mutex<Option<AccountNotice>>,
    live_database: Mutex<Option<PathBuf>>,
    pub(super) settings: Mutex<Option<Arc<crate::settings::SettingsStore>>>,
    pub(super) scheduler: Mutex<Scheduler>,
    pub(super) reader: Mutex<Option<(u64, std::sync::mpsc::Sender<ReaderMessage>)>>,
    pub(super) reader_spawn: AtomicU64,
    pub(super) reader_idle: Mutex<Duration>,
    pub(super) idle: Condvar,
    pub(super) preparation: Mutex<Preparation>,
    prepared: Condvar,
    preparation_changed: tokio::sync::Notify,
    #[cfg(test)]
    pub(super) on_identity_wait: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) on_recovery_batch: Mutex<Option<RecoveryBatchHook>>,
    #[cfg(test)]
    pub(super) on_recovery_attempt: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) on_prepare: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) clock: Mutex<Option<Arc<dyn Fn() -> Instant + Send + Sync>>>,
    #[cfg(test)]
    pub(super) on_reader: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) on_open: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) on_storage_admission: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

const READER_IDLE: Duration = Duration::from_secs(60);

struct KeptReader {
    database: PathBuf,
    generation: u64,
    reader: maxed_store::StoreReader,
}

thread_local! {
    pub(super) static READER_THREAD: Cell<bool> = const { Cell::new(false) };
    static KEPT_READER: RefCell<Option<KeptReader>> = const { RefCell::new(None) };
}

pub(super) fn drop_kept_reader() {
    KEPT_READER.with(|kept| {
        kept.borrow_mut().take();
    });
}

pub(super) fn reader_is_open() -> bool {
    KEPT_READER.with(|kept| kept.borrow().is_some())
}

impl Default for HomeReads {
    fn default() -> Self {
        Self {
            context: Default::default(),
            generation: Default::default(),
            account_revision: Default::default(),
            resolved_account: Default::default(),
            account_notice: Default::default(),
            live_database: Default::default(),
            settings: Default::default(),
            scheduler: Default::default(),
            reader: Default::default(),
            reader_spawn: Default::default(),
            idle: Default::default(),
            preparation: Default::default(),
            prepared: Default::default(),
            preparation_changed: Default::default(),
            reader_idle: Mutex::new(READER_IDLE),
            #[cfg(test)]
            on_identity_wait: Default::default(),
            #[cfg(test)]
            on_recovery_batch: Default::default(),
            #[cfg(test)]
            on_recovery_attempt: Default::default(),
            #[cfg(test)]
            on_prepare: Default::default(),
            #[cfg(test)]
            clock: Default::default(),
            #[cfg(test)]
            on_reader: Default::default(),
            #[cfg(test)]
            on_open: Default::default(),
            #[cfg(test)]
            on_storage_admission: Default::default(),
        }
    }
}

#[derive(Default)]
pub(super) struct Preparation {
    pub(super) ready: bool,
    absent: bool,
    pub(super) running: bool,
    pub(super) error: Option<&'static str>,
    last_attempt: Option<Instant>,
    allow_recovery: bool,
    update_paused: bool,
}

/// Context lease owned only by the active blocking operation.
pub(super) struct HomeJob {
    pub(super) owner: Arc<HomeReads>,
    pub(super) context: HomeReadContext,
    pub(super) account_scoped: bool,
}

impl HomeReads {
    pub(super) fn live_database(&self) -> Option<PathBuf> {
        self.live_database.lock().ok()?.clone()
    }

    pub(super) fn pause_for_update(&self) -> Result<(), String> {
        let mut preparation = self.preparation.lock().map_err(|_| UNAVAILABLE)?;
        preparation.update_paused = true;
        self.preparation_changed.notify_waiters();
        drop(
            self.prepared
                .wait_while(preparation, |state| state.running)
                .map_err(|_| UNAVAILABLE)?,
        );
        self.shutdown_reads();
        Ok(())
    }

    pub(super) fn resume_after_update(&self) {
        self.preparation
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .update_paused = false;
        self.resume_reads();
    }

    fn next_generation(&self) -> Result<u64, String> {
        self.generation
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map(|previous| previous + 1)
            .map_err(|_| UNAVAILABLE.into())
    }

    /// Lifecycle callers serialize transitions. Clear first so an in-flight
    /// reader fails its final check, then wait without holding the context lock.
    pub(super) fn invalidate_and_wait(&self) -> Result<u64, String> {
        let mut preparation = self.preparation.lock().map_err(|_| UNAVAILABLE)?;
        preparation.allow_recovery = false;
        let generation = {
            let mut context = self.context.lock().map_err(|_| UNAVAILABLE)?;
            *context = None;
            let mut queue = self.scheduler.lock().map_err(|_| UNAVAILABLE)?;
            queue.retire();
            self.next_generation()
        };
        self.preparation_changed.notify_waiters();
        // Lifecycle entrypoints run on native blocking workers. Finish the
        // dedicated upgrade before capture takes its namespace lease or cleanup.
        drop(
            self.prepared
                .wait_while(preparation, |state| state.running)
                .map_err(|_| UNAVAILABLE)?,
        );
        self.close_reader();
        let active = self.scheduler.lock().map_err(|_| UNAVAILABLE)?;
        drop(
            self.idle
                .wait_while(active, |queue| queue.active() || queue.open())
                .map_err(|_| UNAVAILABLE)?,
        );
        generation
    }

    fn install(&self, mut context: HomeReadContext) -> Result<(), String> {
        let mut current = self.context.lock().map_err(|_| UNAVAILABLE)?;
        if self.generation.load(Ordering::Acquire) != context.generation {
            return Err(CHANGED.into());
        }
        // A cancelled sweep can publish recovery between context construction and
        // installation. Retain that revision even when the account is unchanged.
        context.account_revision = self.account_revision.load(Ordering::Acquire);
        // Lifecycle retirement closed the sole reader before installation.
        // Startup preparation calls this only after migration has finished.
        context.account = maxed_store::open_reader(&context.database)
            .and_then(|reader| reader.current_account())
            .ok()
            .flatten();
        let mut resolved = self.resolved_account.lock().map_err(|_| UNAVAILABLE)?;
        let value = (context.namespace, context.account.clone());
        let changed = resolved
            .as_ref()
            .map_or(context.account.is_some(), |old| old != &value);
        *resolved = Some(value);
        if changed {
            context.account_revision = self.account_revision.fetch_add(1, Ordering::AcqRel) + 1;
        }
        let revision = context.account_revision;
        *current = Some(context);
        drop(resolved);
        drop(current);
        if changed {
            if let Some(notice) = self.account_notice.lock().ok().and_then(|n| n.clone()) {
                notice(maxed_types::AccountHistoryChanged {
                    revision,
                    reason: maxed_types::AccountHistoryChangeReason::AccountSwitched,
                });
            }
        }
        Ok(())
    }

    fn check(&self, expected: &HomeReadContext, account_scoped: bool) -> Result<(), String> {
        let context = self.context.lock().map_err(|_| UNAVAILABLE)?;
        match context.as_ref() {
            Some(current)
                if current.generation == expected.generation
                    && current.namespace == expected.namespace
                    && current.stream_id == expected.stream_id
                    && (!account_scoped
                        || current.account_revision == expected.account_revision) =>
            {
                Ok(())
            }
            _ => Err(CHANGED.into()),
        }
    }

    pub(super) fn reader_context(
        &self,
        expected: &HomeReadContext,
        account_scoped: bool,
    ) -> Result<HomeReadContext, String> {
        let preparation = self.preparation.lock().map_err(|_| UNAVAILABLE)?;
        if expected.namespace == Namespace::Live && !preparation.ready {
            return Err(preparation.error.unwrap_or(SCHEMA_FAILED).into());
        }
        let context = self.context.lock().map_err(|_| UNAVAILABLE)?;
        match context.as_ref() {
            Some(current)
                if current.generation == expected.generation
                    && current.namespace == expected.namespace
                    && current.stream_id == expected.stream_id
                    && (!account_scoped
                        || current.account_revision == expected.account_revision) =>
            {
                Ok(current.clone())
            }
            _ => Err(CHANGED.into()),
        }
    }

    pub(super) fn restore_live(&self) -> Result<(), String> {
        let generation = self.invalidate_and_wait()?;
        self.install_live(generation)
    }

    fn install_live(&self, generation: u64) -> Result<(), String> {
        let database = self.live_database.lock().map_err(|_| UNAVAILABLE)?.clone();
        if let Some(database) = database {
            let mut state = self.preparation.lock().map_err(|_| UNAVAILABLE)?;
            state.allow_recovery = true;
            // Restoring the live source must not block lifecycle transitions.
            // Schema readiness is enforced only when admitting history reads.
            let absent = state.absent;
            drop(state);
            self.install(HomeReadContext {
                database,
                namespace: Namespace::Live,
                generation,
                stream_id: None,
                absent,
                account: None,
                account_revision: self.account_revision.load(Ordering::Acquire),
                identity_sequence: None,
            })?;
        }
        Ok(())
    }
}

impl HomeReads {
    pub(super) fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(clock) = self.clock.lock().unwrap().as_ref() {
            return clock();
        }
        Instant::now()
    }

    fn dispatch_preparation(self: &Arc<Self>, generation: u64, root: PathBuf) {
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _lease = super::storage::namespace_lease(&root, Namespace::Live)
                    .map_err(|_| SCHEMA_FAILED)?;
                #[cfg(test)]
                {
                    let hook = owner.on_prepare.lock().unwrap().clone();
                    if let Some(hook) = hook {
                        hook();
                    }
                }
                let path = root.join("live.sqlite3");
                match std::fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
                    Err(_) => Err(SCHEMA_FAILED),
                    Ok(_) => {
                        maxed_store::upgrade_history(&path).map_err(schema_error)?;
                        let retention = owner
                            .settings
                            .lock()
                            .ok()
                            .and_then(|settings| settings.clone())
                            .and_then(|settings| settings.get().ok())
                            .map(|settings| settings.chat.retention_days);
                        // Never prune with a guessed value: unreadable settings skip pruning.
                        if let Some(days) = retention {
                            let before = super::chat::cutoff(super::chat::wall_now(), days);
                            for _ in 0..20 {
                                let more = maxed_store::maintain_chat_file(
                                    &path,
                                    maxed_store::ChatMaintenance::Prune { before },
                                )
                                .map(|n| n >= u64::from(maxed_store::CHAT_PRUNE_BATCH))
                                .unwrap_or(false);
                                if !more {
                                    break;
                                }
                            }
                        }
                        Ok(false)
                    }
                }
            }))
            .unwrap_or(Err(SCHEMA_FAILED));
            let mut state = owner.preparation.lock().unwrap_or_else(|p| p.into_inner());
            state.running = false;
            match result {
                Ok(absent) => {
                    state.ready = true;
                    state.absent = absent;
                    state.error = None;
                    // Retirement still applies when the checked generation
                    // counter is exhausted and cannot advance to a new value.
                    if state.allow_recovery {
                        let _ = owner.install(HomeReadContext {
                            database: root.join("live.sqlite3"),
                            namespace: Namespace::Live,
                            generation,
                            stream_id: None,
                            absent,
                            account: None,
                            account_revision: owner.account_revision.load(Ordering::Acquire),
                            identity_sequence: None,
                        });
                    }
                }
                Err(error) => {
                    state.ready = false;
                    state.error = Some(error);
                }
            }
            drop(state);
            owner.prepared.notify_all();
            owner.preparation_changed.notify_waiters();
        });
    }

    pub(super) async fn ensure(self: &Arc<Self>) -> Result<(), String> {
        let generation = self.generation.load(Ordering::Acquire);
        let mut admitted_recovery = false;
        loop {
            // Register before inspecting state so completion cannot be missed.
            let changed = self.preparation_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.generation.load(Ordering::Acquire) != generation {
                return Err(CHANGED.into());
            }
            let dispatch = {
                let mut state = self.preparation.lock().map_err(|_| UNAVAILABLE)?;
                if state.update_paused {
                    return Err(UNAVAILABLE.into());
                }
                if self
                    .context
                    .lock()
                    .map_err(|_| UNAVAILABLE)?
                    .as_ref()
                    .is_some_and(|context| context.namespace != Namespace::Live || state.ready)
                {
                    return Ok(());
                }
                if !state.allow_recovery {
                    return Err(UNAVAILABLE.into());
                }
                if !state.ready && !state.running {
                    state.error.get_or_insert(SCHEMA_FAILED);
                }
                if let Some(error) = state.error {
                    if admitted_recovery {
                        if !state.running {
                            return Err(error.into());
                        }
                        None
                    } else if state.running
                        || state.last_attempt.is_some_and(|at| {
                            self.now().saturating_duration_since(at) < Duration::from_secs(5)
                        })
                    {
                        return Err(error.into());
                    } else {
                        let path = self
                            .live_database
                            .lock()
                            .map_err(|_| UNAVAILABLE)?
                            .clone()
                            .ok_or(UNAVAILABLE)?;
                        state.running = true;
                        state.last_attempt = Some(self.now());
                        admitted_recovery = true;
                        Some(path.parent().ok_or(UNAVAILABLE)?.to_path_buf())
                    }
                } else if state.running {
                    None
                } else {
                    return Err(UNAVAILABLE.into());
                }
            };
            if let Some(root) = dispatch {
                self.dispatch_preparation(generation, root);
            }
            changed.await;
        }
    }

    fn fail_appeared_database(
        &self,
        context: &HomeReadContext,
        error: maxed_store::StoreError,
    ) -> String {
        let error = schema_error(error);
        let mut state = self.preparation.lock().unwrap_or_else(|p| p.into_inner());
        let mut current = self.context.lock().unwrap_or_else(|p| p.into_inner());
        if current
            .as_ref()
            .is_some_and(|c| c.generation == context.generation)
        {
            state.ready = false;
            state.error = Some(error);
            *current = None;
        }
        error.into()
    }
}

fn schema_error(error: maxed_store::StoreError) -> &'static str {
    match error {
        maxed_store::StoreError::NewerSchema => SCHEMA_NEWER,
        maxed_store::StoreError::HistoryUpgradeRequired => HISTORY_UPGRADE_REQUIRED,
        _ => SCHEMA_FAILED,
    }
}

impl HomeReads {
    pub(super) fn upgraded(&self) -> Result<(), String> {
        let mut state = self.preparation.lock().map_err(|_| UNAVAILABLE)?;
        state.ready = true;
        state.absent = false;
        state.error = None;
        state.last_attempt = None;
        drop(state);
        self.restore_live()
    }
}

impl Shared {
    pub(super) fn open_home(
        &self,
        options: &RuntimeOptions,
        cancel: &AtomicBool,
        bridge: Bridge,
        catalog: Option<Arc<Catalog>>,
    ) -> Result<Adapter, String> {
        if self.upgrade.running() {
            return Err("History upgrade is running; wait for it to finish.".into());
        }
        // Playback suspends live maintenance, but cannot revoke a request
        // already answered Queued. attach(false) keeps the FIFO dormant until
        // live ownership returns.
        self.views.maintenance.handoff_background();
        if options.namespace != Namespace::Live {
            self.views.maintenance.defer_requests();
        }
        // Resolve policy before retiring history or creating a writer. This
        // dedicated lock also serializes read/publication with preference edits;
        // none of the history admission locks are held during settings I/O.
        let mut alerts = self
            .alerts
            .lock()
            .map_err(|_| "Alert settings unavailable")?;
        let (saved, chat) = match &alerts.settings {
            Some(settings) => match settings.get() {
                Ok(settings) => (settings.alerts, settings.chat),
                Err(error) if options.namespace == Namespace::Live => return Err(error),
                Err(_) => (
                    maxed_types::AlertPreferences::disabled(),
                    maxed_types::ChatSettings::default(),
                ),
            },
            // Native callers without a settings service use the in-memory policy.
            None => (
                self.views
                    .alert_policy
                    .lock()
                    .map_err(|_| UNAVAILABLE)?
                    .clone(),
                self.views
                    .chat_policy
                    .lock()
                    .map_err(|_| UNAVAILABLE)?
                    .clone(),
            ),
        };
        *self.views.chat_policy.lock().map_err(|_| UNAVAILABLE)? = chat;
        #[cfg(test)]
        if let Some(hook) = self.on_alert_policy_read.lock().unwrap().clone() {
            hook();
        }
        let mut delivery = self
            .views
            .delivery
            .lock()
            .map_err(|_| "Alert delivery unavailable")?;
        delivery.retire();
        drop(delivery); // No delivery guard spans history waits, SQLite or asset loading.
        let generation = self.home.invalidate_and_wait()?;
        if options.namespace == Namespace::Live {
            *self.home.live_database.lock().map_err(|_| UNAVAILABLE)? = Some(options.database());
        }
        // Playback cleanup inside open_catalog runs only after every Home reader
        // has closed. Failed playback opens leave history unavailable. Live opens
        // do not replace the database, so existing history remains readable.
        *self.home.account_notice.lock().map_err(|_| UNAVAILABLE)? =
            Some(bridge.account_changed.clone());
        let mut adapter = match options.open_catalog(cancel, self.views.clone(), bridge, catalog) {
            Ok(adapter) => adapter,
            Err(error) => {
                if options.namespace == Namespace::Live {
                    self.home.install_live(generation)?;
                }
                return Err(error);
            }
        };
        if options.namespace == Namespace::Live {
            let mut state = self.home.preparation.lock().map_err(|_| UNAVAILABLE)?;
            state.ready = true;
            state.absent = false;
            state.error = None;
        }
        let stream_id = self
            .views
            .snapshot
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .stream_id
            .clone();
        self.home.install(HomeReadContext {
            database: options.database(),
            namespace: options.namespace,
            generation,
            stream_id: Some(stream_id.clone()),
            absent: false,
            account: None,
            account_revision: self.home.account_revision.load(Ordering::Acquire),
            identity_sequence: None,
        })?;
        let mut delivery = self
            .views
            .delivery
            .lock()
            .map_err(|_| "Alert delivery unavailable")?;
        if options.namespace == Namespace::Live {
            *self.views.alert_policy.lock().map_err(|_| UNAVAILABLE)? = saved;
        } else {
            adapter.alert_policy = Arc::new(Mutex::new(saved));
        }
        if options.namespace == Namespace::Live {
            delivery.stream = Some(stream_id);
        }
        alerts.namespace = options.namespace;
        adapter.home = Some(self.home.clone());
        self.views
            .maintenance
            .attach(options.namespace == Namespace::Live);
        Ok(adapter)
    }
}

impl CaptureRuntime {
    #[cfg(test)]
    pub(super) fn register_storage_path_for_test(&self, path: &std::path::Path) {
        *self.shared.home.live_database.lock().unwrap() = Some(path.to_owned());
    }
    /// Register and schedule immediately, without filesystem/SQLite work or a
    /// blocking wait on the caller (the app setup/UI thread).
    pub fn initialize_home(self: &Arc<Self>, app_data: PathBuf) -> Result<(), String> {
        let home = &self.shared.home;
        let mut path = home.live_database.lock().map_err(|_| UNAVAILABLE)?;
        if path.is_some() {
            return Err(UNAVAILABLE.into());
        }
        *path = Some(app_data.join("live.sqlite3"));
        drop(path);
        let generation = home.next_generation()?;
        {
            let mut state = home.preparation.lock().map_err(|_| UNAVAILABLE)?;
            state.running = true;
            state.last_attempt = Some(home.now());
            state.allow_recovery = true;
        }
        home.dispatch_preparation(generation, app_data);
        let runtime = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let Some(runtime) = runtime.upgrade() else {
                return;
            };
            runtime.wait_history_preparation().await;
            let manifest = runtime
                .shared
                .home
                .live_database()
                .and_then(|p| p.parent().map(|p| p.join("backups/live/manifest.sqlite3")));
            if !manifest.is_some_and(|p| p.is_file()) {
                return;
            }
            if let Ok(jobs) = runtime.read_storage_jobs().await {
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    runtime.resume_requested_storage(jobs)
                })
                .await;
            }
        });
        Ok(())
    }

    pub(crate) async fn wait_history_preparation(&self) {
        loop {
            let changed = self.shared.home.preparation_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self
                .shared
                .home
                .preparation
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .running
            {
                return;
            }
            changed.await;
        }
    }

    pub(crate) async fn ensure_history_ready(self: &Arc<Self>) -> Result<(), String> {
        self.shared.home.ensure().await
    }

    /// Construct and use SQLite only on the calling thread.
    pub(super) fn read_history<T>(
        &self,
        job: HomeJob,
        absent_live: impl FnOnce() -> T,
        read: impl FnOnce(&maxed_store::StoreReader) -> Result<T, maxed_store::StoreError>,
    ) -> Result<T, String> {
        // Pending work retains its source, but must see an appeared database's
        // updated readiness/absence state after the preceding read finishes.
        let context = job.owner.reader_context(&job.context, job.account_scoped)?;
        let result = self.read_history_context(&context, absent_live, read);
        // Keep the schema failure when an appeared-invalid database clears readiness.
        // Namespace retirement is classified by the scheduler after reader close.
        if !matches!(&result, Err(error) if error == SCHEMA_FAILED || error == SCHEMA_NEWER) {
            job.owner.check(&job.context, job.account_scoped)?;
        }
        result
    }

    pub(super) fn read_existing_storage(
        &self,
        job: HomeJob,
        estimate: bool,
    ) -> Result<Option<super::maintenance::StorageObservation>, String> {
        let context = job.owner.reader_context(&job.context, job.account_scoped)?;
        if context.namespace != Namespace::Live {
            return Ok(None);
        }
        let service = &self.shared.views.maintenance;
        let gate = estimate
            .then(|| service.estimate_gate.try_lock().ok())
            .flatten();
        let result = KEPT_READER.with(|kept| {
            let kept = kept.borrow();
            let Some(kept) = kept
                .as_ref()
                .filter(|k| k.generation == context.generation && k.database == context.database)
            else {
                return Ok(None);
            };
            if kept.reader.user_version().map_err(read_error)? != SCHEMA_VERSION {
                return Ok(None);
            }
            let jobs = kept.reader.maintenance_statuses().map_err(read_error)?;
            let measured =
                (gate.is_some() && !jobs.iter().any(super::maintenance::runnable)).then(|| {
                    kept.reader
                        .estimate_legacy_receipt_bytes(super::chat::wall_now())
                });
            Ok(Some((
                kept.reader.logical_storage().map_err(read_error)?,
                jobs,
                measured,
            )))
        });
        job.owner.check(&job.context, job.account_scoped)?;
        result
    }

    fn read_history_context<T>(
        &self,
        context: &HomeReadContext,
        absent_live: impl FnOnce() -> T,
        read: impl FnOnce(&maxed_store::StoreReader) -> Result<T, maxed_store::StoreError>,
    ) -> Result<T, String> {
        let kept = KEPT_READER.with(|kept| kept.borrow_mut().take());
        match std::fs::symlink_metadata(&context.database) {
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && context.namespace == Namespace::Live
                    && context.stream_id.is_none()
                    && context.absent =>
            {
                // Only an established absent cold live database means no history.
                // Existing corrupt/unreadable files and absent playback never do.
                Ok(absent_live())
            }
            Err(_) => Err(READ_FAILED.into()),
            Ok(_) => {
                let kept = kept.filter(|kept| {
                    READER_THREAD.get()
                        && kept.generation == context.generation
                        && kept.database == context.database
                        && !context.absent
                });
                let reader = match kept {
                    Some(kept) => match kept.reader.user_version().map_err(read_error)? {
                        SCHEMA_VERSION => Some(kept.reader),
                        version if version > SCHEMA_VERSION => return Err(SCHEMA_NEWER.into()),
                        _ => None,
                    },
                    None => None,
                };
                let reader = match reader {
                    Some(reader) => reader,
                    None => {
                        let reader =
                            maxed_store::open_reader(&context.database).map_err(|error| {
                                if context.absent {
                                    self.shared.home.fail_appeared_database(context, error)
                                } else if matches!(error, maxed_store::StoreError::NewerSchema) {
                                    SCHEMA_NEWER.to_owned()
                                } else {
                                    READ_FAILED.to_owned()
                                }
                            })?;
                        #[cfg(test)]
                        {
                            let hook = self.shared.home.on_open.lock().unwrap().clone();
                            if let Some(hook) = hook {
                                hook();
                            }
                        }
                        reader
                    }
                };
                if context.absent {
                    let mut state = self
                        .shared
                        .home
                        .preparation
                        .lock()
                        .map_err(|_| UNAVAILABLE)?;
                    let mut current = self.shared.home.context.lock().map_err(|_| UNAVAILABLE)?;
                    if let Some(current) = current
                        .as_mut()
                        .filter(|c| c.generation == context.generation)
                    {
                        current.absent = false;
                        state.absent = false;
                    }
                }
                #[cfg(test)]
                {
                    let hook = self.shared.home.on_reader.lock().unwrap().clone();
                    if let Some(hook) = hook {
                        hook();
                    }
                }
                let result = read(&reader).map_err(read_error);
                if result.is_ok()
                    && READER_THREAD.get()
                    && !self
                        .shared
                        .home
                        .reader_idle
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .is_zero()
                {
                    KEPT_READER.with(|kept| {
                        *kept.borrow_mut() = Some(KeptReader {
                            database: context.database.clone(),
                            generation: context.generation,
                            reader,
                        });
                    });
                }
                result
            }
        }
    }
}

pub(super) fn validate_scope(scope: &HomeScope) -> Result<(), String> {
    let valid = match scope {
        HomeScope::Session { id } => {
            !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
        }
        HomeScope::Today { range } => range.start < range.end && range.end.0 <= i64::MAX as u64,
    };
    if valid {
        Ok(())
    } else {
        Err(INVALID_SCOPE.into())
    }
}

pub(super) fn empty_history() -> HomeSummary {
    HomeSummary {
        unattributed_history: false,
        dungeon_stats: None,
        runs: 0,
        clears: 0,
        deaths: 0,
        fame_delta: None,
        fame: vec![],
        fame_characters: vec![],
        best_drop: None,
        top_fight: None,
        recent_runs: vec![],
        issues: vec![],
        recent_details: vec![],
        fame_missing_baselines: 0,
        fame_sampled: false,
        unranked_drop_count: 0,
    }
}

#[cfg(test)]
mod maintenance_tests {
    use super::super::{
        history::{HistoryCaller, HistoryKey},
        maintenance_tests::{current_fixture, initialized},
    };
    use super::*;
    use maxed_types::HistoryReply;
    use std::future::Future;

    #[test]
    fn history_storage_admission_racing_preparation_keeps_context() {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let home = &runtime.shared.home;
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let (release, held) = crossbeam_channel::bounded(1);
        *home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
            entered.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(10)).unwrap();
        }));
        *home.on_storage_admission.lock().unwrap() = Some({
            let home = Arc::downgrade(home);
            let root = root.path().to_path_buf();
            Arc::new(move || {
                let home = home.upgrade().unwrap();
                {
                    let mut state = home.preparation.lock().unwrap();
                    state.running = true;
                    state.ready = false;
                }
                home.dispatch_preparation(home.generation.load(Ordering::Acquire), root.clone());
                waiting.recv_timeout(Duration::from_secs(5)).unwrap();
            })
        });
        let reader = runtime.clone();
        let mut read = Box::pin(runtime.submit_history(
            HistoryCaller::Storage,
            HistoryKey::MaintenanceJobs,
            move |job| reader.read_history(job, Vec::new, |r| r.maintenance_statuses()),
        ));
        let result = read
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()));
        release.send(()).unwrap();
        assert!(result.is_pending());
        assert!(matches!(
            tauri::async_runtime::block_on(read).unwrap(),
            HistoryReply::Ready { .. }
        ));
    }
}

#[cfg(test)]
mod account_scope_install_tests {
    use super::*;

    #[test]
    fn account_scope_install_retains_recovery_notice_revision() {
        let root = tempfile::tempdir().unwrap();
        let home = HomeReads::default();
        let context = HomeReadContext {
            database: root.path().join("absent.sqlite3"),
            namespace: Namespace::Live,
            generation: 0,
            stream_id: None,
            absent: true,
            account: None,
            account_revision: 0,
            identity_sequence: None,
        };
        // A recovery notice arrives after construction but before installation.
        home.account_revision.store(1, Ordering::Release);
        home.install(context).unwrap();
        assert_eq!(
            home.context
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .account_revision,
            1
        );
    }
}

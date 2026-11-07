//! Native account leases and cancellable, off-capture legacy recovery.
use super::{
    home::{HomeJob, HomeReads, UNAVAILABLE},
    CaptureRuntime, Namespace,
};
use maxed_types::{AccountHistoryChangeReason, AccountHistoryChanged, Sequence};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub(super) fn verified_account(event: &maxed_types::EngineEvent) -> Option<&str> {
    match event {
        maxed_types::EngineEvent::LocalIdentityChanged {
            account_id,
            character_id: Some(_),
            ..
        } => account_id.as_deref(),
        maxed_types::EngineEvent::CharacterUpdated(c) if c.identity_verified => {
            c.account_id.as_deref()
        }
        _ => None,
    }
}

impl HomeReads {
    pub(super) fn observe_account(
        &self,
        stream: Option<&str>,
        account: &str,
        sequence: Option<Sequence>,
    ) -> Option<AccountHistoryChanged> {
        let mut context = self.context.lock().ok()?;
        let context = context.as_mut()?;
        if context.stream_id.as_deref() != stream {
            return None;
        }
        if context.account.as_deref() == Some(account) {
            if sequence.is_some_and(|next| {
                context
                    .identity_sequence
                    .is_none_or(|old| next.0 .0 > old.0 .0)
            }) {
                context.identity_sequence = sequence;
            }
            return None;
        }
        // The scheduler lock orders final responses against switches.
        let mut queue = self.scheduler.lock().ok()?;
        queue.retire_accounts();
        context.account = Some(account.to_owned());
        *self.resolved_account.lock().ok()? = Some((context.namespace, context.account.clone()));
        context.identity_sequence = sequence;
        context.account_revision = self.account_revision.fetch_add(1, Ordering::AcqRel) + 1;
        Some(AccountHistoryChanged {
            revision: context.account_revision,
            reason: AccountHistoryChangeReason::AccountSwitched,
        })
    }
    fn recovered(&self) -> Option<AccountHistoryChanged> {
        let mut context = self.context.lock().ok()?;
        let mut queue = self.scheduler.lock().ok()?;
        queue.retire_accounts();
        let revision = self.account_revision.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(context) = context.as_mut() {
            context.account_revision = revision;
        }
        Some(AccountHistoryChanged {
            revision,
            reason: AccountHistoryChangeReason::LegacyRecovered,
        })
    }
}

impl CaptureRuntime {
    pub(super) fn read_account_history<T>(
        &self,
        job: HomeJob,
        empty: impl FnOnce() -> T,
        read: impl FnOnce(&maxed_store::StoreReader, Option<&str>) -> Result<T, maxed_store::StoreError>,
    ) -> Result<T, String> {
        let context = job.owner.reader_context(&job.context, true)?;
        let active_live = context.namespace == Namespace::Live
            && context.stream_id.is_some()
            && self
                .shared
                .views
                .store
                .lock()
                .map_err(|_| UNAVAILABLE)?
                .state
                != maxed_types::StoreState::Closed;
        // Never query a newly observed identity before its ownership projection commits.
        if let Some(sequence) = context.identity_sequence.filter(|_| active_live) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                if self.shared.home.account_revision.load(Ordering::Acquire)
                    != context.account_revision
                {
                    return Err(super::home::CHANGED.into());
                }
                let status = self
                    .shared
                    .views
                    .store
                    .lock()
                    .map_err(|_| UNAVAILABLE)?
                    .clone();
                if status.stream_id == context.stream_id
                    && status.durable.is_some_and(|n| n.0 .0 >= sequence.0 .0)
                {
                    break;
                }
                if status.state == maxed_types::StoreState::Failed
                    || std::time::Instant::now() >= deadline
                {
                    return Err(UNAVAILABLE.into());
                }
                #[cfg(test)]
                if let Some(hook) = self.shared.home.on_identity_wait.lock().unwrap().clone() {
                    hook();
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        self.read_history(job, empty, |reader| {
            reader.with_account(context.account.as_deref().filter(|_| active_live), read)
        })
    }
}

pub(super) struct Recovery {
    cancel: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    send: crossbeam_channel::Sender<String>,
    last_account: Option<String>,
}
impl Recovery {
    pub(super) fn start(
        home: Arc<HomeReads>,
        writer: Arc<maxed_store::StoreWriter>,
        path: std::path::PathBuf,
        key: [u8; 32],
        notice: Arc<dyn Fn(AccountHistoryChanged) + Send + Sync>,
    ) -> Option<Self> {
        if !home.context.lock().ok().is_some_and(|context| {
            context
                .as_ref()
                .is_some_and(|c| c.namespace == Namespace::Live && c.database == path)
        }) {
            return None;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let (send, receive) = crossbeam_channel::bounded::<String>(16);
        let generation = home.generation.load(Ordering::Acquire);
        let worker = std::thread::spawn(move || {
            let mut completed = std::collections::HashSet::new();
            while !stopped.load(Ordering::Acquire)
                && home.generation.load(Ordering::Acquire) == generation
            {
                let account = match receive.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(account) => account,
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                if home.generation.load(Ordering::Acquire) != generation {
                    break;
                }
                if completed.contains(&account) {
                    continue;
                }
                #[cfg(test)]
                if let Some(hook) = home.on_recovery_attempt.lock().unwrap().clone() {
                    hook();
                }
                let (reply, progress) = std::sync::mpsc::channel();
                home.send_reader(super::history::ReaderMessage::RecoveryProgress(
                    path.clone(),
                    generation,
                    account.clone(),
                    reply,
                ));
                let Ok(Ok(mut progress)) = progress.recv() else {
                    continue;
                };
                let mut attached = 0;
                while !stopped.load(Ordering::Acquire) {
                    let live = home.context.lock().ok().is_some_and(|c| {
                        c.as_ref().is_some_and(|c| {
                            c.namespace == Namespace::Live
                                && c.database == path
                                && c.generation == generation
                        })
                    });
                    if !live {
                        break;
                    }
                    let Some(batch) = super::account_batches::next_batch(progress) else {
                        completed.insert(account.clone());
                        break;
                    };
                    let end = *batch.end();
                    let ids: Option<Vec<_>> = batch
                        .map(|n| maxed_engine::character_id_for(&key, &account, i32::from(n)))
                        .collect();
                    let Some(ids) = ids else {
                        break;
                    };
                    let Ok(counts) = writer.attach_legacy_characters(&account, &ids) else {
                        break;
                    };
                    attached += counts.attached;
                    if writer.record_recovery_progress(&account, end).is_err() {
                        break;
                    }
                    progress = Some(end);
                    #[cfg(test)]
                    if let Some(hook) = home.on_recovery_batch.lock().unwrap().clone() {
                        hook(end);
                    }
                }
                if attached > 0 {
                    if let Some(changed) = home.recovered() {
                        notice(changed);
                    }
                }
            }
        });
        Some(Self {
            cancel,
            worker: Some(worker),
            send,
            last_account: None,
        })
    }
    /// One attempt per verified account change, including failed attempts.
    pub(super) fn observe(&mut self, account: &str) -> bool {
        if self.last_account.as_deref() == Some(account) {
            return false;
        }
        self.last_account = Some(account.to_owned());
        self.send.try_send(account.to_owned()).is_ok()
    }
    pub(super) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub(super) fn stop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Recovery {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
#[path = "account_history_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "account_recovery_tests.rs"]
mod recovery_tests;
#[cfg(test)]
#[path = "account_wait_tests.rs"]
mod wait_tests;

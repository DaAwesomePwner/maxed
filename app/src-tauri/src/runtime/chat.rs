//! Chat policy, Logs reads, retention and clearing. Errors are fixed strings.
use super::{CaptureRuntime, Namespace};
use maxed_types::*;

const DAY_MICROS: u64 = 86_400_000_000;

pub(super) fn cutoff(now: Timestamp, days: u16) -> Timestamp {
    Timestamp(now.0.saturating_sub(u64::from(days) * DAY_MICROS))
}

pub(super) fn wall_now() -> Timestamp {
    let micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    Timestamp(u64::try_from(micros).unwrap_or(u64::MAX))
}

pub(super) fn context(
    policy: &ChatSettings,
    message: &ChatMessageView,
    alertable: bool,
) -> maxed_store::ChatContext {
    maxed_store::ChatContext {
        store: policy.store.stores(message.channel),
        hidden: policy.hides(message),
        alertable,
    }
}

impl CaptureRuntime {
    pub async fn get_chat_overview(
        self: &std::sync::Arc<Self>,
        request: ChatOverviewRequest,
    ) -> Result<HistoryReply<ChatOverview>, String> {
        let runtime = self.clone();
        self.submit_history(
            super::history::HistoryCaller::MainWindow,
            super::history::HistoryKey::ChatOverview(request.clone()),
            move |job| {
                runtime.read_history(job, ChatOverview::default, |reader| {
                    reader.chat_overview(request)
                })
            },
        )
        .await
    }

    pub async fn get_chat_sender(
        self: &std::sync::Arc<Self>,
        request: ChatSenderRequest,
    ) -> Result<HistoryReply<Option<ChatSenderSummary>>, String> {
        let runtime = self.clone();
        self.submit_history(
            super::history::HistoryCaller::MainWindow,
            super::history::HistoryKey::ChatSender(request.clone()),
            move |job| runtime.read_history(job, || None, |reader| reader.chat_sender(request)),
        )
        .await
    }

    pub async fn list_whisper_partners(
        self: &std::sync::Arc<Self>,
        filter: WhisperPartnerFilter,
    ) -> Result<HistoryReply<WhisperPartners>, String> {
        let policy = self
            .shared
            .views
            .chat_policy
            .lock()
            .map_err(|_| "Chat unavailable")?
            .clone();
        let hidden = maxed_store::HiddenLists {
            senders: policy.hidden_senders,
            keywords: policy.hidden_keywords,
        };
        let runtime = self.clone();
        self.submit_history(
            super::history::HistoryCaller::MainWindow,
            super::history::HistoryKey::WhisperPartners(
                filter.clone(),
                hidden.senders.clone(),
                hidden.keywords.clone(),
            ),
            move |job| {
                runtime.read_history(job, WhisperPartners::default, |reader| {
                    reader.whisper_partners(filter, &hidden)
                })
            },
        )
        .await
    }

    pub async fn list_chat(
        self: &std::sync::Arc<Self>,
        filter: ChatFilter,
    ) -> Result<HistoryReply<ChatPage>, String> {
        let policy = self
            .shared
            .views
            .chat_policy
            .lock()
            .map_err(|_| "Chat unavailable")?
            .clone();
        let hidden = maxed_store::HiddenLists {
            senders: policy.hidden_senders,
            keywords: policy.hidden_keywords,
        };
        let runtime = self.clone();
        self.submit_history(
            super::history::HistoryCaller::MainWindow,
            super::history::HistoryKey::Chat(
                filter.clone(),
                hidden.senders.clone(),
                hidden.keywords.clone(),
            ),
            move |job| {
                runtime.read_history(
                    job,
                    || ChatPage {
                        rows: vec![],
                        next: None,
                    },
                    |reader| reader.list_chat(filter, &hidden),
                )
            },
        )
        .await
    }

    pub async fn get_event_summary(
        self: &std::sync::Arc<Self>,
        request: EventSummaryRequest,
    ) -> Result<HistoryReply<EventSummary>, String> {
        let runtime = self.clone();
        self.submit_history(
            super::history::HistoryCaller::MainWindow,
            super::history::HistoryKey::EventSummary(request.clone()),
            move |job| {
                runtime.read_history(job, EventSummary::default, |reader| {
                    reader.event_summary(request)
                })
            },
        )
        .await
    }

    pub async fn list_instance_events(
        self: &std::sync::Arc<Self>,
        filter: EventFilter,
    ) -> Result<HistoryReply<EventPage>, String> {
        let runtime = self.clone();
        self.submit_history(
            super::history::HistoryCaller::MainWindow,
            super::history::HistoryKey::Events(filter.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || EventPage {
                        entered_runs: vec![],
                        rows: vec![],
                        next: None,
                    },
                    |reader| reader.list_events(filter),
                )
            },
        )
        .await
    }

    pub fn clear_chat_history(&self) -> Result<(), String> {
        self.clear_chat_history_with_timeout(std::time::Duration::from_secs(60))
    }

    pub(super) fn clear_chat_history_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let alerts = self.shared.alerts.lock().map_err(|_| "Chat unavailable")?;
        if alerts.namespace != Namespace::Live {
            return Err("Chat history can only be cleared in live history.".into());
        }
        drop(alerts);
        let path = self.shared.home.live_database().ok_or("Chat unavailable")?;
        let root = path.parent().ok_or("Chat unavailable")?.to_path_buf();
        const FAILED: &str = "Chat history could not be cleared.";
        let mut timeout = timeout;
        let lease = super::chat::history_write_lease(self, &root, &mut timeout);
        match lease {
            Ok(_lease) => {
                while maxed_store::maintain_chat_file(&path, maxed_store::ChatMaintenance::Clear)
                    .map_err(|_| FAILED)?
                    >= u64::from(maxed_store::CHAT_CLEAR_BATCH)
                {}
                Ok(())
            }
            Err(_) => {
                // A cold preparation or parked adapter can hold the lease with
                // no worker left to drain a request.
                if !self.is_running() {
                    return Err(FAILED.into());
                }
                let (send, recv) = crossbeam_channel::bounded(1);
                let mut pending = self
                    .shared
                    .views
                    .chat_clear
                    .lock()
                    .map_err(|_| "Chat unavailable")?;
                if pending.is_some() {
                    return Err(FAILED.into());
                }
                *pending = Some(send.clone());
                drop(pending);
                match recv.recv_timeout(timeout) {
                    Ok(result) => result,
                    Err(_) => {
                        // Withdrawing under the same lock used by drain makes
                        // failure final: this request can never execute later.
                        let mut pending = self
                            .shared
                            .views
                            .chat_clear
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner());
                        if pending
                            .as_ref()
                            .is_some_and(|request| request.same_channel(&send))
                        {
                            pending.take();
                            return Err(FAILED.into());
                        }
                        drop(pending);
                        // The adapter owns this request now. Report its actual
                        // result, and let adapter loss disconnect the receiver.
                        drop(send);
                        recv.recv().map_err(|_| FAILED.to_string())?
                    }
                }
            }
        }
    }

    pub fn update_chat_settings(&self, settings: ChatSettings) -> Result<(), String> {
        if !settings.valid() {
            return Err(crate::settings::SETTINGS_INVALID.into());
        }
        // Same lock order as alert preferences: alerts state, then settings, then policy.
        let alerts = self
            .shared
            .alerts
            .lock()
            .map_err(|_| "Chat settings unavailable")?;
        if alerts.namespace != Namespace::Live {
            return Err("Chat settings cannot be changed during preview or replay.".into());
        }
        let store = alerts
            .settings
            .as_ref()
            .ok_or("Chat settings unavailable")?;
        store.put_chat(settings.clone())?;
        *self
            .shared
            .views
            .chat_policy
            .lock()
            .map_err(|_| "Chat settings unavailable")? = settings;
        Ok(())
    }
}

/// Only the stopped maintenance owner is expected to release its lease without
/// user action. Parked adapters and cold preparation must still fail promptly.
pub(super) fn history_write_lease(
    runtime: &CaptureRuntime,
    root: &std::path::Path,
    timeout: &mut std::time::Duration,
) -> Result<std::fs::File, super::storage::LeaseError> {
    use super::storage::{try_namespace_lease, LeaseError};
    use std::time::{Duration, Instant};
    let started = Instant::now();
    let mut waited = false;
    let service = &runtime.shared.views.maintenance;
    // halted persists after an ordinary stop. Preserve fresh cold writes, but
    // a command joining maintenance must not compete with its next owner.
    let mut joined_maintenance = service.owns_stopped_namespace();
    let attempt_lease = |joined: bool| {
        if joined && service.background_halted() {
            return Err(LeaseError::Contended);
        }
        let attempt = try_namespace_lease(root, Namespace::Live);
        if joined && service.background_halted() {
            // Handoff may have begun during the open. Never perform the write
            // with a lease intended for capture or history upgrade.
            drop(attempt);
            return Err(LeaseError::Contended);
        }
        attempt
    };
    let result = loop {
        // This also makes the final attempt after the last bounded sleep.
        let attempt = attempt_lease(joined_maintenance);
        if attempt.is_ok() {
            break attempt;
        }
        // Capture may have taken over while we waited. Let the caller use its
        // existing queue and withdrawal protocol with the remaining timeout.
        if runtime.is_running() || !matches!(attempt, Err(LeaseError::Contended)) {
            break attempt;
        }
        if joined_maintenance && service.background_halted() {
            break attempt;
        }
        if !service.owns_stopped_namespace() {
            // The owner may have released between the failed open and the flag
            // read. Try once more; never wait for a different owner.
            break attempt_lease(joined_maintenance);
        }
        joined_maintenance = true;
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break attempt;
        }
        waited = true;
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    };
    if waited {
        *timeout = timeout.saturating_sub(started.elapsed());
    }
    result
}

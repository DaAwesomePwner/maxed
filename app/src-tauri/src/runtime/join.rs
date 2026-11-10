//! One ordered engine stream, its durable writer, and its presentation watermark.
use super::{pipeline::PipelineEvent, sanitize};
use maxed_assets::CachedCatalog;
use maxed_engine::{Engine, EngineConfig};
use maxed_protocol::DecodeEvent;
use maxed_store::{start_writer, StoreWriter, SubmitError, WriterConfig};
use maxed_types::*;
use std::{
    collections::VecDeque,
    fs::File,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(super) struct Bridge {
    pub account_changed: Arc<dyn Fn(AccountHistoryChanged) + Send + Sync>,
    pub visibility: Arc<crate::bridge::PresentationVisibility>,
    // Full canonical envelopes, also used by replay consumers as store inputs.
    pub event: Arc<dyn Fn(EventEnvelope) + Send + Sync>,
    // Native presentation delivery; copies may carry presentation-only metadata.
    pub batch: Option<Arc<dyn Fn(Vec<EventEnvelope>) + Send + Sync>>,
    pub snapshot: Arc<dyn Fn(LiveSnapshot) + Send + Sync>,
    pub party_changed: Arc<dyn Fn(crate::bridge::PartyChanged) + Send + Sync>,
}
impl Default for Bridge {
    fn default() -> Self {
        Self {
            account_changed: Arc::new(|_| {}),
            visibility: Arc::default(),
            event: Arc::new(|_| {}),
            batch: None,
            snapshot: Arc::new(|_| {}),
            party_changed: Arc::new(|_| {}),
        }
    }
}

pub(super) struct Views {
    pub maintenance: Arc<super::maintenance::StorageService>,
    pub delivery: Mutex<super::alerts::delivery::Delivery>,
    pub alert_wake: Arc<super::alerts::delivery::DeliveryWake>,
    pub alert_policy: Arc<Mutex<AlertPreferences>>,
    pub chat_policy: Arc<Mutex<ChatSettings>>,
    pub chat_clear: Mutex<Option<crossbeam_channel::Sender<Result<(), String>>>>,
    pub snapshot: Mutex<LiveSnapshot>,
    pub character: Mutex<super::character::CharacterViewState>,
    pub party: Mutex<super::party::PartyRoster>,
    pub party_write: Mutex<Option<super::party::PartyWriteRequest>>,
    pub party_catalog: Mutex<Option<Arc<super::party::PartyCatalog>>>,
    pub store: Mutex<StoreStatus>,
}
impl Default for Views {
    fn default() -> Self {
        let alert_wake = Arc::new(super::alerts::delivery::DeliveryWake::default());
        Self {
            maintenance: Arc::new(super::maintenance::StorageService::default()),
            delivery: Mutex::new(super::alerts::delivery::Delivery::with_wake(
                alert_wake.clone(),
            )),
            alert_wake,
            alert_policy: Arc::new(Mutex::new(AlertPreferences::default())),
            chat_policy: Arc::new(Mutex::new(ChatSettings::default())),
            chat_clear: Mutex::new(None),
            character: Mutex::new(super::character::CharacterViewState::default()),
            party: Mutex::new(super::party::PartyRoster::default()),
            party_write: Mutex::new(None),
            party_catalog: Mutex::new(None),
            snapshot: Mutex::new(LiveSnapshot {
                revision: None,
                instance: None,
                run_totals: None,
                at: Timestamp(0),
                stream_id: String::new(),
                sequence: Sequence(Timestamp(0)),
                capture: CaptureStatus::default(),
                session: None,
                character: None,
                map: None,
                area_kind: AreaKind::Unknown,
                run: None,
                fights: vec![],
                drops: vec![],
                issues: vec![],
            }),
            store: Mutex::new(StoreStatus {
                state: StoreState::Closed,
                queued: 0,
                durable: None,
                stream_id: None,
                message: "Summary storage is not open".into(),
            }),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct StoreInputs {
    writer: usize,
    generation: u64,
    pending: usize,
    rejected: bool,
    notice: Option<&'static str>,
}

pub(super) struct Adapter {
    store_inputs: std::cell::Cell<Option<StoreInputs>>,
    catalog_prefix: std::cell::RefCell<String>,
    maintenance_work: super::maintenance::OwnerWork,
    pub engine: Engine,
    writer: Option<Arc<StoreWriter>>,
    recovery: Option<super::account_history::Recovery>,
    identity_key: [u8; 32],
    recovery_account: Option<String>,
    identity_acquisition: super::account_history::IdentityAcquisition,
    path: PathBuf,
    writer_config: WriterConfig,
    pending: VecDeque<maxed_store::ContextualEvent>,
    bridge: Bridge,
    views: Arc<Views>,
    missing_catalog: bool,
    character_catalog: Option<Arc<Catalog>>,
    character_generation: Arc<()>,
    finalized: bool,
    gate: crate::bridge::SnapshotGate,
    snapshot_dirty: bool,
    snapshot_revision: u64,
    rejected: bool,
    at: Timestamp,
    sequence: Sequence,
    stream_id: String,
    capture_status: CaptureStatus,
    claimed_through: Option<Sequence>,
    chat_prune_due: Option<std::time::Instant>,
    pub catalog_notice: Option<&'static str>,
    pub ephemeral: bool,
    pub live: bool,
    pub alert_policy: Arc<Mutex<AlertPreferences>>,
    pub chat_policy: Arc<Mutex<ChatSettings>>,
    pub(super) home: Option<Arc<super::home::HomeReads>>,
    pub(super) alert_calendar: Arc<dyn super::alerts::AlertCalendar>,
    #[cfg(test)]
    pub snapshot_builds: usize,
    // Keep both cross-process namespace ownership and the validated cache lease.
    pub lease: Option<File>,
    pub catalog_lease: Option<CachedCatalog>,
}

impl Adapter {
    pub(super) fn initialize_namespace(&mut self, namespace: super::storage::Namespace) {
        use super::storage::Namespace;
        // The capture owner holds the namespace lease here, after the writer's
        // normal migration/recovery and before any packets or maintenance hand-off.
        // This open path runs on the native worker, never the UI thread.
        if namespace == Namespace::Live {
            let definitions = maxed_store::StoreReader::read_crucible_definitions_file(&self.path);
            match definitions {
                Ok(definitions) => self.engine.seed_crucible_definitions(definitions),
                Err(_) => {
                    eprintln!("Saved Crucible definitions unavailable; continuing capture")
                }
            }
        }
        self.live = namespace == Namespace::Live;
        self.ephemeral = namespace != Namespace::Live;
    }

    pub(super) fn committed(&self) -> crossbeam_channel::Receiver<()> {
        self.writer
            .as_ref()
            .expect("active summary writer")
            .committed()
    }

    #[cfg(test)]
    pub(super) fn writer_flush_for_test(&self) {
        self.writer.as_ref().unwrap().flush().unwrap();
    }
    #[cfg(test)]
    pub(super) fn maintenance_writer_for_test(&self) -> &StoreWriter {
        self.writer.as_ref().unwrap()
    }
    #[cfg(test)]
    pub(super) fn submit_pending_for_test(&mut self) {
        self.submit_pending().unwrap();
    }
    pub fn open(
        path: PathBuf,
        catalog: Arc<Catalog>,
        config: EngineConfig,
        writer_config: WriterConfig,
        missing_catalog: bool,
        views: Arc<Views>,
        bridge: Bridge,
    ) -> Result<Self, String> {
        let stream_id = config.stream_id.clone();
        let identity_key = config.identity_key;
        let engine = Engine::new(config, catalog.clone())
            .map_err(|_| "Catalog could not initialize the engine")?;
        let writer =
            start_writer(path.clone(), writer_config.clone()).map_err(|e| e.to_string())?;
        // A new allocator must have a new stream ID; resuming an old engine is unsupported.
        if writer.status().durable.is_some() {
            let _ = writer.shutdown();
            return Err("A new capture requires a new stream namespace".into());
        }
        *views.party.lock().unwrap_or_else(|p| p.into_inner()) = Default::default();
        let mut adapter = Self {
            store_inputs: std::cell::Cell::new(None),
            catalog_prefix: std::cell::RefCell::new(String::new()),
            maintenance_work: super::maintenance::OwnerWork::default(),
            engine,
            writer: Some(Arc::new(writer)),
            recovery: None,
            identity_key,
            recovery_account: None,
            identity_acquisition: Default::default(),
            path,
            writer_config,
            pending: VecDeque::new(),
            bridge,
            alert_policy: views.alert_policy.clone(),
            chat_policy: views.chat_policy.clone(),
            views,
            missing_catalog,
            character_catalog: (!missing_catalog).then(|| catalog.clone()),
            character_generation: Arc::new(()),
            finalized: false,
            gate: crate::bridge::SnapshotGate {
                dirty: true,
                ..Default::default()
            },
            snapshot_dirty: true,
            snapshot_revision: 0,
            rejected: false,
            at: Timestamp(0),
            sequence: Sequence(Timestamp(0)),
            stream_id,
            capture_status: CaptureStatus::default(),
            claimed_through: None,
            chat_prune_due: None,
            catalog_notice: None,
            ephemeral: false,
            live: false,
            home: None,
            alert_calendar: Arc::new(super::alerts::calendar::LocalAlertCalendar),
            #[cfg(test)]
            snapshot_builds: 0,
            lease: None,
            catalog_lease: None,
        };
        adapter.share_party_catalog(catalog);
        adapter.publish_snapshot();
        adapter.refresh_store();
        Ok(adapter)
    }

    pub fn refresh_store(&self) {
        let inputs = StoreInputs {
            writer: self
                .writer
                .as_ref()
                .map_or(0, |writer| Arc::as_ptr(writer) as usize),
            generation: self
                .writer
                .as_ref()
                .map_or(0, |writer| writer.status_generation()),
            pending: self.pending.len(),
            rejected: self.rejected,
            notice: self.catalog_notice,
        };
        let previous = self.store_inputs.get();
        if previous == Some(inputs) {
            return;
        }
        if previous.is_none_or(|old| old.notice != inputs.notice) {
            *self.catalog_prefix.borrow_mut() = inputs
                .notice
                .map_or_else(String::new, |notice| format!("{notice} "));
        }
        self.store_inputs.set(Some(inputs));
        if let Some(writer) = &self.writer {
            let mut status = writer.status();
            status.queued = status.queued.saturating_add(self.pending.len() as u32);
            if !self.pending.is_empty() && status.state == StoreState::Ready {
                status.state = StoreState::Backpressured;
                status.message = "Changes not saved yet; capture consumption is paused".into();
            }
            if self.rejected {
                status.state = StoreState::Failed;
                status.message =
                    "Summary event could not be saved; pending changes are retained".into();
            }
            if self.catalog_notice.is_some() {
                status.message.insert_str(0, &self.catalog_prefix.borrow());
            }
            *self.views.store.lock().unwrap_or_else(|p| p.into_inner()) = status;
        } else if !self.pending.is_empty() {
            let mut status = self.views.store.lock().unwrap_or_else(|p| p.into_inner());
            status.state = StoreState::Failed;
            status.queued = self.pending.len() as u32;
            status.message = "Summary writer stopped; retained changes are not saved".into();
        }
    }

    fn partial(&self, event: &mut EngineEvent) {
        if !self.missing_catalog {
            return;
        }
        match event {
            EngineEvent::SessionUpdated(v) => add_missing(&mut v.issues),
            EngineEvent::RunStarted(v) | EngineEvent::RunUpdated(v) | EngineEvent::RunEnded(v) => {
                v.complete_capture = false;
                add_missing(&mut v.issues);
            }
            EngineEvent::FightStarted(v)
            | EngineEvent::FightUpdated(v)
            | EngineEvent::FightEnded(v) => {
                v.complete_capture = false;
                add_missing(&mut v.issues);
            }
            EngineEvent::CharacterUpdated(v) => add_missing(&mut v.issues),
            EngineEvent::BagDropped(v) | EngineEvent::DropUpdated(v) => add_missing(&mut v.issues),
            _ => {}
        }
    }

    pub(super) fn events(&mut self, events: Vec<EngineEvent>) {
        self.events_at(events, None);
    }

    fn events_at(&mut self, events: Vec<EngineEvent>, at: Option<Timestamp>) {
        // Engine metadata (e.g. last traffic) can change without a domain event.
        self.snapshot_dirty = true;
        self.gate.dirty = true;
        // Feed both paths under one lock, before publishing the complete batch.
        // Presence and roster profiles never enter the durable writer queue.
        let presence = self.engine.take_roster_presence();
        let profiles = self.engine.take_roster_profiles();
        let definitions = self.engine.take_crucible_definitions();
        if self.live && !definitions.is_empty() {
            if let Some(writer) = &self.writer {
                writer.try_save_crucible_definitions(definitions, at.unwrap_or(self.at));
            }
        }
        if !events.is_empty() || !profiles.is_empty() || !presence.is_empty() {
            let mut party = self.views.party.lock().unwrap_or_else(|p| p.into_inner());
            let mut presence = presence.into_iter().peekable();
            for (index, event) in events.iter().enumerate() {
                while presence
                    .peek()
                    .is_some_and(|update| update.event_index == index)
                {
                    party.observe_presence(&presence.next().unwrap().presence);
                }
                party.apply(event);
            }
            for update in presence {
                party.observe_presence(&update.presence);
            }
            for profile in profiles {
                party.observe_profile(profile);
            }
        }
        if at.is_none() && self.stream_id.starts_with("live-") {
            self.views
                .delivery
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observe_clock(&self.stream_id, self.at, std::time::Instant::now());
        }
        if events.is_empty() {
            return;
        }
        // Pin acquisition reads to the complete engine batch, including the later
        // LocalIdentityChanged projection following CharacterUpdated.
        let identity_watermark = Sequence(Timestamp(self.sequence.0 .0 + events.len() as u64));
        let mut envelopes = Vec::with_capacity(events.len());
        for mut event in events {
            self.partial(&mut event);
            let acquired = self.identity_acquisition.observe(&event);
            let account = super::account_history::verified_account(&event);
            if let (Some(account), Some(home)) = (account, &self.home) {
                if let Some(notice) = home.observe_account(
                    Some(&self.stream_id),
                    account,
                    acquired.then_some(identity_watermark),
                ) {
                    (self.bridge.account_changed)(notice);
                }
                if self.live && self.recovery_account.as_deref() != Some(account) {
                    if self.recovery.is_none() {
                        if let Some(writer) = &self.writer {
                            self.recovery = super::account_history::Recovery::start(
                                home.clone(),
                                writer.clone(),
                                self.path.clone(),
                                self.identity_key,
                                self.bridge.account_changed.clone(),
                            );
                        }
                    }
                    if let Some(recovery) = &mut self.recovery {
                        recovery.observe(account);
                        self.recovery_account = Some(account.to_owned());
                    } else {
                        // A failed start must not suppress the next acquisition.
                        self.recovery_account = None;
                    }
                }
            }
            if let EngineEvent::CaptureStateChanged(status) = &event {
                self.capture_status = status.clone();
            }
            self.sequence.0 .0 += 1;
            let mut envelope = EventEnvelope {
                fight_chart_omitted: false,
                stream_id: self.stream_id.clone(),
                sequence: self.sequence,
                at: at.unwrap_or(self.at),
                event,
            };
            // Retain the complete ordered batch before presentation. Acceptance is
            // volatile; StoreStatus.durable is the only persistence acknowledgment.
            let context = maxed_store::AlertContext {
                chat: match &envelope.event {
                    EngineEvent::Chat(message) => Some(super::chat::context(
                        &self.chat_policy.lock().unwrap_or_else(|p| p.into_inner()),
                        message,
                        self.live && self.engine.chat_alertable(),
                    )),
                    _ => None,
                },
                live: self.live,
                key_pop_dungeon: if !self.live {
                    None
                } else {
                    self.engine.key_pop_dungeon(&envelope.event)
                },
                preferences: {
                    let policy = self.alert_policy.lock().unwrap_or_else(|p| p.into_inner());
                    if maxed_store::AlertContext::uses_preferences(&envelope.event)
                        || !policy.valid()
                    {
                        policy.clone()
                    } else {
                        AlertPreferences::disabled()
                    }
                },
                bag_window: match &envelope.event {
                    EngineEvent::BagDropped(drop) | EngineEvent::DropUpdated(drop) => {
                        self.alert_calendar.seven_day_window(drop.at).ok()
                    }
                    _ => None,
                },
            };
            if let Some(writer) = &self.writer {
                writer.set_maintenance_blocked(true);
            }
            // Update the calculator's separate view without changing full events.
            match &envelope.event {
                EngineEvent::CharacterUpdated(character) => {
                    self.observe_character(Some(character.clone()));
                }
                EngineEvent::CaptureStateChanged(_) => {
                    let input = self
                        .views
                        .character
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .input
                        .as_ref()
                        .filter(|input| Arc::ptr_eq(&input.generation, &self.character_generation))
                        .cloned();
                    if let Some(input) = input {
                        self.observe_character(input.character.clone());
                    }
                }
                _ => {}
            }
            // perf-live "before" mode measures the old full-clone path. The check
            // lives here because payload.rs is also compiled into maxed-cli tests.
            #[cfg(feature = "perf-live")]
            let full_clone = super::perf::before();
            #[cfg(not(feature = "perf-live"))]
            let full_clone = false;
            let presentation = if self.bridge.batch.is_some() && !full_clone {
                crate::bridge::payload::presentation_copy(&mut envelope)
            } else {
                // Replay consumers use canonical events as storage inputs.
                envelope.clone()
            };
            self.pending.push_back(maxed_store::ContextualEvent {
                event: envelope,
                context,
            });
            envelopes.push(presentation);
        }
        // The original full events and alert contexts are retained first. Only
        // presentation delivery changes; writer ordering/backpressure is intact.
        if let Some(batch) = &self.bridge.batch {
            // Annotate only native presentation copies, after forming the writer's
            // envelopes. Each event uses its own observation and this stream's catalog.
            for envelope in &mut envelopes {
                if let EngineEvent::CharacterUpdated(character) = &mut envelope.event {
                    character.calculation_revision = Some(maxed_engine::character::input_revision(
                        &envelope.stream_id,
                        character,
                        self.character_catalog.as_deref(),
                    ));
                }
            }
            batch(envelopes);
        } else {
            for envelope in envelopes {
                (self.bridge.event)(envelope);
            }
        }
        self.engine.set_watermark(self.sequence);
        self.gate.dirty = true;
    }

    pub fn drain(&mut self) -> Result<(), String> {
        self.submit_pending()?;
        // One coalesced commit wake must service every page already made durable.
        while self.drain_alert_claims()? == 32 {}
        self.submit_pending()?;
        self.service_maintenance();
        Ok(())
    }

    pub fn service_maintenance(&mut self) {
        self.service_chat_maintenance();
        self.service_party_write();
        if self.live {
            if let Some(writer) = &self.writer {
                writer.set_maintenance_blocked(
                    !self.pending.is_empty() || self.maintenance_work.closed(),
                );
                self.maintenance_work
                    .service_live(writer, &self.views.maintenance);
            }
        }
    }

    fn service_party_write(&mut self) {
        if !self.live {
            return;
        }
        let Some(writer) = &self.writer else { return };
        let request = self
            .views
            .party_write
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(request) = request {
            let result = request
                .operation
                .apply(writer)
                .map_err(|_| super::party::WRITE_FAILED.into());
            if result.is_ok() {
                self.views
                    .party
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .mark_dirty();
            }
            let _ = request.reply.send(result);
        }
    }

    fn share_party_catalog(&self, catalog: Arc<Catalog>) {
        let mut shared = self
            .views
            .party_catalog
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if self.missing_catalog {
            *shared = None;
        } else if shared
            .as_ref()
            .is_none_or(|previous| !Arc::ptr_eq(&previous.catalog, &catalog))
        {
            *shared = Some(Arc::new(super::party::PartyCatalog::new(catalog)));
        }
    }

    fn publish_party_changed(&self) {
        let notice = self
            .views
            .party
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take_changed();
        if let Some(notice) = notice {
            (self.bridge.party_changed)(notice);
        }
    }

    pub fn service_chat_maintenance(&mut self) {
        if !self.live {
            return;
        }
        let Some(writer) = self.writer.as_ref() else {
            return;
        };
        let reply = self
            .views
            .chat_clear
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(reply) = reply {
            let result = loop {
                match writer.maintain_chat(maxed_store::ChatMaintenance::Clear) {
                    Ok(n) if n < u64::from(maxed_store::CHAT_CLEAR_BATCH) => break Ok(()),
                    Ok(_) => continue,
                    Err(_) => break Err("Chat history could not be cleared.".to_string()),
                }
            };
            let _ = reply.send(result);
        }
        // The namespace is assigned after Adapter::open; arm on the first live maintenance call.
        let due = self
            .chat_prune_due
            .get_or_insert_with(std::time::Instant::now);
        if std::time::Instant::now() >= *due {
            let days = self
                .chat_policy
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .retention_days;
            let before = super::chat::cutoff(super::chat::wall_now(), days);
            // At most two batches per call; an unfinished pass continues next call.
            let mut finished = false;
            for _ in 0..2 {
                match writer.maintain_chat(maxed_store::ChatMaintenance::Prune { before }) {
                    Ok(n) if n < u64::from(maxed_store::CHAT_PRUNE_BATCH) => {
                        finished = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        finished = true;
                        break;
                    }
                }
            }
            if finished {
                self.chat_prune_due =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3600));
            }
        }
    }

    fn submit_pending(&mut self) -> Result<(), String> {
        let writer = self.writer.as_ref().ok_or("Summary writer is closed")?;
        writer.set_maintenance_blocked(!self.pending.is_empty() || self.maintenance_work.closed());
        while let Some(pending) = self.pending.pop_front() {
            let context = pending.context;
            match writer.try_submit_with_alert_context(pending.event, context.clone()) {
                Ok(()) => {}
                Err(SubmitError::Full(event)) => {
                    self.pending
                        .push_front(maxed_store::ContextualEvent { event, context });
                    self.refresh_store();
                    // Only the orchestration worker waits. No further engine batch
                    // or capture frame is consumed until this batch is accepted.
                    writer.flush().map_err(|e| e.to_string())?;
                    std::thread::yield_now();
                }
                Err(
                    SubmitError::Failed(event)
                    | SubmitError::Closed(event)
                    | SubmitError::Invalid(event),
                ) => {
                    self.pending
                        .push_front(maxed_store::ContextualEvent { event, context });
                    self.rejected = true;
                    self.refresh_store();
                    return Err("Summary storage failed; pending changes are not saved".into());
                }
            }
        }
        self.rejected = false;
        writer.set_maintenance_blocked(self.maintenance_work.closed());
        self.refresh_store();
        Ok(())
    }

    fn drain_alert_claims(&mut self) -> Result<usize, String> {
        let writer = self.writer.as_ref().ok_or("Summary writer is closed")?;
        let head = writer.status().durable;
        if head.is_none() || head == self.claimed_through {
            return Ok(0);
        }
        let views = self.views.clone();
        let mut delivery = views
            .delivery
            .lock()
            .map_err(|_| "Alert delivery unavailable")?;
        let mut count = 0;
        {
            let claims = self
                .writer
                .as_ref()
                .ok_or("Summary writer is closed")?
                .claim_alerts(32)
                .map_err(|e| e.to_string())?;
            let length = claims.len();
            for alert in claims {
                let policy = self
                    .alert_policy
                    .lock()
                    .map_err(|_| "Alert policy unavailable")?
                    .clone();
                let at = if self.stream_id.starts_with("live-") {
                    delivery
                        .event_now(&self.stream_id, std::time::Instant::now())
                        .unwrap_or(Timestamp(0))
                } else {
                    self.at
                };
                let mut view = alert.view.clone();
                view.presentation = Some(super::alerts::delivery::effective(&alert, &policy, at));
                delivery.enqueue(alert.clone(), &policy, at, std::time::Instant::now());
                self.events_at(
                    vec![EngineEvent::AlertFired(view)],
                    Some(alert.committed_at),
                );
                count += 1;
            }
            if length < 32 {
                self.claimed_through = head;
            }
        }
        Ok(count)
    }

    fn flush_alerts(&mut self) -> Result<(), String> {
        loop {
            self.submit_pending()?;
            self.writer
                .as_ref()
                .ok_or("Summary writer is closed")?
                .flush()
                .map_err(|e| e.to_string())?;
            if self.drain_alert_claims()? == 0 {
                break;
            }
        }
        Ok(())
    }

    pub fn traffic(&mut self, at: Timestamp) -> Result<(), String> {
        self.at = self.at.max(at);
        let events = self.engine.observe_traffic(at);
        self.events(events);
        self.drain()
    }
    pub fn live_traffic(&mut self, at: Timestamp) -> Result<(), String> {
        self.at = self.at.max(at);
        let events = self.engine.observe_live_traffic(at);
        self.events(events);
        self.drain()
    }
    pub fn decoded(&mut self, event: PipelineEvent) -> Result<(), String> {
        self.decoded_inner(event, false)
    }
    pub fn decoded_observed(&mut self, event: PipelineEvent) -> Result<(), String> {
        self.decoded_inner(event, true)
    }
    fn decoded_inner(&mut self, event: PipelineEvent, observed: bool) -> Result<(), String> {
        let events = match event {
            PipelineEvent::Opened { .. } => return Ok(()),
            PipelineEvent::Decoded(DecodeEvent::Packet(packet)) => {
                let at = packet.at;
                let verified = packet.alignment_verified;
                let events = if observed {
                    self.engine.apply_observed(packet)
                } else {
                    self.engine.apply(packet)
                };
                if verified {
                    self.at = self.at.max(at);
                }
                events
            }
            PipelineEvent::Decoded(DecodeEvent::Diagnostic {
                conn,
                dir,
                at,
                kind,
            }) => {
                self.at = self.at.max(at);
                self.engine.diagnostic(conn, dir, at, kind)
            }
        };
        self.events(events);
        self.drain()
    }
    #[cfg(test)]
    pub fn tick(&mut self, at: Timestamp) -> Result<(), String> {
        self.at = self.at.max(at);
        let events = self.engine.advance_to(at);
        self.events(events);
        self.drain()?;
        Ok(())
    }
    pub fn live_tick(&mut self, at: Timestamp, quiet: std::time::Duration) -> Result<(), String> {
        self.at = self.at.max(at);
        let events = self.engine.advance_live_to(self.at, quiet);
        // An event-free clock tick changes nothing presented. Keep the gate's
        // prior state so idle sessions do not republish at the 1 Hz floor.
        let gate_dirty = self.gate.dirty;
        let idle = events.is_empty();
        self.events(events);
        if idle {
            self.gate.dirty = gate_dirty;
        }
        self.drain()
    }
    pub fn capture(&mut self, mut status: CaptureStatus) -> Result<(), String> {
        status.message = sanitize(&status.state).into();
        // Native readiness/counters and engine diagnostics have different owners.
        // A wording difference alone must not duplicate the same capture event.
        if self.capture_status.state != status.state
            || self.capture_status.dropped_frames != status.dropped_frames
            || self.capture_status.unknown_packets != status.unknown_packets
            || self.capture_status.decode_failures != status.decode_failures
        {
            if self.capture_status.state == status.state
                && matches!(
                    status.state,
                    CaptureState::Live | CaptureState::Recovering | CaptureState::WaitingForGame
                )
                && !self.capture_status.message.is_empty()
                && self.capture_status.message != sanitize(&self.capture_status.state)
            {
                status.message = self.capture_status.message.clone();
            }
            let events = self.engine.capture_changed(status);
            self.events(events);
        }
        self.drain()
    }
    pub fn at(&self) -> Timestamp {
        self.at
    }

    pub fn publish_due(&mut self, now: std::time::Duration) {
        self.publish_due_at(now, self.at);
    }

    /// `activity_at` is capture time; `now` is monotonic presentation elapsed time.
    pub fn publish_due_at(&mut self, now: std::time::Duration, activity_at: Timestamp) {
        // Live restore/read requests materialize on demand. Keep both dirty
        // flags and the gate deadline intact until the window becomes visible.
        // Demo/replay still need paced cache refreshes for their direct reads.
        if self.live
            && !self
                .bridge
                .visibility
                .accepts(self.bridge.visibility.epoch())
        {
            self.publish_party_changed();
            return;
        }
        let last_hit_age = self
            .engine
            .latest_open_fight_hit()
            .map(|at| std::time::Duration::from_micros(activity_at.0.saturating_sub(at.0)));
        if self.gate.take_due_with_hit_age(now, last_hit_age) {
            self.gate.dirty = true;
            self.publish_snapshot();
        }
        self.publish_party_changed();
    }

    pub fn publish_snapshot(&mut self) {
        if !self.gate.dirty {
            self.publish_party_changed();
            return;
        }
        let snapshot = self.read_snapshot();
        (self.bridge.snapshot)(snapshot);
        self.gate.dirty = false;
        self.publish_party_changed();
    }

    pub(super) fn read_snapshot(&mut self) -> LiveSnapshot {
        self.materialize_snapshot();
        self.views
            .snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(super) fn read_character_identity(&mut self) -> super::character::CharacterIdentity {
        // Refresh pending metadata exactly as a snapshot read does, but never
        // clone the cached fights, series, buckets or drops for this query.
        self.materialize_snapshot();
        super::character::CharacterIdentity::from_snapshot(
            &self
                .views
                .snapshot
                .lock()
                .unwrap_or_else(|p| p.into_inner()),
        )
    }

    fn observe_character(&self, character: Option<CharacterView>) -> Option<CharacterView> {
        // Engine finalization clears world state. Keep the last presentation
        // observation only in that retiring generation, with its immutable catalog.
        // A newly opened adapter must still publish an empty initial observation.
        let character = character.or_else(|| {
            if !self.finalized {
                return None;
            }
            self.views
                .character
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .input
                .as_ref()
                .filter(|input| {
                    Arc::ptr_eq(&input.generation, &self.character_generation)
                        && input.stream_id == self.stream_id
                })
                .and_then(|input| input.character.clone())
        });
        let mut capture = self.capture_status.clone();
        if self.finalized {
            // Finalization precedes native Stopped delivery. Calculations must
            // already be LastSeen (or Died), even if the last capture state is Live.
            capture.state = CaptureState::Stopped;
        }
        let input = super::character::CharacterInputs::new(
            self.character_generation.clone(),
            self.stream_id.clone(),
            character,
            self.character_catalog.clone(),
            capture,
        );
        let character = input.character.clone();
        self.views
            .character
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observe(input);
        character
    }

    fn materialize_snapshot(&mut self) {
        if !self.snapshot_dirty {
            return;
        }
        #[cfg(test)]
        {
            self.snapshot_builds += 1;
        }
        #[cfg(not(feature = "perf-live"))]
        let mut snapshot = self.engine.live_snapshot();
        #[cfg(feature = "perf-live")]
        let mut snapshot = if super::perf::before() {
            self.engine.snapshot()
        } else {
            self.engine.live_snapshot()
        };
        if self.missing_catalog {
            add_missing(&mut snapshot.issues);
            if let Some(session) = &mut snapshot.session {
                add_missing(&mut session.issues);
            }
            if let Some(run) = &mut snapshot.run {
                run.complete_capture = false;
                add_missing(&mut run.issues);
            }
            for fight in &mut snapshot.fights {
                fight.complete_capture = false;
                add_missing(&mut fight.issues);
            }
            if let Some(character) = &mut snapshot.character {
                add_missing(&mut character.issues);
            }
            for drop in &mut snapshot.drops {
                add_missing(&mut drop.issues);
            }
        }
        self.snapshot_revision = self
            .snapshot_revision
            .checked_add(1)
            .expect("snapshot revision exhausted");
        snapshot.revision = Some(Sequence(Timestamp(self.snapshot_revision)));
        snapshot.character = self.observe_character(snapshot.character.take());
        *self
            .views
            .snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = snapshot;
        self.snapshot_dirty = false;
    }

    pub fn finish(&mut self, at: Timestamp, reason: FightEnd) -> Result<(), String> {
        if !self.finalized {
            self.at = self.at.max(at);
            self.finalized = true;
            self.snapshot_dirty = true;
            self.gate.dirty = true;
            let events = self.engine.finish(at, reason);
            self.events(events);
        }
        let result = self.flush_alerts();
        self.refresh_store();
        self.publish_snapshot();
        result
    }

    pub fn retry_and_close(&mut self) -> Result<(), String> {
        if let Some(writer) = &self.writer {
            match writer.retry() {
                Ok(()) => {}
                Err(maxed_store::StoreError::WorkerFailed { .. }) => self.reclaim_failed_writer(),
                Err(error) => return Err(error.to_string()),
            }
        }
        if self.writer.is_none() {
            // Resume the durable head with the exact original stream. Startup
            // recovery can close interrupted rows, then these retained terminal
            // revisions restore their recorded evidence before retry acknowledges.
            self.store_inputs.set(None);
            self.writer = Some(Arc::new(
                start_writer(self.path.clone(), self.writer_config.clone())
                    .map_err(|e| e.to_string())?,
            ));
        }
        self.drain()?;
        self.close()
    }

    fn reclaim_failed_writer(&mut self) {
        self.recovery_account = None;
        if let Some(mut recovery) = self.recovery.take() {
            recovery.stop();
        }
        if let Some(writer) = self.writer.take() {
            if let Err(maxed_store::ContextualShutdownError { pending, .. }) =
                Arc::try_unwrap(writer)
                    .unwrap_or_else(|_| panic!("recovery writer still owned"))
                    .shutdown_with_alert_context()
            {
                self.retain_accepted(pending);
            }
        }
        self.refresh_store();
    }

    fn retain_accepted(&mut self, accepted: Vec<maxed_store::ContextualEvent>) {
        // Accepted-but-uncommitted events precede every rejected envelope.
        let mut ordered: VecDeque<_> = accepted.into();
        ordered.append(&mut self.pending);
        self.pending = ordered;
    }

    #[cfg(test)]
    pub(super) fn fail_shutdown_for_test(&mut self) {
        self.reclaim_failed_writer();
    }

    pub fn close(&mut self) -> Result<(), String> {
        self.recovery_account = None;
        if let Some(mut recovery) = self.recovery.take() {
            recovery.stop();
        }
        self.maintenance_work.stop();
        if let Some(writer) = &self.writer {
            writer.set_maintenance_blocked(true);
        }
        if self.live {
            self.views.maintenance.attach(false);
        }
        self.flush_alerts()?;
        self.publish_snapshot();
        let writer = self.writer.as_ref().ok_or("Summary writer is closed")?;
        // Keep the writer alive on failure: retry must not trigger interrupted-run
        // recovery over this stream's still-owned terminal events.
        writer.flush().map_err(|e| e.to_string())?;
        let before = writer.status();
        let writer = self.writer.take().expect("writer checked above");
        let mut status = before;
        match Arc::try_unwrap(writer)
            .unwrap_or_else(|_| panic!("recovery writer still owned"))
            .shutdown_with_alert_context()
        {
            Ok(receipt) => {
                status.state = StoreState::Closed;
                status.queued = 0;
                status.durable = Some(receipt.sequence);
                status.message = "All summaries saved".into();
            }
            Err(maxed_store::ContextualShutdownError { pending, .. }) => {
                self.retain_accepted(pending);
                status.state = StoreState::Failed;
                status.queued = self.pending.len() as u32;
                status.message = "Summary worker failed; retained changes are not saved".into();
            }
        }
        let failed = status.state == StoreState::Failed;
        if let Some(notice) = self.catalog_notice {
            status.message = format!("{notice} {}", status.message);
        }
        // Completed and recovery adapters share this view; a direct write must
        // not leave a cached refresh claiming that it is still current.
        self.store_inputs.set(None);
        *self.views.store.lock().unwrap_or_else(|p| p.into_inner()) = status;
        if failed {
            Err("Summary shutdown failed; changes are not saved".into())
        } else {
            Ok(())
        }
    }

    fn cleanup_playback(&self) -> Result<(), String> {
        if self.ephemeral {
            if let Some(home) = &self.home {
                home.invalidate_and_wait()?;
            }
            remove_playback_database(&self.path)?;
        }
        Ok(())
    }

    pub fn discard(&mut self) {
        self.maintenance_work.stop();
        if self.live {
            self.views.maintenance.attach(false);
        }
        self.reclaim_failed_writer();
        self.pending.clear();
        let _ = self.cleanup_playback();
        // Completed and recovery adapters share the view, including discard writes.
        self.store_inputs.set(None);
        let mut status = self.views.store.lock().unwrap_or_else(|p| p.into_inner());
        status.state = StoreState::Closed;
        status.queued = 0;
        status.message = "Exited without saving pending changes".into();
    }
}

pub(super) fn remove_playback_database(path: &std::path::Path) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match std::fs::remove_file(PathBuf::from(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err("Temporary playback database is in use or could not be removed".into())
            }
        }
    }
    Ok(())
}

fn add_missing(issues: &mut Vec<CoverageIssue>) {
    if !issues.contains(&CoverageIssue::MissingCatalog) {
        issues.push(CoverageIssue::MissingCatalog);
    }
}

pub(super) fn unavailable_catalog() -> Arc<Catalog> {
    Arc::new(Catalog {
        format_version: 1,
        client_fingerprint: "unavailable".into(),
        items: vec![],
        objects: vec![],
        classes: vec![],
        enchants: vec![],
        dungeons: vec![],
        bags: vec![],
        sprites: vec![],
        tiles: vec![],
        crucible_bonuses: vec![],
    })
}

#[cfg(test)]
mod session_coverage_tests {
    use super::*;

    #[test]
    fn direct_reads_refresh_silent_metadata_without_consuming_the_publication_gate() {
        let dir = tempfile::tempdir().unwrap();
        let (sent, snapshots) = crossbeam_channel::unbounded();
        let mut adapter = Adapter::open(
            dir.path().join("direct-reads"),
            unavailable_catalog(),
            EngineConfig::default(),
            WriterConfig {
                stream_id: "engine".into(),
                ..Default::default()
            },
            true,
            Arc::new(Views::default()),
            Bridge {
                snapshot: Arc::new(move |snapshot| sent.send(snapshot).unwrap()),
                ..Default::default()
            },
        )
        .unwrap();
        snapshots.try_recv().unwrap(); // Initial baseline.
        adapter.live_traffic(Timestamp(1)).unwrap();
        let first = adapter.read_snapshot();
        assert_eq!(first.session.as_ref().unwrap().last_traffic, Timestamp(1));
        assert!(first.issues.contains(&CoverageIssue::MissingCatalog));
        assert_eq!(adapter.snapshot_builds, 2);

        // SessionUpdated is throttled to 30 seconds; this changes no watermark.
        adapter.live_traffic(Timestamp(2)).unwrap();
        // The lightweight query must still refresh pending owner metadata.
        adapter.read_character_identity();
        assert_eq!(adapter.snapshot_builds, 3);
        let latest = adapter.read_snapshot();
        assert_eq!(latest.sequence, first.sequence);
        assert!(latest.revision.unwrap().0 > first.revision.unwrap().0);
        assert_eq!(latest.session.as_ref().unwrap().last_traffic, Timestamp(2));
        assert_eq!(adapter.snapshot_builds, 3);
        for _ in 0..10 {
            adapter.read_character_identity();
            assert_eq!(adapter.read_snapshot(), latest);
        }
        assert_eq!(
            adapter.snapshot_builds, 3,
            "clean reads reuse the projection"
        );
        for millis in [250, 500, 750, 999] {
            adapter.publish_due(std::time::Duration::from_millis(millis));
        }
        assert!(
            snapshots.try_recv().is_err(),
            "reads must never push a snapshot"
        );
        adapter.publish_due(std::time::Duration::from_secs(1));
        assert_eq!(snapshots.try_recv().unwrap(), latest);
        assert_eq!(
            adapter.snapshot_builds, 3,
            "publication reuses the fresh cache"
        );

        // A silent transaction after a clean publication must dirty the gate.
        assert!(!adapter.gate.dirty);
        adapter.live_traffic(Timestamp(3)).unwrap();
        assert!(adapter.gate.dirty);
        adapter.publish_due(std::time::Duration::from_millis(1999));
        assert!(snapshots.try_recv().is_err());
        adapter.publish_due(std::time::Duration::from_secs(2));
        let silent = snapshots.try_recv().unwrap();
        assert_eq!(silent.sequence, latest.sequence);
        assert!(silent.revision.unwrap().0 > latest.revision.unwrap().0);
        assert_eq!(silent.session.as_ref().unwrap().last_traffic, Timestamp(3));

        adapter
            .capture(CaptureStatus {
                state: CaptureState::Live,
                ..Default::default()
            })
            .unwrap();
        let latest = adapter.read_snapshot();
        adapter.publish_due(std::time::Duration::from_millis(2250));
        assert!(
            snapshots.try_recv().is_err(),
            "idle publication stays at 1 Hz"
        );
        adapter.publish_due(std::time::Duration::from_secs(3));
        assert_eq!(snapshots.try_recv().unwrap(), latest);
        adapter
            .finish(Timestamp(3), FightEnd::CaptureStopped)
            .unwrap();
        adapter.close().unwrap();
    }

    #[test]
    fn missing_catalog_session_only_capture_persists_coverage_after_repair() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-only");
        let views = Arc::new(Views::default());
        let mut adapter = Adapter::open(
            path.clone(),
            unavailable_catalog(),
            EngineConfig::default(),
            WriterConfig {
                stream_id: "engine".into(),
                ..Default::default()
            },
            true,
            views.clone(),
            Bridge::default(),
        )
        .unwrap();
        adapter.traffic(Timestamp(1_000_000)).unwrap();
        adapter.gate.dirty = true;
        adapter.publish_snapshot();
        let session = views.snapshot.lock().unwrap().session.clone().unwrap();
        assert_eq!(session.issues, vec![CoverageIssue::MissingCatalog]);
        adapter.missing_catalog = false;
        adapter.traffic(Timestamp(32_000_000)).unwrap();
        adapter.writer_flush_for_test();
        adapter.close().unwrap();
        let home = maxed_store::open_reader(&path)
            .unwrap()
            .home_summary(
                HomeScope::Session { id: session.id },
                maxed_store::open_reader(&path)
                    .unwrap()
                    .current_account()
                    .unwrap()
                    .as_deref(),
            )
            .unwrap();
        assert_eq!((home.runs, home.clears), (0, 0));
        assert_eq!(home.issues, vec![CoverageIssue::MissingCatalog]);
    }
}

#[cfg(test)]
#[path = "departure_tests.rs"]
mod departure_tests;

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod snapshot_tests;

#[cfg(test)]
#[path = "hidden_snapshot_tests.rs"]
mod hidden_snapshot_tests;

#[cfg(test)]
#[path = "account_adapter_tests.rs"]
mod account_adapter_tests;

#[cfg(test)]
#[path = "join_publication_tests.rs"]
mod publication_tests;

#[cfg(test)]
#[path = "crucible_tests.rs"]
mod crucible_tests;

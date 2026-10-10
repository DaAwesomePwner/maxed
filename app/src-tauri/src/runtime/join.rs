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
    pub event: Arc<dyn Fn(EventEnvelope) + Send + Sync>,
    pub batch: Option<Arc<dyn Fn(Vec<EventEnvelope>) + Send + Sync>>,
    pub snapshot: Arc<dyn Fn(LiveSnapshot) + Send + Sync>,
}
impl Default for Bridge {
    fn default() -> Self {
        Self {
            event: Arc::new(|_| {}),
            batch: None,
            snapshot: Arc::new(|_| {}),
        }
    }
}

pub(super) struct Views {
    pub delivery: Mutex<super::alerts::delivery::Delivery>,
    pub alert_wake: Arc<super::alerts::delivery::DeliveryWake>,
    pub alert_policy: Arc<Mutex<AlertPreferences>>,
    pub chat_policy: Arc<Mutex<ChatSettings>>,
    pub chat_clear: Mutex<Option<crossbeam_channel::Sender<Result<(), String>>>>,
    pub snapshot: Mutex<LiveSnapshot>,
    pub store: Mutex<StoreStatus>,
}
impl Default for Views {
    fn default() -> Self {
        let alert_wake = Arc::new(super::alerts::delivery::DeliveryWake::default());
        Self {
            delivery: Mutex::new(super::alerts::delivery::Delivery::with_wake(
                alert_wake.clone(),
            )),
            alert_wake,
            alert_policy: Arc::new(Mutex::new(AlertPreferences::default())),
            chat_policy: Arc::new(Mutex::new(ChatSettings::default())),
            chat_clear: Mutex::new(None),
            snapshot: Mutex::new(LiveSnapshot {
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

pub(super) struct Adapter {
    pub engine: Engine,
    writer: Option<StoreWriter>,
    path: PathBuf,
    writer_config: WriterConfig,
    pending: VecDeque<maxed_store::ContextualEvent>,
    bridge: Bridge,
    views: Arc<Views>,
    missing_catalog: bool,
    finalized: bool,
    gate: crate::bridge::SnapshotGate,
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
        let engine =
            Engine::new(config, catalog).map_err(|_| "Catalog could not initialize the engine")?;
        let writer =
            start_writer(path.clone(), writer_config.clone()).map_err(|e| e.to_string())?;
        // A new allocator must have a new stream ID; resuming an old engine is unsupported.
        if writer.status().durable.is_some() {
            let _ = writer.shutdown();
            return Err("A new capture requires a new stream namespace".into());
        }
        let mut adapter = Self {
            engine,
            writer: Some(writer),
            path,
            writer_config,
            pending: VecDeque::new(),
            bridge,
            alert_policy: views.alert_policy.clone(),
            chat_policy: views.chat_policy.clone(),
            views,
            missing_catalog,
            finalized: false,
            gate: crate::bridge::SnapshotGate {
                dirty: true,
                ..Default::default()
            },
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
        adapter.publish_snapshot();
        adapter.refresh_store();
        Ok(adapter)
    }

    pub fn refresh_store(&self) {
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
            if let Some(notice) = self.catalog_notice {
                status.message = format!("{notice} {}", status.message);
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
        let mut presentation = Vec::with_capacity(events.len());
        for mut event in events {
            self.partial(&mut event);
            if let EngineEvent::CaptureStateChanged(status) = &event {
                self.capture_status = status.clone();
            }
            self.sequence.0 .0 += 1;
            let envelope = EventEnvelope {
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
                preferences: self
                    .alert_policy
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone(),
                bag_window: match &envelope.event {
                    EngineEvent::BagDropped(drop) | EngineEvent::DropUpdated(drop) => {
                        self.alert_calendar.seven_day_window(drop.at).ok()
                    }
                    _ => None,
                },
            };
            self.pending.push_back(maxed_store::ContextualEvent {
                event: envelope.clone(),
                context,
            });
            // The publisher compacts the presentation copy before enqueueing.
            presentation.push(envelope);
        }
        // The original full events and alert contexts are retained first. Only
        // presentation delivery changes; writer ordering/backpressure is intact.
        if let Some(batch) = &self.bridge.batch {
            batch(presentation);
        } else {
            for envelope in presentation {
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
        self.service_chat_maintenance();
        Ok(())
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
        self.events(events);
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
        if self.gate.take_due(now) {
            self.gate.dirty = true;
            self.publish_snapshot();
        }
    }

    pub fn publish_snapshot(&mut self) {
        if !self.gate.dirty {
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
        *self
            .views
            .snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = snapshot.clone();
        (self.bridge.snapshot)(snapshot);
        self.gate.dirty = false;
    }

    pub fn finish(&mut self, at: Timestamp, reason: FightEnd) -> Result<(), String> {
        if !self.finalized {
            self.at = self.at.max(at);
            self.finalized = true;
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
            self.writer = Some(
                start_writer(self.path.clone(), self.writer_config.clone())
                    .map_err(|e| e.to_string())?,
            );
        }
        self.drain()?;
        self.close()
    }

    fn reclaim_failed_writer(&mut self) {
        if let Some(writer) = self.writer.take() {
            if let Err(maxed_store::ContextualShutdownError { pending, .. }) =
                writer.shutdown_with_alert_context()
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
        self.flush_alerts()?;
        self.publish_snapshot();
        let writer = self.writer.as_ref().ok_or("Summary writer is closed")?;
        // Keep the writer alive on failure: retry must not trigger interrupted-run
        // recovery over this stream's still-owned terminal events.
        writer.flush().map_err(|e| e.to_string())?;
        let before = writer.status();
        let writer = self.writer.take().expect("writer checked above");
        let mut status = before;
        match writer.shutdown_with_alert_context() {
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
        self.reclaim_failed_writer();
        self.pending.clear();
        let _ = self.cleanup_playback();
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
            .home_summary(HomeScope::Session { id: session.id })
            .unwrap();
        assert_eq!((home.runs, home.clears), (0, 0));
        assert_eq!(home.issues, vec![CoverageIssue::MissingCatalog]);
    }
}

#[cfg(test)]
#[path = "departure_tests.rs"]
mod departure_tests;

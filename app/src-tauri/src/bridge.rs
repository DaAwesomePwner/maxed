//! Presentation scheduling and ordered delivery, independent of durable storage.
pub mod channels;
pub mod payload;
use crossbeam_channel::{bounded, Sender};
use maxed_types::{CaptureState, CaptureStatus, EventEnvelope, LiveSnapshot};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

pub const DELIVERY_FAILED: &str = "Live updates interrupted; reconnect or restart Maxed";
const ACTIVE_FIGHT_RECENT_HIT: Duration = Duration::from_secs(5);

/// Even epochs deliver snapshots/notices; odd epochs suspend them. Old queued
/// snapshots cannot cross a hide/restore boundary. Ordered events always flow.
pub struct PresentationVisibility {
    epoch: AtomicU64,
    pub document_visible: AtomicBool,
}
impl Default for PresentationVisibility {
    fn default() -> Self {
        Self {
            epoch: AtomicU64::new(0),
            document_visible: AtomicBool::new(true),
        }
    }
}
impl PresentationVisibility {
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub fn accepts(&self, epoch: u64) -> bool {
        epoch.is_multiple_of(2) && self.epoch() == epoch
    }
    pub fn set_visible(&self, visible: bool) -> bool {
        self.epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                ((epoch.is_multiple_of(2)) != visible).then(|| epoch.wrapping_add(1))
            })
            .is_ok()
    }
}

#[derive(Default)]
pub struct SnapshotGate {
    pub last: Duration,
    pub dirty: bool,
}
impl SnapshotGate {
    pub fn take_due_with_hit_age(&mut self, now: Duration, last_hit_age: Option<Duration>) -> bool {
        let recent = last_hit_age.is_some_and(|age| age < ACTIVE_FIGHT_RECENT_HIT);
        self.take_due(now, recent)
    }

    pub fn take_due(&mut self, now: Duration, recent: bool) -> bool {
        let interval = Duration::from_millis(if recent { 250 } else { 1000 });
        if !self.dirty || now.saturating_sub(self.last) < interval {
            return false;
        }
        self.last = now;
        self.dirty = false;
        true
    }
}

#[derive(Clone, Debug, serde::Serialize)]
/// Presentation invalidation only; never an engine event or a store write.
pub struct PartyChanged {
    pub scope_id: Option<String>,
}

pub enum Publication {
    Capture(CaptureStatus),
    Event(Box<EventEnvelope>),
    Events(Vec<EventEnvelope>),
    Snapshot(Box<LiveSnapshot>),
    PartyChanged(PartyChanged),
}

// Only compact, bounded event vectors can enter the worker queue.
enum QueuedPublication {
    Capture(CaptureStatus),
    Events(Vec<EventEnvelope>),
    Snapshot(Box<LiveSnapshot>),
    PartyChanged(PartyChanged),
}

fn queued_event(event: EventEnvelope) -> EventEnvelope {
    #[cfg(feature = "perf-live")]
    if crate::runtime::perf::before() {
        return event;
    }
    payload::presentation_event(event)
}

enum Delivery {
    Publish(QueuedPublication, u64),
    Flush(Sender<Result<(), String>>),
}

/// One consumer preserves enqueue order across capture, domain and snapshot IPC.
/// Event transactions retain every member. Party notices coalesce independently
/// with a leading delivery and a trailing delivery after one monotonic second.
/// Scope changes deliver immediately and reset the throttle window.
/// Flush and disconnect deliver pending notices immediately.
/// A full queue backpressures its producer instead of growing without bound.
/// Emission happens on this worker, never inside the caller's lifecycle locks.
#[derive(Clone)]
pub struct Publisher {
    send: Sender<Delivery>,
    failed: Arc<dyn Fn() + Send + Sync>,
    visibility: Arc<PresentationVisibility>,
}
impl Publisher {
    pub(crate) fn visibility(&self) -> Arc<PresentationVisibility> {
        self.visibility.clone()
    }

    /// Individual-envelope sink receiving compact presentation events. Native IPC
    /// uses `new_batched` to deliver bounded chunks of each engine transaction.
    pub fn new(
        mut emit: impl FnMut(Publication) -> Result<(), ()> + Send + 'static,
        failed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        Self::new_batched(
            move |publication| match publication {
                Publication::Events(events) => {
                    let mut result = Ok(());
                    for event in events {
                        if emit(Publication::Event(Box::new(event))).is_err() {
                            result = Err(());
                        }
                    }
                    result
                }
                publication => emit(publication),
            },
            failed,
        )
    }

    /// Native byte-channel consumer retains the adapter's transaction boundary.
    pub fn new_batched(
        mut emit: impl FnMut(Publication) -> Result<(), ()> + Send + 'static,
        failed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        Self::start(
            move |publication| {
                emit(match publication {
                    QueuedPublication::Capture(value) => Publication::Capture(value),
                    QueuedPublication::Events(events) => Publication::Events(events),
                    QueuedPublication::Snapshot(value) => Publication::Snapshot(value),
                    QueuedPublication::PartyChanged(value) => Publication::PartyChanged(value),
                })
            },
            failed,
            Arc::default(),
        )
    }

    fn start(
        mut emit: impl FnMut(QueuedPublication) -> Result<(), ()> + Send + 'static,
        failed: Arc<dyn Fn() + Send + Sync>,
        visibility: Arc<PresentationVisibility>,
    ) -> Result<Self, String> {
        // Eight transactions, each capped at EVENT_BATCH_LIMIT, backpressure
        // capture instead of retaining hundreds of mass-close vectors.
        let (send, receive) = bounded(8);
        let failure = failed.clone();
        let worker_visibility = visibility.clone();
        std::thread::Builder::new()
            .name("maxed-bridge".into())
            .spawn(move || {
                let interrupted = std::cell::Cell::new(false);
                let mut last_party: Option<Instant> = None;
                let mut last_party_scope = None;
                let mut pending_party = None;
                let mut party_epoch = worker_visibility.epoch();
                let mut deliver = |publication| {
                    if emit(publication).is_err() {
                        interrupted.set(true);
                        failure();
                    }
                };
                loop {
                    if !worker_visibility.accepts(party_epoch) {
                        pending_party = None;
                        last_party = None;
                    }
                    let remaining = last_party.map_or(Duration::ZERO, |last| {
                        Duration::from_secs(1).saturating_sub(last.elapsed())
                    });
                    if pending_party.is_some() && remaining.is_zero() {
                        deliver(QueuedPublication::PartyChanged(
                            pending_party.take().unwrap(),
                        ));
                        last_party = Some(Instant::now());
                    }
                    let delivery = if pending_party.is_some() {
                        match receive.recv_timeout(remaining) {
                            Ok(delivery) => delivery,
                            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                                if worker_visibility.accepts(party_epoch) {
                                    deliver(QueuedPublication::PartyChanged(
                                        pending_party.take().unwrap(),
                                    ));
                                }
                                break;
                            }
                        }
                    } else {
                        let Ok(delivery) = receive.recv() else { break };
                        delivery
                    };
                    match delivery {
                        Delivery::Publish(publication, epoch)
                            if matches!(
                                publication,
                                QueuedPublication::Snapshot(_) | QueuedPublication::PartyChanged(_)
                            ) && !worker_visibility.accepts(epoch) =>
                        {
                            pending_party = None;
                        }
                        Delivery::Publish(QueuedPublication::PartyChanged(notice), epoch) => {
                            party_epoch = epoch;
                            if last_party_scope != notice.scope_id
                                || last_party
                                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(1))
                            {
                                // A new scope supersedes the old scope's trailing notice.
                                pending_party = None;
                                last_party_scope.clone_from(&notice.scope_id);
                                deliver(QueuedPublication::PartyChanged(notice));
                                last_party = Some(Instant::now());
                            } else {
                                pending_party = Some(notice);
                            }
                        }
                        Delivery::Publish(publication, _) => {
                            deliver(publication);
                        }
                        Delivery::Flush(done) => {
                            if let Some(notice) = pending_party
                                .take()
                                .filter(|_| worker_visibility.accepts(party_epoch))
                            {
                                deliver(QueuedPublication::PartyChanged(notice));
                                last_party = Some(Instant::now());
                            }
                            let _ = done.send(if interrupted.get() {
                                Err(DELIVERY_FAILED.into())
                            } else {
                                Ok(())
                            });
                        }
                    }
                }
            })
            .map_err(|_| DELIVERY_FAILED.to_string())?;
        Ok(Self {
            send,
            failed,
            visibility,
        })
    }

    pub fn publish(&self, publication: Publication) {
        let epoch = self.visibility.epoch();
        if matches!(
            publication,
            Publication::Snapshot(_) | Publication::PartyChanged(_)
        ) && !self.visibility.accepts(epoch)
        {
            return;
        }
        let events = match publication {
            Publication::Event(event) => vec![*event],
            Publication::Events(events) => events,
            Publication::Capture(value) => {
                self.enqueue(QueuedPublication::Capture(value), epoch);
                return;
            }
            Publication::Snapshot(value) => {
                self.enqueue(QueuedPublication::Snapshot(value), epoch);
                return;
            }
            Publication::PartyChanged(value) => {
                self.enqueue(QueuedPublication::PartyChanged(value), epoch);
                return;
            }
        };
        // Compact on the producer, never after an uncompacted event has queued.
        let mut events = events.into_iter().map(queued_event).peekable();
        while events.peek().is_some() {
            let batch = events.by_ref().take(payload::EVENT_BATCH_LIMIT).collect();
            self.enqueue(QueuedPublication::Events(batch), epoch);
        }
    }

    fn enqueue(&self, publication: QueuedPublication, epoch: u64) {
        if self
            .send
            .send(Delivery::Publish(publication, epoch))
            .is_err()
        {
            (self.failed)();
        }
    }

    /// Call after releasing lifecycle locks, before command completion/app exit.
    pub fn flush(&self) -> Result<(), String> {
        let (done, wait) = bounded(1);
        self.send
            .send(Delivery::Flush(done))
            .map_err(|_| DELIVERY_FAILED)?;
        wait.recv().map_err(|_| DELIVERY_FAILED.to_string())?
    }

    pub(crate) fn native(
        app: tauri::AppHandle,
        failed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let warned = std::sync::atomic::AtomicBool::new(false);
        let failure = failed.clone();
        let mut encoder = payload::SnapshotEncoder::default();
        let visibility = app.state::<Arc<PresentationVisibility>>().inner().clone();
        let encoder_visibility = visibility.clone();
        let mut encoder_epoch = visibility.epoch();
        Self::start(
            move |publication| {
                let epoch = encoder_visibility.epoch();
                if epoch != encoder_epoch {
                    encoder = payload::SnapshotEncoder::default();
                    encoder_epoch = epoch;
                }
                let result = match publication {
                    QueuedPublication::PartyChanged(value) => app
                        .state::<channels::LivePayloads>()
                        .send("party-changed", &value),
                    QueuedPublication::Capture(value) => {
                        app.emit("capture-status", value).map_err(|_| ())
                    }
                    QueuedPublication::Events(events) => {
                        #[cfg(feature = "perf-live")]
                        if crate::runtime::perf::before() {
                            let mut result = Ok(());
                            for event in events {
                                if app.emit("engine-event", event).is_err() {
                                    result = Err(());
                                }
                            }
                            return result;
                        }
                        let payloads = app.state::<channels::LivePayloads>();
                        let mut result = Ok(());
                        for batch in payload::event_batches(events) {
                            if payloads.send("engine-event", &batch).is_err() {
                                result = Err(());
                            }
                        }
                        result
                    }
                    QueuedPublication::Snapshot(value) => {
                        #[cfg(feature = "perf-live")]
                        if crate::runtime::perf::before() {
                            return app.emit("live-snapshot", value).map_err(|_| ());
                        }
                        app.state::<channels::LivePayloads>()
                            .send("live-snapshot", &encoder.encode(*value))
                    }
                };
                if result.is_err() {
                    failure();
                    if !warned.swap(true, std::sync::atomic::Ordering::AcqRel) {
                        let _ = app.emit(
                            "capture-status",
                            CaptureStatus {
                                state: CaptureState::Failed,
                                message: DELIVERY_FAILED.into(),
                                ..Default::default()
                            },
                        );
                        // Still actionable when IPC itself is unavailable. These
                        // UI calls stay on this worker, including failure paths.
                        if let Some(window) = app.get_webview_window("main") {
                            let _ =
                                window.set_title("Maxed - Live updates interrupted; restart Maxed");
                        }
                    }
                }
                result.map_err(|_| ())
            },
            failed,
            visibility,
        )
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn snapshot_gate_keeps_idle_and_recent_fight_cadences() {
        let mut gate = SnapshotGate {
            dirty: true,
            ..Default::default()
        };
        assert!(!gate.take_due_with_hit_age(Duration::from_millis(999), None));
        assert!(gate.take_due_with_hit_age(Duration::from_millis(1000), None));
        gate.dirty = true;
        assert!(!gate.take_due_with_hit_age(Duration::from_millis(1249), Some(Duration::ZERO)));
        assert!(gate.take_due_with_hit_age(Duration::from_millis(1250), Some(Duration::ZERO)));
        gate.dirty = true;
        assert!(!gate.take_due_with_hit_age(Duration::from_millis(2249), None));
        assert!(gate.take_due_with_hit_age(Duration::from_millis(2250), None));
        assert!(!gate.take_due_with_hit_age(Duration::from_secs(10), Some(Duration::ZERO)));
    }

    #[test]
    fn snapshot_gate_slows_at_five_seconds_and_resumes_on_the_next_tick() {
        let mut gate = SnapshotGate {
            dirty: true,
            ..Default::default()
        };
        assert!(gate.take_due_with_hit_age(
            Duration::from_millis(4999),
            Some(Duration::from_millis(4999))
        ));
        for millis in [5249, 5749, 5998] {
            gate.dirty = true;
            assert!(!gate.take_due_with_hit_age(
                Duration::from_millis(millis),
                Some(Duration::from_millis(millis))
            ));
        }
        assert!(gate.take_due_with_hit_age(
            Duration::from_millis(5999),
            Some(Duration::from_millis(5999))
        ));
        gate.dirty = true;
        assert!(gate.take_due_with_hit_age(
            Duration::from_millis(6249),
            Some(Duration::from_millis(249))
        ));

        for age in [Duration::from_secs(5), Duration::from_secs(20)] {
            let mut gate = SnapshotGate {
                dirty: true,
                ..Default::default()
            };
            assert!(!gate.take_due_with_hit_age(Duration::from_millis(999), Some(age)));
            assert!(gate.take_due_with_hit_age(Duration::from_secs(1), Some(age)));
        }
    }

    #[test]
    fn visibility_discards_snapshots_but_keeps_ordered_events_and_capture_status() {
        let visibility = Arc::new(PresentationVisibility::default());
        let (send, receive) = crossbeam_channel::unbounded();
        let (resume, wait) = crossbeam_channel::bounded(1);
        let mut first = true;
        let publisher = Publisher::start(
            move |publication| {
                send.send(match publication {
                    QueuedPublication::Capture(_) => "capture",
                    QueuedPublication::Snapshot(_) => "snapshot",
                    QueuedPublication::Events(_) => "events",
                    QueuedPublication::PartyChanged(_) => "party",
                })
                .unwrap();
                if first {
                    first = false;
                    wait.recv().unwrap();
                }
                Ok(())
            },
            Arc::new(|| panic!("delivery failed")),
            visibility.clone(),
        )
        .unwrap();
        let snapshot = || Publication::Snapshot(Box::new(maxed_types::demo::demo_snapshot()));
        publisher.publish(snapshot());
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            "snapshot"
        );
        publisher.publish(snapshot()); // queued before hide; must not leak on restore
        let event = |sequence| EventEnvelope {
            stream_id: "synthetic".into(),
            sequence: maxed_types::Sequence(maxed_types::Timestamp(sequence)),
            at: maxed_types::Timestamp(sequence),
            fight_chart_omitted: false,
            event: maxed_types::EngineEvent::CaptureStateChanged(CaptureStatus::default()),
        };
        publisher.publish(Publication::Event(Box::new(event(1))));
        visibility.set_visible(false);
        publisher.publish(snapshot());
        publisher.publish(Publication::Events(vec![event(2)]));
        publisher.publish(Publication::Capture(CaptureStatus::default()));
        visibility.set_visible(true);
        publisher.publish(snapshot());
        resume.send(()).unwrap();
        publisher.flush().unwrap();
        assert_eq!(
            receive.try_iter().collect::<Vec<_>>(),
            ["events", "events", "capture", "snapshot"]
        );
    }

    #[test]
    fn review_flush_and_disconnect_deliver_pending_party_promptly() {
        for disconnect in [false, true] {
            let (send, receive) = crossbeam_channel::unbounded();
            let publisher = Publisher::new(
                move |publication| {
                    if let Publication::PartyChanged(notice) = publication {
                        send.send(notice).unwrap();
                    }
                    Ok(())
                },
                Arc::new(|| panic!("delivery failed")),
            )
            .unwrap();
            let notice = |scope: &str| {
                Publication::PartyChanged(PartyChanged {
                    scope_id: Some(scope.into()),
                })
            };
            publisher.publish(notice("scope-a"));
            receive.recv_timeout(Duration::from_secs(5)).unwrap();
            publisher.publish(notice("scope-a"));
            assert!(receive.recv_timeout(Duration::from_millis(100)).is_err());
            let start = Instant::now();
            if disconnect {
                drop(publisher);
            } else {
                publisher.flush().unwrap();
            }
            let trailing = receive.recv_timeout(Duration::from_millis(300)).unwrap();
            assert_eq!(trailing.scope_id.as_deref(), Some("scope-a"));
            assert!(start.elapsed() < Duration::from_millis(300));
        }
    }

    #[test]
    fn party_notice_bursts_have_one_leading_and_one_trailing_delivery() {
        let (send, receive) = crossbeam_channel::unbounded();
        let publisher = Publisher::new(
            move |publication| {
                if let Publication::PartyChanged(notice) = publication {
                    send.send((std::time::Instant::now(), notice)).unwrap();
                }
                Ok(())
            },
            Arc::new(|| panic!("delivery failed")),
        )
        .unwrap();
        let notice = |scope: &str| {
            Publication::PartyChanged(PartyChanged {
                scope_id: Some(scope.into()),
            })
        };
        publisher.publish(notice("scope-a"));
        let (leading_at, leading) = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            serde_json::to_value(leading).unwrap(),
            serde_json::json!({"scope_id":"scope-a"})
        );
        publisher.publish(notice("scope-a"));
        publisher.publish(notice("scope-a"));
        assert!(receive.recv_timeout(Duration::from_millis(100)).is_err());
        let (trailing_at, trailing) = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(trailing_at.duration_since(leading_at) >= Duration::from_secs(1));
        assert_eq!(trailing.scope_id.as_deref(), Some("scope-a"));
        publisher.flush().unwrap();
        assert!(receive.try_recv().is_err());
    }

    #[test]
    fn scope_change_is_immediate_supersedes_pending_and_resets_throttle() {
        let (send, receive) = crossbeam_channel::unbounded();
        let publisher = Publisher::new(
            move |publication| {
                if let Publication::PartyChanged(notice) = publication {
                    send.send((Instant::now(), notice)).unwrap();
                }
                Ok(())
            },
            Arc::new(|| panic!("delivery failed")),
        )
        .unwrap();
        let notice = |scope: Option<&str>| {
            Publication::PartyChanged(PartyChanged {
                scope_id: scope.map(str::to_owned),
            })
        };
        let mut scope = Some("scope-a");
        publisher.publish(notice(scope));
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
        for next_scope in [Some("scope-b"), None, Some("scope-c")] {
            publisher.publish(notice(scope));
            assert!(receive.recv_timeout(Duration::from_millis(100)).is_err());
            publisher.publish(notice(next_scope));
            let (changed_at, changed) = receive.recv_timeout(Duration::from_millis(300)).unwrap();
            assert_eq!(changed.scope_id.as_deref(), next_scope);
            publisher.publish(notice(next_scope));
            let (trailing_at, trailing) = receive.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(trailing_at.duration_since(changed_at) >= Duration::from_secs(1));
            assert_eq!(trailing.scope_id.as_deref(), next_scope);
            publisher.flush().unwrap();
            assert!(receive.try_recv().is_err());
            scope = next_scope;
        }
    }

    #[test]
    fn producer_compacts_single_and_batch_before_enqueue() {
        // No worker exists: inspect the actual queue, not its eventual output.
        let (send, receive) = bounded(8);
        let publisher = Publisher {
            send,
            failed: Arc::new(|| panic!("delivery failed")),
            visibility: Arc::default(),
        };
        let mut fight = maxed_types::demo::demo_snapshot().fights.remove(0);
        fight.target_kind = Some(maxed_types::TargetKind::Ordinary);
        assert!(!fight.buckets.is_empty());
        let original = EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "queue".into(),
            sequence: maxed_types::Sequence(maxed_types::Timestamp(1)),
            at: maxed_types::Timestamp(1),
            event: maxed_types::EngineEvent::FightEnded(fight),
        };
        publisher.publish(Publication::Event(Box::new(original.clone())));
        publisher.publish(Publication::Events(vec![original.clone()]));
        for _ in 0..2 {
            let Delivery::Publish(QueuedPublication::Events(events), _) = receive.recv().unwrap()
            else {
                panic!("expected batch")
            };
            assert_eq!(events, vec![payload::presentation_event(original.clone())]);
            let maxed_types::EngineEvent::FightEnded(fight) = &events[0].event else {
                panic!("expected fight")
            };
            assert!(fight.buckets.is_empty());
            assert_eq!(fight.buckets.capacity(), 0);
            assert!(fight.player_series.is_none());
        }
    }
}

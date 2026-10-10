//! Presentation scheduling and ordered delivery, independent of durable storage.
pub mod channels;
pub mod payload;
use crossbeam_channel::{bounded, Sender};
use maxed_types::{CaptureState, CaptureStatus, EventEnvelope, LiveSnapshot};
use std::{sync::Arc, time::Duration};
use tauri::{Emitter, Manager};

pub const DELIVERY_FAILED: &str = "Live updates interrupted; reconnect or restart Maxed";

#[derive(Default)]
pub struct SnapshotGate {
    pub last: Duration,
    pub dirty: bool,
}
impl SnapshotGate {
    pub fn take_due(&mut self, now: Duration) -> bool {
        if !self.dirty || now.saturating_sub(self.last) < Duration::from_millis(250) {
            return false;
        }
        self.last = now;
        self.dirty = false;
        true
    }
}

pub enum Publication {
    Capture(CaptureStatus),
    Event(Box<EventEnvelope>),
    Events(Vec<EventEnvelope>),
    Snapshot(Box<LiveSnapshot>),
}

// Only compact, bounded event vectors can enter the worker queue.
enum QueuedPublication {
    Capture(CaptureStatus),
    Events(Vec<EventEnvelope>),
    Snapshot(Box<LiveSnapshot>),
}

fn queued_event(event: EventEnvelope) -> EventEnvelope {
    #[cfg(feature = "perf-live")]
    if crate::runtime::perf::before() {
        return event;
    }
    payload::presentation_event(event)
}

enum Delivery {
    Publish(QueuedPublication),
    Flush(Sender<Result<(), String>>),
}

/// One consumer preserves enqueue order across capture, domain and snapshot IPC.
/// Event transactions retain every member; only snapshot creation is gated.
/// A full queue backpressures its producer instead of growing without bound.
/// Emission happens on this worker, never inside the caller's lifecycle locks.
#[derive(Clone)]
pub struct Publisher {
    send: Sender<Delivery>,
    failed: Arc<dyn Fn() + Send + Sync>,
}
impl Publisher {
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
                })
            },
            failed,
        )
    }

    fn start(
        mut emit: impl FnMut(QueuedPublication) -> Result<(), ()> + Send + 'static,
        failed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        // Eight transactions, each capped at EVENT_BATCH_LIMIT, backpressure
        // capture instead of retaining hundreds of mass-close vectors.
        let (send, receive) = bounded(8);
        let failure = failed.clone();
        std::thread::Builder::new()
            .name("maxed-bridge".into())
            .spawn(move || {
                let mut interrupted = false;
                for delivery in receive {
                    match delivery {
                        Delivery::Publish(publication) => {
                            if emit(publication).is_err() {
                                // Fail visibly; a snapshot cannot recover missed celebrations.
                                interrupted = true;
                                failure();
                            }
                        }
                        Delivery::Flush(done) => {
                            let _ = done.send(if interrupted {
                                Err(DELIVERY_FAILED.into())
                            } else {
                                Ok(())
                            });
                        }
                    }
                }
            })
            .map_err(|_| DELIVERY_FAILED.to_string())?;
        Ok(Self { send, failed })
    }

    pub fn publish(&self, publication: Publication) {
        let events = match publication {
            Publication::Event(event) => vec![*event],
            Publication::Events(events) => events,
            Publication::Capture(value) => {
                self.enqueue(QueuedPublication::Capture(value));
                return;
            }
            Publication::Snapshot(value) => {
                self.enqueue(QueuedPublication::Snapshot(value));
                return;
            }
        };
        // Compact on the producer, never after an uncompacted event has queued.
        let mut events = events.into_iter().map(queued_event).peekable();
        while events.peek().is_some() {
            let batch = events.by_ref().take(payload::EVENT_BATCH_LIMIT).collect();
            self.enqueue(QueuedPublication::Events(batch));
        }
    }

    fn enqueue(&self, publication: QueuedPublication) {
        if self.send.send(Delivery::Publish(publication)).is_err() {
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
        Self::start(
            move |publication| {
                let result = match publication {
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
        )
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn producer_compacts_single_and_batch_before_enqueue() {
        // No worker exists: inspect the actual queue, not its eventual output.
        let (send, receive) = bounded(8);
        let publisher = Publisher {
            send,
            failed: Arc::new(|| panic!("delivery failed")),
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
            let Delivery::Publish(QueuedPublication::Events(events)) = receive.recv().unwrap()
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

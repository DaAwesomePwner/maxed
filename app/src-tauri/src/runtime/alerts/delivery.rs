//! Lock order: alert-settings/lifecycle -> delivery -> policy/snapshot/status.
//! The delivery worker never takes capture/session/history/settings locks. Only
//! this worker calls play/notify; retirement synchronously stops the active voice.
use super::*;
use maxed_store::CommittedAlert;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

const COOLDOWN: Duration = Duration::from_millis(300);
pub trait DeliverySink: Send {
    fn play(&mut self, kind: AlertKind, volume: u8) -> Result<(), String>;
    /// Cheap handle and cached permission only; no native calls under the guard.
    fn notifier(&self) -> Arc<dyn DeliveryNotifier>;
    fn permission(&self) -> NotificationPermission;
    fn sound_available(&self) -> bool;
    /// Called only with the sink detached and delivery unlocked; may query the OS.
    fn device_stale(&self) -> bool {
        false
    }
    fn playing(&self) -> bool;
    fn stop(&mut self);
    fn recover(&mut self) -> Result<(), String>;
}
pub trait DeliveryNotifier: Send + Sync {
    fn refresh_permission(&self) -> NotificationPermission;
    fn focused(&self) -> Result<bool, String>;
    /// Implementations must always request silent notifications.
    fn notify_silent(&self, alert: &AlertView) -> Result<(), String>;
}

struct NotificationTask {
    notifier: Arc<dyn DeliveryNotifier>,
    pending: Pending,
    generation: u64,
    epoch: Arc<AtomicU64>,
    at: Timestamp,
    started: Instant,
}
impl NotificationTask {
    fn current(&self) -> bool {
        self.epoch.load(Ordering::Acquire) == self.generation
    }
    fn run(&self, runtime: &CaptureRuntime) -> Result<(), String> {
        if !self.current() || self.notifier.refresh_permission() != NotificationPermission::Granted
        {
            return Ok(());
        }
        let focused = self
            .notifier
            .focused()
            .map_err(|_| "Window focus unavailable; notification suppressed.".to_string())?;
        let policy = runtime
            .shared
            .views
            .alert_policy
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let at = Timestamp(
            self.at
                .0
                .saturating_add(self.started.elapsed().as_micros().min(u64::MAX as u128) as u64),
        );
        if self.current()
            && (!focused || self.pending.sample)
            && policy.windows_notifications
            && runtime.external_delivery_allowed(&self.pending.alert.source_stream_id)
            && visible(self.pending.effective(&policy, at))
        {
            self.notifier.notify_silent(&self.pending.alert.view)?;
        }
        Ok(())
    }
}

pub(super) struct Pending {
    pub alert: CommittedAlert,
    pub due: Instant,
    pub sample: bool,
}
impl Pending {
    fn effective(&self, policy: &AlertPreferences, at: Timestamp) -> AlertLoudness {
        // Explicit Test previews the cue independently of dungeon selection.
        if self.sample {
            effective_loudness(&self.alert, policy, at)
        } else {
            effective(&self.alert, policy, at)
        }
    }
}
#[derive(Default)]
pub(crate) struct Delivery {
    pub stream: Option<String>,
    pub wake: Arc<DeliveryWake>,
    // Source envelope time and receipt Instant, never a system-wall timestamp.
    event_clock: Option<(String, Timestamp, Instant)>,
    pub(super) waiting: Option<Pending>,
    pub(super) sample: Option<Pending>,
    pub sink: Option<Box<dyn DeliverySink>>,
    recovery_status: Option<AlertDeliveryStatus>,
    pub message: Option<String>,
    last_play: Option<Instant>,
    notification_epoch: Arc<AtomicU64>,
    pub shutdown: bool,
}
/// A coalescing wake permit; taking the permit and waiting use the same mutex,
/// so enqueue between deadline calculation and wait cannot be lost.
#[derive(Default)]
pub(crate) struct DeliveryWake {
    pending: Mutex<bool>,
    ready: Condvar,
    #[cfg(test)]
    pub cycles: std::sync::atomic::AtomicUsize,
}
impl DeliveryWake {
    pub fn signal(&self) {
        *self.pending.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.ready.notify_one();
    }
    fn wait(&self, timeout: Option<Duration>) {
        let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(timeout) = timeout {
            pending = self
                .ready
                .wait_timeout_while(pending, timeout, |ready| !*ready)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        } else {
            pending = self
                .ready
                .wait_while(pending, |ready| !*ready)
                .unwrap_or_else(|p| p.into_inner());
        }
        *pending = false;
    }
}

pub fn effective(
    alert: &CommittedAlert,
    policy: &AlertPreferences,
    at: Timestamp,
) -> AlertLoudness {
    if !policy.allows_alert(&alert.view) {
        return AlertLoudness::Off;
    }
    effective_loudness(alert, policy, at)
}
fn effective_loudness(
    alert: &CommittedAlert,
    policy: &AlertPreferences,
    at: Timestamp,
) -> AlertLoudness {
    use AlertLoudness::*;
    let rank = |level| match level {
        Off => 0,
        QuietLogOnly => 1,
        Toast => 2,
        Banner => 3,
    };
    let current = policy.loudness(&alert.view.kind);
    let level = if rank(current) < rank(alert.loudness) {
        current
    } else {
        alert.loudness
    };
    if (at.0 == 0
        || at < alert.committed_at
        || at.0.saturating_sub(alert.committed_at.0) > 30_000_000)
        && rank(level) > 1
    {
        QuietLogOnly
    } else {
        level
    }
}
fn visible(level: AlertLoudness) -> bool {
    matches!(level, AlertLoudness::Toast | AlertLoudness::Banner)
}
impl Drop for Delivery {
    fn drop(&mut self) {
        self.wake.signal();
    }
}
impl Delivery {
    pub fn with_wake(wake: Arc<DeliveryWake>) -> Self {
        let mut delivery = Self::default();
        delivery.wake = wake;
        delivery
    }
    pub fn observe_clock(&mut self, stream: &str, at: Timestamp, arrived: Instant) {
        if at.0 != 0
            && self
                .event_clock
                .as_ref()
                .is_none_or(|(id, previous, _)| id != stream || at > *previous)
        {
            self.event_clock = Some((stream.into(), at, arrived));
        }
    }
    pub fn event_now(&self, stream: &str, clock: Instant) -> Option<Timestamp> {
        let (id, at, arrived) = self.event_clock.as_ref()?;
        (id == stream).then(|| {
            Timestamp(
                at.0.saturating_add(
                    clock
                        .saturating_duration_since(*arrived)
                        .as_micros()
                        .min(u64::MAX as u128) as u64,
                ),
            )
        })
    }
    fn next_wake(&self, clock: Instant) -> Option<Duration> {
        let due = self
            .waiting
            .iter()
            .chain(self.sample.iter())
            .map(|pending| pending.due)
            .min()?;
        // Only pending work can require another timed wake (e.g. an active voice).
        Some(
            due.saturating_duration_since(clock)
                .max(Duration::from_millis(20)),
        )
    }
    pub fn retire(&mut self) {
        self.cancel_notifications();
        self.wake.signal();
        self.event_clock = None;
        self.stream = None;
        self.waiting = None;
        self.sample = None;
        self.last_play = None;
        if let Some(sink) = &mut self.sink {
            sink.stop();
        }
    }
    pub(super) fn cancel_notifications(&self) {
        self.notification_epoch.fetch_add(1, Ordering::AcqRel);
    }
    fn recovery_ready(&self, clock: Instant) -> bool {
        !self.shutdown
            && self.sample.as_ref().is_some_and(|p| p.due <= clock)
            && !self.waiting.as_ref().is_some_and(|p| p.due <= clock)
            && self.sink.as_ref().is_some_and(|s| !s.playing())
            && self
                .last_play
                .is_none_or(|last| clock.saturating_duration_since(last) >= COOLDOWN)
    }
    fn device_check_ready(&self, clock: Instant) -> bool {
        !self.shutdown
            && self.waiting.as_ref().is_some_and(|p| p.due <= clock)
            && self.sink.as_ref().is_some_and(|s| !s.playing())
            && self
                .last_play
                .is_none_or(|last| clock.saturating_duration_since(last) >= COOLDOWN)
    }
    pub fn enqueue(
        &mut self,
        alert: CommittedAlert,
        policy: &AlertPreferences,
        at: Timestamp,
        clock: Instant,
    ) {
        if self.shutdown
            || self.stream.as_deref() != Some(&alert.source_stream_id)
            || !visible(effective(&alert, policy, at))
        {
            return;
        }
        // Real arrivals take priority over the disposable generated sample.
        self.sample = None;
        self.wake.signal();
        // One waiting cue, replaced by the newest eligible arrival. Keep the
        // first deadline so a sustained burst cannot postpone delivery forever.
        let due = self.waiting.as_ref().map(|p| p.due).unwrap_or(clock);
        self.waiting = Some(Pending {
            alert,
            due,
            sample: false,
        });
    }
    pub fn status(&self) -> AlertDeliveryStatus {
        if let Some(status) = &self.recovery_status {
            return status.clone();
        }
        let sound_available = self.sink.as_ref().is_some_and(|s| s.sound_available());
        let message = self.message.clone().or_else(|| {
            (!sound_available)
                .then(|| "Sound device unavailable; use Test notification to retry.".into())
        });
        AlertDeliveryStatus {
            permission: self
                .sink
                .as_ref()
                .map_or(NotificationPermission::Unavailable, |s| s.permission()),
            sound_available,
            message,
            flawless_unavailable: maxed_engine::FLAWLESS_UNSUPPORTED.into(),
        }
    }
    pub(super) fn available(&self) -> bool {
        !self.shutdown && (self.sink.is_some() || self.recovery_status.is_some())
    }
    fn prepare(
        &mut self,
        runtime: &CaptureRuntime,
        at: Timestamp,
        clock: Instant,
    ) -> Option<NotificationTask> {
        let started = Instant::now();
        let attempt_at = || {
            Timestamp(
                at.0.saturating_add(started.elapsed().as_micros().min(u64::MAX as u128) as u64),
            )
        };
        if self.shutdown {
            self.retire();
            return None;
        }
        let Some(sink) = &mut self.sink else {
            return None;
        };
        if self
            .stream
            .as_ref()
            .is_none_or(|stream| !runtime.external_delivery_allowed(stream))
        {
            self.waiting = None;
            self.sample = None;
            sink.stop();
            return None;
        }
        let policy = runtime
            .shared
            .views
            .alert_policy
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if policy.muted || policy.volume_percent == 0 {
            sink.stop();
        }
        let sample_ready = self.sample.as_ref().is_some_and(|p| p.due <= clock);
        let ready = self.waiting.as_ref().is_some_and(|p| p.due <= clock);
        if !sample_ready && !ready {
            return None;
        }
        if sink.playing()
            || self
                .last_play
                .is_some_and(|last| clock.saturating_duration_since(last) < COOLDOWN)
        {
            return None;
        }
        let pending = (if ready {
            self.waiting.take()
        } else {
            self.sample.take()
        })?;
        let alert = &pending.alert;
        if !visible(pending.effective(&policy, attempt_at())) {
            return None;
        }
        // These are separate final-boundary checks, even with the delivery guard
        // held. No capture/session lock is held during either external call.
        if !policy.muted
            && policy.volume_percent > 0
            && runtime.external_delivery_allowed(&alert.source_stream_id)
            && visible(pending.effective(&policy, attempt_at()))
            && sink.sound_available()
        {
            match sink.play(alert.view.kind.clone(), policy.volume_percent) {
                Ok(()) => self.last_play = Some(clock),
                Err(error) => self.message = Some(error),
            }
        }
        policy.windows_notifications.then(|| NotificationTask {
            notifier: sink.notifier(),
            pending,
            generation: self.notification_epoch.load(Ordering::Acquire),
            epoch: self.notification_epoch.clone(),
            at,
            started,
        })
    }
    #[cfg(test)]
    pub(super) fn tick(&mut self, runtime: &CaptureRuntime, at: Timestamp, clock: Instant) {
        // Deterministic fake sinks only; production dispatch releases the guard.
        if self.recovery_ready(clock) {
            self.message = self.sink.as_mut().unwrap().recover().err();
        }
        if let Some(task) = self.prepare(runtime, at, clock) {
            if let Err(error) = task.run(runtime) {
                self.message = Some(error);
            }
        }
    }
}

impl CaptureRuntime {
    pub(crate) fn start_alert_delivery(
        self: &Arc<Self>,
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let weak = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("maxed-alerts".into())
            .spawn(move || {
                let Some(runtime) = weak.upgrade() else {
                    return;
                };
                let sink = super::native::NativeSink::new(app);
                {
                    let mut delivery = runtime
                        .shared
                        .views
                        .delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    if delivery.shutdown {
                        return;
                    }
                    delivery.sink = Some(Box::new(sink));
                }
                let wake = runtime.shared.views.alert_wake.clone();
                drop(runtime);
                Self::run_alert_worker(weak, wake);
            })
            .map_err(|_| "Alert worker could not start".to_string())?;
        Ok(())
    }
    pub(super) fn run_alert_worker(weak: Weak<Self>, wake: Arc<DeliveryWake>) {
        loop {
            let Some(runtime) = weak.upgrade() else {
                return;
            };
            #[cfg(test)]
            wake.cycles
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if runtime.alert_delivery_cycle(Instant::now()) {
                return;
            }
            let timeout = runtime
                .shared
                .views
                .delivery
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .next_wake(Instant::now());
            drop(runtime);
            wake.wait(timeout);
        }
    }
    pub(super) fn alert_delivery_cycle(&self, clock: Instant) -> bool {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Device discovery/open may block. Detach only with no active voice.
            let recovery = {
                let mut delivery = self
                    .shared
                    .views
                    .delivery
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let force_recovery = delivery.recovery_ready(clock);
                if (force_recovery || delivery.device_check_ready(clock))
                    && delivery
                        .stream
                        .as_ref()
                        .is_some_and(|stream| self.external_delivery_allowed(stream))
                {
                    delivery.recovery_status = Some(delivery.status());
                    Some((
                        delivery.sink.take().unwrap(),
                        delivery.notification_epoch.load(Ordering::Acquire),
                        force_recovery,
                    ))
                } else {
                    None
                }
            };
            let mut clock = clock;
            if let Some((mut sink, generation, force_recovery)) = recovery {
                let started = Instant::now();
                let result = (force_recovery || sink.device_stale()).then(|| sink.recover());
                // Device checks can block in the OS; that time counts against the pending alert.
                clock += started.elapsed();
                let mut delivery = self
                    .shared
                    .views
                    .delivery
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                delivery.recovery_status = None;
                if !delivery.shutdown {
                    delivery.sink = Some(sink);
                    if let Some(result) = result.filter(|_| {
                        delivery.notification_epoch.load(Ordering::Acquire) == generation
                    }) {
                        if force_recovery
                            || result.is_err()
                            || delivery.message.as_deref()
                                == Some("Sound device unavailable; use Test notification to retry.")
                        {
                            delivery.message = result.err();
                        }
                    }
                } else {
                    drop(delivery);
                    drop(sink);
                }
            }
            let mut delivery = self
                .shared
                .views
                .delivery
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let at = delivery
                .stream
                .as_ref()
                .and_then(|stream| delivery.event_now(stream, clock))
                .unwrap_or(Timestamp(0));
            let task = delivery.prepare(self, at, clock);
            let done = delivery.shutdown;
            drop(delivery);
            if let Some(task) = task {
                let result = task.run(self);
                if let Err(error) = result {
                    let mut delivery = self
                        .shared
                        .views
                        .delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    if task.current() {
                        delivery.message = Some(error);
                    }
                }
            }
            done
        }));
        match result {
            Ok(done) => done,
            Err(_) => {
                let mut delivery = self
                    .shared
                    .views
                    .delivery
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                delivery.retire();
                delivery.recovery_status = None;
                let sink = delivery.sink.take();
                delivery.shutdown = true;
                delivery.message = Some("Alert worker unavailable; restart Maxed.".into());
                // A failed sink must not poison subsequent source persistence.
                self.shared.views.delivery.clear_poison();
                drop(delivery);
                drop(sink);
                true
            }
        }
    }
    pub(crate) fn shutdown_alert_delivery(&self) {
        let mut delivery = self
            .shared
            .views
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        delivery.shutdown = true;
        delivery.recovery_status = None;
        delivery.retire();
        let sink = delivery.sink.take();
        drop(delivery);
        drop(sink);
    }
    pub(crate) fn retire_alert_delivery(&self) {
        self.shared
            .views
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retire();
    }
    pub fn request_notification_permission(&self) -> Result<NotificationPermission, String> {
        Ok(self.alert_delivery_status().permission)
    }
}

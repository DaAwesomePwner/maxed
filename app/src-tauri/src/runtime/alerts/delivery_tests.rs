use super::{delivery::*, *};
use crate::runtime::join::{Adapter, Bridge};
use maxed_store::{CommittedAlert, WriterConfig};
use std::sync::{atomic::Ordering, Mutex};
use std::time::{Duration, Instant};

#[derive(Default, Debug)]
struct Calls {
    play: usize,
    played_at: Option<Instant>,
    notify: usize,
    stop: usize,
    recover: usize,
    volume: u8,
    dropped: usize,
}
struct Spy {
    calls: Arc<Mutex<Calls>>,
    focus: Result<bool, String>,
    permission: NotificationPermission,
    device: bool,
    busy: bool,
    fail_notify: bool,
    panic_play: bool,
}
impl Drop for Spy {
    fn drop(&mut self) {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).dropped += 1;
    }
}
impl DeliverySink for Spy {
    fn play(&mut self, _: AlertKind, volume: u8) -> Result<(), String> {
        assert!(!self.panic_play, "injected sink panic");
        let mut calls = self.calls.lock().unwrap();
        calls.play += 1;
        calls.played_at = Some(Instant::now());
        calls.volume = volume;
        Ok(())
    }
    fn notifier(&self) -> Arc<dyn DeliveryNotifier> {
        Arc::new(SpyNotifier {
            calls: self.calls.clone(),
            focus: self.focus.clone(),
            permission: self.permission.clone(),
            fail: self.fail_notify,
        })
    }
    fn permission(&self) -> NotificationPermission {
        self.permission.clone()
    }
    fn sound_available(&self) -> bool {
        self.device
    }
    fn playing(&self) -> bool {
        self.busy
    }
    fn stop(&mut self) {
        self.calls.lock().unwrap().stop += 1;
        self.busy = false;
    }
    fn recover(&mut self) -> Result<(), String> {
        self.calls.lock().unwrap().recover += 1;
        self.device = true;
        Ok(())
    }
}
struct SpyNotifier {
    calls: Arc<Mutex<Calls>>,
    focus: Result<bool, String>,
    permission: NotificationPermission,
    fail: bool,
}

struct RecoveringSink {
    spy: Spy,
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}
impl DeliverySink for RecoveringSink {
    fn play(&mut self, kind: AlertKind, volume: u8) -> Result<(), String> {
        self.spy.play(kind, volume)
    }
    fn notifier(&self) -> Arc<dyn DeliveryNotifier> {
        self.spy.notifier()
    }
    fn permission(&self) -> NotificationPermission {
        self.spy.permission()
    }
    fn sound_available(&self) -> bool {
        self.spy.sound_available()
    }
    fn playing(&self) -> bool {
        self.spy.playing()
    }
    fn stop(&mut self) {
        self.spy.stop();
    }
    fn recover(&mut self) -> Result<(), String> {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        self.spy.recover()
    }
}

#[test]
fn recovery_preserves_cached_status_and_accepts_test_requests() {
    for device in [false, true] {
        let (runtime, calls) = setup("live-test");
        let runtime = Arc::new(runtime);
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let (release, resume) = crossbeam_channel::bounded(1);
        runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(RecoveringSink {
            spy: Spy {
                calls: calls.clone(),
                focus: Ok(false),
                permission: NotificationPermission::Granted,
                device,
                busy: false,
                fail_notify: false,
                panic_play: false,
            },
            entered,
            release: resume,
        }));
        runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
        let recovering = runtime.clone();
        let worker = std::thread::spawn(move || {
            recovering.alert_delivery_cycle(Instant::now() + Duration::from_secs(1))
        });
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let querying = runtime.clone();
        let (done, results) = crossbeam_channel::bounded(1);
        let query = std::thread::spawn(move || {
            done.send((
                querying.alert_delivery_status(),
                querying.test_alert_delivery(AlertKind::WhiteBag),
            ))
            .unwrap();
        });
        let during = results.recv_timeout(Duration::from_millis(250));
        release.send(()).unwrap();
        worker.join().unwrap();
        query.join().unwrap();
        let (status, test) = during.expect("recovery blocked status or Test");
        assert_eq!(status.permission, NotificationPermission::Granted);
        assert_eq!(status.sound_available, device);
        let test = test.expect("Test must remain available during recovery");
        assert_eq!(test.permission, NotificationPermission::Granted);
        assert_eq!(test.sound_available, device);
        assert!(runtime.alert_delivery_status().sound_available);
        assert_eq!(calls.lock().unwrap().play, 1);
    }
}

struct BlockingNotifier {
    stage: u8,
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
    calls: Arc<Mutex<Calls>>,
}

struct StaleDeviceSink {
    spy: Spy,
    runtime: std::sync::Weak<CaptureRuntime>,
    stale: Arc<std::sync::atomic::AtomicBool>,
    recover_delay: Duration,
}
impl StaleDeviceSink {
    fn assert_unlocked(&self) {
        let runtime = self.runtime.upgrade().unwrap();
        assert!(runtime.shared.views.delivery.try_lock().is_ok());
        assert!(runtime.session.try_lock().is_ok());
    }
}
impl DeliverySink for StaleDeviceSink {
    fn play(&mut self, kind: AlertKind, volume: u8) -> Result<(), String> {
        assert!(!self.stale.load(Ordering::Acquire));
        self.spy.play(kind, volume)
    }
    fn notifier(&self) -> Arc<dyn DeliveryNotifier> {
        self.spy.notifier()
    }
    fn permission(&self) -> NotificationPermission {
        self.spy.permission()
    }
    fn sound_available(&self) -> bool {
        self.spy.sound_available()
    }
    fn device_stale(&self) -> bool {
        self.assert_unlocked();
        self.stale.load(Ordering::Acquire)
    }
    fn playing(&self) -> bool {
        self.spy.playing()
    }
    fn stop(&mut self) {
        self.spy.stop();
    }
    fn recover(&mut self) -> Result<(), String> {
        self.assert_unlocked();
        std::thread::sleep(self.recover_delay);
        self.stale.store(false, Ordering::Release);
        self.spy.recover()
    }
}

#[test]
fn stale_device_recovers_unlocked_before_due_real_alert_only() {
    let (runtime, calls) = setup("live-test");
    let runtime = Arc::new(runtime);
    let stale = Arc::new(std::sync::atomic::AtomicBool::new(true));
    runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(StaleDeviceSink {
        spy: Spy {
            calls: calls.clone(),
            focus: Ok(false),
            permission: NotificationPermission::Granted,
            device: true,
            busy: false,
            fail_notify: false,
            panic_play: false,
        },
        runtime: Arc::downgrade(&runtime),
        stale: stale.clone(),
        recover_delay: Duration::ZERO,
    }));
    let clock = Instant::now();
    let enqueue = |due| {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        delivery.enqueue(
            candidate(
                AlertKind::WhiteBag,
                AlertLoudness::Banner,
                "live-test",
                now().0,
            ),
            &AlertPreferences::default(),
            now(),
            due,
        );
        assert!(delivery.sample.is_none());
    };
    enqueue(clock + Duration::from_secs(1));
    assert!(!runtime.alert_delivery_cycle(clock));
    assert_eq!(calls.lock().unwrap().recover, 0);
    assert!(!runtime.alert_delivery_cycle(clock + Duration::from_secs(1)));
    assert_eq!(calls.lock().unwrap().recover, 1);
    assert_eq!(calls.lock().unwrap().play, 1);

    enqueue(clock + Duration::from_secs(2));
    assert!(!runtime.alert_delivery_cycle(clock + Duration::from_secs(2)));
    assert_eq!(calls.lock().unwrap().recover, 1);
    assert_eq!(calls.lock().unwrap().play, 2);

    stale.store(true, Ordering::Release);
    enqueue(clock + Duration::from_millis(2100));
    assert!(!runtime.alert_delivery_cycle(clock + Duration::from_millis(2100)));
    assert_eq!(calls.lock().unwrap().recover, 1);
    assert_eq!(calls.lock().unwrap().play, 2);
    assert!(!runtime.alert_delivery_cycle(clock + Duration::from_secs(3)));
    assert_eq!(calls.lock().unwrap().recover, 2);
    assert_eq!(calls.lock().unwrap().play, 3);
}
#[test]
fn blocking_device_recovery_counts_against_alert_lifetime() {
    let (runtime, calls) = setup("live-test");
    let runtime = Arc::new(runtime);
    let arrived = Instant::now();
    runtime.shared.views.delivery.lock().unwrap().observe_clock(
        "live-test",
        Timestamp(now().0 + 1),
        arrived,
    );
    runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(StaleDeviceSink {
        spy: Spy {
            calls: calls.clone(),
            focus: Ok(false),
            permission: NotificationPermission::Granted,
            device: true,
            busy: false,
            fail_notify: false,
            panic_play: false,
        },
        runtime: Arc::downgrade(&runtime),
        stale: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        recover_delay: Duration::from_millis(60),
    }));
    // Due 20 ms before the 30 s lifetime ends; recovery blocks past it.
    let clock = arrived + Duration::from_secs(30) - Duration::from_millis(20);
    {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        delivery.enqueue(
            candidate(
                AlertKind::WhiteBag,
                AlertLoudness::Banner,
                "live-test",
                now().0,
            ),
            &AlertPreferences::default(),
            now(),
            clock,
        );
    }
    assert!(!runtime.alert_delivery_cycle(clock));
    let calls = calls.lock().unwrap();
    let (recover, play, notify) = (calls.recover, calls.play, calls.notify);
    drop(calls);
    assert_eq!((recover, play, notify), (1, 0, 0));
}
fn failed_device_setup(stale: bool) -> (Arc<CaptureRuntime>, Arc<Mutex<Calls>>) {
    let (runtime, calls) = setup("live-test");
    let runtime = Arc::new(runtime);
    runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(StaleDeviceSink {
        spy: Spy {
            calls: calls.clone(),
            focus: Ok(false),
            permission: NotificationPermission::Granted,
            device: false,
            busy: false,
            fail_notify: false,
            panic_play: false,
        },
        runtime: Arc::downgrade(&runtime),
        stale: Arc::new(std::sync::atomic::AtomicBool::new(stale)),
        recover_delay: Duration::ZERO,
    }));
    (runtime, calls)
}

fn deliver_due_real_alert(runtime: &CaptureRuntime, clock: Instant) {
    {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        delivery.enqueue(
            candidate(
                AlertKind::WhiteBag,
                AlertLoudness::Banner,
                "live-test",
                now().0,
            ),
            &AlertPreferences::default(),
            now(),
            clock,
        );
        assert!(delivery.sample.is_none());
    }
    assert!(!runtime.alert_delivery_cycle(clock));
}

#[test]
fn failed_stream_with_changed_default_recovers_unlocked_and_plays() {
    let (runtime, calls) = failed_device_setup(true);
    assert!(!runtime.alert_delivery_status().sound_available);
    deliver_due_real_alert(&runtime, Instant::now());
    assert_eq!(calls.lock().unwrap().recover, 1);
    assert_eq!(calls.lock().unwrap().play, 1);
    assert!(runtime.alert_delivery_status().sound_available);
}

#[test]
fn failed_stream_with_unchanged_default_does_not_auto_recover() {
    let (runtime, calls) = failed_device_setup(false);
    let clock = Instant::now();
    for seconds in 0..2 {
        deliver_due_real_alert(&runtime, clock + Duration::from_secs(seconds));
        assert_eq!(calls.lock().unwrap().recover, 0);
        assert_eq!(calls.lock().unwrap().play, 0);
        assert!(!runtime.alert_delivery_status().sound_available);
    }
}

#[test]
fn automatic_reopen_clears_only_sound_unavailable_message() {
    for message in [
        "Windows notification could not be delivered.",
        "Window focus unavailable; notification suppressed.",
        "Sound device unavailable; use Test notification to retry.",
    ] {
        let (runtime, calls) = failed_device_setup(true);
        runtime.shared.views.delivery.lock().unwrap().message = Some(message.into());
        deliver_due_real_alert(&runtime, Instant::now());
        assert_eq!(calls.lock().unwrap().recover, 1);
        assert_eq!(calls.lock().unwrap().play, 1);
        let expected = if message == "Sound device unavailable; use Test notification to retry." {
            None
        } else {
            Some(message)
        };
        assert_eq!(runtime.alert_delivery_status().message.as_deref(), expected);
    }
}

impl BlockingNotifier {
    fn wait(&self, stage: u8) {
        if self.stage == stage {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }
}
impl DeliveryNotifier for BlockingNotifier {
    fn refresh_permission(&self) -> NotificationPermission {
        self.wait(0);
        NotificationPermission::Granted
    }
    fn focused(&self) -> Result<bool, String> {
        self.wait(1);
        Ok(false)
    }
    fn notify_silent(&self, _: &AlertView) -> Result<(), String> {
        self.wait(2);
        self.calls.lock().unwrap().notify += 1;
        Ok(())
    }
}
struct NotificationOnlySink(Arc<dyn DeliveryNotifier>);
impl DeliverySink for NotificationOnlySink {
    fn notifier(&self) -> Arc<dyn DeliveryNotifier> {
        self.0.clone()
    }
    fn permission(&self) -> NotificationPermission {
        NotificationPermission::Granted
    }
    fn play(&mut self, _: AlertKind, _: u8) -> Result<(), String> {
        panic!("no sound device")
    }
    fn sound_available(&self) -> bool {
        false
    }
    fn playing(&self) -> bool {
        false
    }
    fn stop(&mut self) {}
    fn recover(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn blocking_notification_and_permission_calls_leave_capture_clock_unlocked() {
    for mode in 0..4 {
        let (runtime, calls) = setup("live-test");
        let runtime = Arc::new(runtime);
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let (release, resume) = crossbeam_channel::bounded(1);
        let policy = AlertPreferences {
            windows_notifications: true,
            ..Default::default()
        };
        *runtime.shared.views.alert_policy.lock().unwrap() = policy.clone();
        {
            let mut delivery = runtime.shared.views.delivery.lock().unwrap();
            delivery.sink = Some(Box::new(NotificationOnlySink(Arc::new(BlockingNotifier {
                stage: mode % 3,
                entered,
                release: resume,
                calls: calls.clone(),
            }))));
            delivery.enqueue(
                candidate(
                    AlertKind::WhiteBag,
                    AlertLoudness::Banner,
                    "live-test",
                    now().0,
                ),
                &policy,
                now(),
                Instant::now(),
            );
        }
        let sending = runtime.clone();
        let worker = std::thread::spawn(move || {
            if mode == 3 {
                sending.alert_delivery_status();
            } else {
                sending.alert_delivery_cycle(Instant::now());
            }
        });
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let observing = runtime.clone();
        let (done, observed) = crossbeam_channel::bounded(1);
        let capture = std::thread::spawn(move || {
            let mut delivery = observing.shared.views.delivery.lock().unwrap();
            delivery.observe_clock("live-test", Timestamp(2_000_000), Instant::now());
            let cached = delivery.status().permission;
            delivery.retire();
            done.send(cached).unwrap();
        });
        let progress = observed.recv_timeout(Duration::from_millis(250));
        release.send(()).unwrap();
        worker.join().unwrap();
        capture.join().unwrap();
        assert_eq!(
            progress.unwrap(),
            NotificationPermission::Granted,
            "mode {mode} held delivery guard"
        );
        // Retirement cancels work blocked before show. An already-issued show cannot be recalled.
        assert_eq!(calls.lock().unwrap().notify, usize::from(mode == 2));
    }
}
impl DeliveryNotifier for SpyNotifier {
    fn refresh_permission(&self) -> NotificationPermission {
        self.permission.clone()
    }
    fn focused(&self) -> Result<bool, String> {
        self.focus.clone()
    }
    fn notify_silent(&self, _: &AlertView) -> Result<(), String> {
        self.calls.lock().unwrap().notify += 1;
        if self.fail {
            Err("injected notification failure".into())
        } else {
            Ok(())
        }
    }
}
fn candidate(kind: AlertKind, loudness: AlertLoudness, stream: &str, at: u64) -> CommittedAlert {
    CommittedAlert {
        view: AlertView {
            id: "invented-occurrence".into(),
            at: Timestamp(at),
            kind: kind.clone(),
            run_id: None,
            explanation: "Invented alert".into(),
            details: (kind == AlertKind::KeyPopped).then(|| AlertDetails::KeyPopped {
                dungeon: "Lost Halls".into(),
            }),
            presentation: Some(loudness),
        },
        source_stream_id: stream.into(),
        source_sequence: Sequence(Timestamp(1)),
        loudness,
        committed_at: Timestamp(at),
    }
}
fn now() -> Timestamp {
    Timestamp(1_000_000)
}

fn setup(stream: &str) -> (CaptureRuntime, Arc<Mutex<Calls>>) {
    let runtime = CaptureRuntime::default();
    runtime.shared.views.snapshot.lock().unwrap().stream_id = stream.into();
    runtime.shared.status.lock().unwrap().state = CaptureState::Live;
    runtime.shared.views.store.lock().unwrap().state = StoreState::Ready;
    let calls = Arc::new(Mutex::new(Calls::default()));
    let mut delivery = runtime.shared.views.delivery.lock().unwrap();
    delivery.stream = Some(stream.into());
    delivery.observe_clock(stream, now(), Instant::now());
    delivery.sink = Some(Box::new(Spy {
        calls: calls.clone(),
        focus: Ok(false),
        permission: NotificationPermission::Granted,
        device: true,
        busy: false,
        fail_notify: false,
        panic_play: false,
    }));
    drop(delivery);
    (runtime, calls)
}
fn dispatch(runtime: &CaptureRuntime, alert: CommittedAlert, at: u64) {
    let clock = Instant::now();
    let mut delivery = runtime.shared.views.delivery.lock().unwrap();
    let policy = runtime.shared.views.alert_policy.lock().unwrap().clone();
    delivery.enqueue(alert, &policy, Timestamp(at), clock);
    delivery.tick(runtime, Timestamp(at), clock + Duration::from_secs(1));
}

#[test]
fn all_routes_mute_focus_namespace_matrix() {
    use AlertLoudness::*;
    let mut positive = false;
    for kind in [
        AlertKind::WhiteBag,
        AlertKind::PersonalBest,
        AlertKind::KeyPopped,
        AlertKind::FlawlessRun,
    ] {
        for level in [Off, QuietLogOnly, Toast, Banner] {
            for muted in [false, true] {
                for focused in [false, true] {
                    for enabled in [false, true] {
                        for permission in [
                            NotificationPermission::Granted,
                            NotificationPermission::Denied,
                            NotificationPermission::Unknown,
                        ] {
                            for stream in ["live-test", "demo-test", "replay-test", "live-retired"]
                            {
                                for device in [false, true] {
                                    let (runtime, calls) = setup(stream);
                                    if stream == "live-retired" {
                                        runtime.shared.views.snapshot.lock().unwrap().stream_id =
                                            "live-new".into();
                                    }
                                    *runtime.shared.views.alert_policy.lock().unwrap() =
                                        AlertPreferences {
                                            white_bag: level,
                                            personal_best: level,
                                            key_popped: level,
                                            muted,
                                            windows_notifications: enabled,
                                            ..Default::default()
                                        };
                                    runtime.shared.views.delivery.lock().unwrap().sink =
                                        Some(Box::new(Spy {
                                            calls: calls.clone(),
                                            focus: Ok(focused),
                                            permission: permission.clone(),
                                            device,
                                            busy: false,
                                            fail_notify: false,
                                            panic_play: false,
                                        }));
                                    dispatch(
                                        &runtime,
                                        candidate(kind.clone(), level, stream, 1_000_000),
                                        2_000_000,
                                    );
                                    let eligible = stream == "live-test"
                                        && kind != AlertKind::FlawlessRun
                                        && matches!(level, Toast | Banner);
                                    let calls = calls.lock().unwrap();
                                    assert_eq!(
                                        calls.play,
                                        usize::from(eligible && !muted && device)
                                    );
                                    assert_eq!(
                                        calls.notify,
                                        usize::from(
                                            eligible
                                                && !focused
                                                && enabled
                                                && permission == NotificationPermission::Granted
                                        )
                                    );
                                    positive |= calls.play == 1 && calls.notify == 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(positive);
}

#[test]
fn retirement_between_claim_and_sink() {
    let (runtime, calls) = setup("live-test");
    runtime
        .shared
        .views
        .alert_policy
        .lock()
        .unwrap()
        .windows_notifications = true;
    let clock = Instant::now();
    let alert = candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1);
    {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        delivery.enqueue(
            alert.clone(),
            &AlertPreferences::default(),
            Timestamp(2),
            clock,
        );
        // The queued value represents an already committed at-most-once claim.
        delivery.retire();
        runtime.shared.views.snapshot.lock().unwrap().stream_id = "demo-new".into();
        delivery.tick(&runtime, Timestamp(3), clock + Duration::from_secs(2));
        delivery.stream = Some("live-next".into());
        runtime.shared.views.snapshot.lock().unwrap().stream_id = "live-next".into();
        delivery.enqueue(
            candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "demo-new", 1),
            &AlertPreferences::default(),
            Timestamp(2),
            clock,
        );
        delivery.tick(&runtime, Timestamp(3), clock + Duration::from_secs(3));
    }
    {
        let calls = calls.lock().unwrap();
        assert_eq!((calls.play, calls.notify), (0, 0));
    }
    dispatch(
        &runtime,
        candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-next", 1),
        2,
    );
    assert_eq!(calls.lock().unwrap().play, 1);
}

#[test]
fn pending_unmute_current_volume_expiry_and_coalescing() {
    let (runtime, calls) = setup("live-test");
    let clock = Instant::now();
    let mut delivery = runtime.shared.views.delivery.lock().unwrap();
    let mut policy = AlertPreferences {
        muted: true,
        ..Default::default()
    };
    *runtime.shared.views.alert_policy.lock().unwrap() = policy.clone();
    for i in 0..500 {
        let mut alert = candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1);
        alert.view.id = format!("bag-{i}");
        delivery.enqueue(alert, &policy, Timestamp(2), clock);
    }
    assert_eq!(delivery.waiting.as_ref().unwrap().alert.view.id, "bag-499");
    assert_eq!(delivery.waiting.as_ref().unwrap().due, clock);
    policy.muted = false;
    policy.volume_percent = 67;
    *runtime.shared.views.alert_policy.lock().unwrap() = policy.clone();
    delivery.tick(&runtime, Timestamp(2), clock);
    assert_eq!(calls.lock().unwrap().volume, 67);
    delivery.tick(&runtime, Timestamp(3), clock + Duration::from_secs(2));
    assert_eq!(calls.lock().unwrap().play, 1);
    delivery.enqueue(
        candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1),
        &policy,
        Timestamp(2),
        clock,
    );
    delivery.tick(
        &runtime,
        Timestamp(30_000_002),
        clock + Duration::from_secs(4),
    );
    assert_eq!(calls.lock().unwrap().play, 1);
}

#[test]
fn stopped_storage_failure_shutdown_and_focus_failure_are_silent() {
    for reason in 0..4 {
        let (runtime, calls) = setup("live-test");
        runtime
            .shared
            .views
            .alert_policy
            .lock()
            .unwrap()
            .windows_notifications = true;
        match reason {
            0 => runtime.shared.status.lock().unwrap().state = CaptureState::Stopped,
            1 => runtime.shared.views.store.lock().unwrap().state = StoreState::Failed,
            2 => runtime.closing.store(true, Ordering::Release),
            _ => runtime.shared.views.delivery.lock().unwrap().shutdown = true,
        }
        dispatch(
            &runtime,
            candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1),
            2,
        );
        let calls = calls.lock().unwrap();
        assert_eq!((calls.play, calls.notify), (0, 0));
    }
    let (runtime, calls) = setup("live-test");
    runtime
        .shared
        .views
        .alert_policy
        .lock()
        .unwrap()
        .windows_notifications = true;
    runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(Spy {
        calls: calls.clone(),
        focus: Err("denied".into()),
        permission: NotificationPermission::Granted,
        device: true,
        busy: false,
        fail_notify: false,
        panic_play: false,
    }));
    dispatch(
        &runtime,
        candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1),
        2,
    );
    let calls = calls.lock().unwrap();
    assert_eq!((calls.play, calls.notify), (1, 0));
    assert!(runtime
        .alert_delivery_status()
        .message
        .unwrap()
        .contains("focus"));
}

#[test]
fn owner_test_is_bounded_immediate_and_has_no_history() {
    let (runtime, calls) = setup("live-test");
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    let mut delivery = runtime.shared.views.delivery.lock().unwrap();
    let pending = delivery.sample.as_ref().unwrap();
    assert_eq!(pending.alert.view.explanation, "Test notification");
    assert_eq!(pending.alert.view.kind, AlertKind::WhiteBag);
    let due = pending.due;
    assert!(due <= Instant::now());
    let at = pending.alert.committed_at;
    delivery.tick(&runtime, at, due - Duration::from_millis(1));
    assert_eq!(calls.lock().unwrap().play, 0);
    delivery.tick(&runtime, at, due);
    assert_eq!(calls.lock().unwrap().play, 1);
    assert!(delivery.sample.is_none());
    assert_eq!(runtime.snapshot().sequence, Sequence(Timestamp(0)));
    delivery.retire();
    drop(delivery);
    assert!(runtime.test_alert_delivery(AlertKind::WhiteBag).is_err());
}

#[test]
fn delivery_has_no_remote_inputs_and_assets_are_valid() {
    let (key_bytes, key_hash) = super::native::asset(&AlertKind::KeyPopped);
    for other in [
        AlertKind::PersonalBest,
        AlertKind::FlawlessRun,
        AlertKind::WhiteBag,
    ] {
        let (bytes, hash) = super::native::asset(&other);
        assert_ne!(key_hash, hash, "Key Popped must have a distinct cue");
        assert_ne!(key_bytes, bytes);
    }
    assert_eq!(key_bytes.len(), 6216);
    assert_eq!(&key_bytes[20..24], &[1, 0, 1, 0]); // PCM, mono
    assert_eq!(
        u32::from_le_bytes(key_bytes[24..28].try_into().unwrap()),
        22050
    );
    assert_eq!(&key_bytes[34..36], &[16, 0]);

    for kind in [
        AlertKind::WhiteBag,
        AlertKind::PersonalBest,
        AlertKind::KeyPopped,
        AlertKind::FlawlessRun,
    ] {
        let (bytes, hash) = super::native::asset(&kind);
        super::native::validate(bytes, hash).unwrap();
        assert!(super::native::validate(&bytes[..20], hash).is_err());
        let mut corrupt = bytes.to_vec();
        corrupt[20] ^= 1;
        assert!(super::native::validate(&corrupt, hash).is_err());
        use rodio::Source;
        let decoded = rodio::Decoder::try_from(std::io::Cursor::new(bytes)).unwrap();
        let duration = decoded.total_duration().unwrap();
        assert!(
            duration
                <= Duration::from_millis(if kind == AlertKind::WhiteBag {
                    1500
                } else if kind == AlertKind::KeyPopped {
                    150
                } else {
                    1000
                })
        );
        let samples: Vec<_> = decoded.collect();
        let peak = samples.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        assert!(20.0 * peak.log10() <= -0.9);
        let rms_band = if kind == AlertKind::WhiteBag {
            -18.1..=-16.1
        } else if kind == AlertKind::KeyPopped {
            assert!(20.0 * peak.log10() <= -12.0);
            -20.0..=-18.5
        } else {
            -11.3..=-5.3
        };
        assert!(rms_band.contains(&(20.0 * rms.log10())));
    }
    let capabilities = include_str!("../../../capabilities/default.json");
    assert!(!capabilities.contains("notification:"));
}

#[test]
fn gain_preserves_stored_volume_and_zero_is_muted() {
    use super::native::volume_gain;
    assert_eq!(volume_gain(0), 0.0);
    assert!((volume_gain(1) - 0.01).abs() < 0.00001);
    assert_eq!(volume_gain(100), 1.0);
    assert_eq!(volume_gain(255), 1.0);
    assert_eq!(volume_gain(35), 0.35);
    assert_eq!(volume_gain(70), 0.70);
    for p in 1..=100 {
        assert!(volume_gain(p) > volume_gain(p - 1));
    }
}

#[test]
fn explicit_notification_test_works_focused_without_a_sound_device() {
    let (runtime, calls) = setup("live-test");
    runtime
        .shared
        .views
        .alert_policy
        .lock()
        .unwrap()
        .windows_notifications = true;
    runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(Spy {
        calls: calls.clone(),
        focus: Ok(true),
        permission: NotificationPermission::Granted,
        device: false,
        busy: false,
        fail_notify: false,
        panic_play: false,
    }));
    assert_eq!(
        runtime.request_notification_permission().unwrap(),
        NotificationPermission::Granted
    );
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    runtime.alert_delivery_cycle(Instant::now());
    assert_eq!(calls.lock().unwrap().notify, 1);
    assert_eq!(
        super::notifications::text(&AlertKind::WhiteBag),
        ("White bag", "White bag observed")
    );
}

#[test]
fn alert_commit_wakes_claim_and_enqueue_without_advancing_capture_tick() {
    let dir = tempfile::tempdir().unwrap();
    let (runtime, calls) = setup("live-latency");
    let runtime = Arc::new(runtime);
    let views = runtime.shared.views.clone();
    let claimed = Arc::new(Mutex::new(None));
    let observed = claimed.clone();
    let mut adapter = Adapter::open(
        dir.path().join("latency.sqlite3"),
        Arc::new(
            serde_json::from_str(include_str!(
                "../../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
            ))
            .unwrap(),
        ),
        maxed_engine::EngineConfig {
            stream_id: "live-latency".into(),
            ..Default::default()
        },
        WriterConfig {
            stream_id: "live-latency".into(),
            ..Default::default()
        },
        false,
        views.clone(),
        Bridge {
            event: Arc::new(move |event| {
                if matches!(event.event, EngineEvent::AlertFired(_)) {
                    *observed.lock().unwrap() = Some(Instant::now());
                }
            }),
            ..Default::default()
        },
    )
    .unwrap();
    // Start a fresh interval. The capture clock stays frozen throughout delivery.
    adapter.events(vec![EngineEvent::SessionUpdated(
        demo::demo_snapshot().session.unwrap(),
    )]);
    adapter.live_traffic(Timestamp(1_000_000)).unwrap();
    adapter.writer_flush_for_test();
    adapter.refresh_store();
    let previous = views.store.lock().unwrap().durable;
    let committed = adapter.committed();
    while committed.try_recv().is_ok() {}
    {
        let mut delivery = views.delivery.lock().unwrap();
        delivery.stream = Some("live-latency".into());
        delivery.observe_clock("live-latency", Timestamp(1_000_000), Instant::now());
    }
    let started = Instant::now();
    adapter.events(vec![synthetic_white_bag(Timestamp(1_000_000))]);
    adapter.submit_pending_for_test();
    let submitted = started.elapsed();
    assert!(claimed.lock().unwrap().is_none());
    // No timer permit is sent: only the durable-head wake can service this select.
    let (_clock, capture_tick) = crossbeam_channel::bounded::<()>(1);
    crossbeam_channel::select! {
        recv(committed) -> result => result.unwrap(),
        recv(capture_tick) -> _ => panic!("capture tick must stay frozen"),
        default(Duration::from_secs(5)) => panic!("commit must wake the join loop"),
    }
    let commit = started.elapsed();
    adapter.refresh_store();
    assert_ne!(views.store.lock().unwrap().durable, previous);
    adapter.drain().unwrap();
    let claim = claimed
        .lock()
        .unwrap()
        .expect("committed white bag must reach the alert queue")
        .duration_since(started);
    assert!(views.delivery.lock().unwrap().waiting.is_some());
    assert_eq!(adapter.at(), Timestamp(1_000_000));
    let weak = Arc::downgrade(&runtime);
    let wake = views.alert_wake.clone();
    let worker = std::thread::spawn(move || CaptureRuntime::run_alert_worker(weak, wake));
    while calls.lock().unwrap().played_at.is_none() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime.shutdown_alert_delivery();
    worker.join().unwrap();
    let played = calls
        .lock()
        .unwrap()
        .played_at
        .expect("committed cue must reach the sink")
        .duration_since(started);
    eprintln!("alert latency: engine-to-submit={submitted:?}, engine-to-commit-wake={commit:?}, commit-wake-to-claim={:?}, engine-to-enqueue={claim:?}, engine-to-sink-play={played:?}", claim.saturating_sub(commit));
    adapter.close().unwrap();
}

#[test]
fn first_cue_is_immediate_and_cooldown_keeps_newest_waiter() {
    let (runtime, calls) = setup("live-test");
    let clock = Instant::now();
    let policy = AlertPreferences::default();
    let mut delivery = runtime.shared.views.delivery.lock().unwrap();
    delivery.enqueue(
        candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1),
        &policy,
        Timestamp(1),
        clock,
    );
    delivery.tick(&runtime, Timestamp(1), clock);
    assert_eq!(calls.lock().unwrap().play, 1);
    for i in 1..=3 {
        let mut alert = candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1);
        alert.view.id = format!("newest-{i}");
        let arrival = clock + Duration::from_millis(i * 90);
        delivery.enqueue(alert, &policy, Timestamp(1), arrival);
        delivery.tick(&runtime, Timestamp(1), arrival);
    }
    assert_eq!(calls.lock().unwrap().play, 1);
    assert_eq!(delivery.waiting.as_ref().unwrap().alert.view.id, "newest-3");
    delivery.tick(&runtime, Timestamp(1), clock + Duration::from_millis(300));
    assert_eq!(calls.lock().unwrap().play, 2);
    assert!(delivery.waiting.is_none());
}

#[test]
fn committed_envelope_override_preserves_order_clock_log_and_fixed_point() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("alerts.sqlite3");
    let events = Arc::new(Mutex::new(Vec::<EventEnvelope>::new()));
    let seen = events.clone();
    let mut adapter = Adapter::open(
        path.clone(),
        Arc::new(
            serde_json::from_str(include_str!(
                "../../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
            ))
            .unwrap(),
        ),
        maxed_engine::EngineConfig {
            stream_id: "demo-claims".into(),
            ..Default::default()
        },
        WriterConfig {
            stream_id: "demo-claims".into(),
            flush_millis: 60_000,
            ..Default::default()
        },
        false,
        Arc::new(crate::runtime::join::Views::default()),
        Bridge {
            event: Arc::new(move |e| seen.lock().unwrap().push(e)),
            ..Default::default()
        },
    )
    .unwrap();
    adapter.events(vec![EngineEvent::SessionUpdated(
        demo::demo_snapshot().session.unwrap(),
    )]);
    adapter.tick(Timestamp(180_000_000)).unwrap();
    adapter.events(vec![synthetic_white_bag(Timestamp(1))]);
    adapter.drain().unwrap();
    adapter.writer_flush_for_test();
    adapter.tick(Timestamp(181_000_000)).unwrap();
    adapter.close().unwrap();
    let events = events.lock().unwrap();
    let fired: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.event, EngineEvent::AlertFired(_)))
        .collect();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].at, Timestamp(180_000_000));
    assert_eq!(adapter.at(), Timestamp(181_000_000));
    assert!(events
        .windows(2)
        .all(|pair| pair[0].sequence.0 < pair[1].sequence.0));
    let EngineEvent::AlertFired(view) = &fired[0].event else {
        unreachable!()
    };
    assert_eq!(view.at, Timestamp(1));
    assert_eq!(view.presentation, Some(AlertLoudness::Banner));
    let reader = maxed_store::open_reader(&path).unwrap();
    let page = reader
        .list_alerts(AlertFilter {
            kinds: vec![],
            exclude_kinds: vec![],
            kind: None,
            range: None,
            query: String::new(),
            cursor: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].presentation, Some(AlertLoudness::Banner));
    let db = rusqlite::Connection::open(path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM alert_occurrences WHERE state='attempted'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn late_retiring_pb_is_fresh_and_displays_last_hit() {
    const LAST_HIT: u64 = 43_200_000_000; // 12:00:00
    const EMITTED: u64 = LAST_HIT + 180_000_000; // 12:03:00
    for current in [EMITTED + 1_000_000, EMITTED + 31_000_000] {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, calls) = setup("live-pb");
        runtime
            .shared
            .views
            .alert_policy
            .lock()
            .unwrap()
            .windows_notifications = true;
        let seen = Arc::new(Mutex::new(Vec::<EventEnvelope>::new()));
        let received = seen.clone();
        let path = dir.path().join("pb.sqlite3");
        let mut adapter = Adapter::open(
            path.clone(),
            Arc::new(
                serde_json::from_str(include_str!(
                    "../../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
                ))
                .unwrap(),
            ),
            maxed_engine::EngineConfig {
                stream_id: "live-pb".into(),
                ..Default::default()
            },
            WriterConfig {
                stream_id: "live-pb".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
            false,
            runtime.shared.views.clone(),
            Bridge {
                event: Arc::new(move |e| received.lock().unwrap().push(e)),
                ..Default::default()
            },
        )
        .unwrap();
        adapter.live = true;
        let mut character = demo::demo_run_story().character.unwrap();
        character.identity_verified = true;
        adapter.events(vec![
            EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
            EngineEvent::CharacterUpdated(character.clone()),
        ]);
        adapter.tick(Timestamp(EMITTED)).unwrap();
        for (id, dps) in [("first", 100.), ("record", 101.)] {
            let mut fight = demo::demo_snapshot().fights.remove(0);
            fight.id = id.into();
            fight.run_id = None;
            fight.complete_capture = true;
            fight.issues.clear();
            fight.boss_type = Some(42);
            fight.ended = Some(Timestamp(EMITTED));
            fight.end_reason = Some(FightEnd::MapChanged);
            fight.first_hit = Some(Timestamp(LAST_HIT - 1_000_000));
            fight.last_hit = Some(Timestamp(LAST_HIT));
            fight.players.retain(|p| p.is_local);
            fight.players[0].player_id = character.id.clone();
            fight.players[0].damage = dps;
            fight.players[0].dps = Some(dps);
            for source in &mut fight.players[0].sources {
                source.estimated = false;
            }
            adapter.events(vec![EngineEvent::FightEnded(fight)]);
        }
        adapter.drain().unwrap();
        adapter.writer_flush_for_test();
        // A newer source envelope has already advanced the clock before claim.
        adapter.tick(Timestamp(current)).unwrap();
        adapter.close().unwrap();
        let seen = seen.lock().unwrap();
        let alerts: Vec<_> = seen
            .iter()
            .filter_map(|e| {
                if let EngineEvent::AlertFired(v) = &e.event {
                    Some((e, v))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(alerts.len(), 1);
        let (envelope, view) = alerts[0];
        assert_eq!(envelope.at, Timestamp(EMITTED));
        assert_eq!(view.at, Timestamp(LAST_HIT));
        let fresh = current == EMITTED + 1_000_000;
        assert_eq!(
            view.presentation,
            Some(if fresh {
                AlertLoudness::Banner
            } else {
                AlertLoudness::QuietLogOnly
            })
        );
        assert_eq!(adapter.at(), Timestamp(current));
        runtime.shared.views.delivery.lock().unwrap().tick(
            &runtime,
            Timestamp(current),
            Instant::now() + Duration::from_secs(1),
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.play, usize::from(fresh));
        assert_eq!(calls.notify, usize::from(fresh));
        let persisted = maxed_store::open_reader(&path)
            .unwrap()
            .list_alerts(AlertFilter {
                kinds: vec![],
                exclude_kinds: vec![],
                kind: None,
                range: None,
                query: String::new(),
                cursor: None,
                limit: 100,
            })
            .unwrap();
        assert_eq!(persisted.rows.len(), 1);
        assert_eq!(persisted.rows[0].presentation, Some(AlertLoudness::Banner));
        let db = rusqlite::Connection::open(path).unwrap();
        assert_eq!(
            db.query_row("SELECT committed_at FROM alert_occurrences", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            EMITTED as i64
        );
    }
}

#[test]
fn policy_downgrade_never_revives_claimed_quiet_and_settings_cancel_test() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, calls) = setup("live-test");
    runtime
        .configure_alert_settings(Arc::new(
            crate::settings::SettingsStore::new(root.path().into()).unwrap(),
        ))
        .unwrap();
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    let mut policy = AlertPreferences {
        muted: true,
        ..Default::default()
    };
    runtime.update_alert_preferences(policy.clone()).unwrap();
    assert!(runtime
        .shared
        .views
        .delivery
        .lock()
        .unwrap()
        .sample
        .is_none());
    let clock = Instant::now();
    let at = now();
    runtime.shared.views.delivery.lock().unwrap().enqueue(
        candidate(
            AlertKind::WhiteBag,
            AlertLoudness::Banner,
            "live-test",
            at.0,
        ),
        &policy,
        at,
        clock,
    );
    policy.white_bag = AlertLoudness::QuietLogOnly;
    runtime.update_alert_preferences(policy.clone()).unwrap();
    policy.white_bag = AlertLoudness::Banner;
    policy.muted = false;
    runtime.update_alert_preferences(policy).unwrap();
    runtime.shared.views.delivery.lock().unwrap().tick(
        &runtime,
        at,
        clock + Duration::from_secs(1),
    );
    assert_eq!(calls.lock().unwrap().play, 0);
    dispatch(
        &runtime,
        candidate(
            AlertKind::WhiteBag,
            AlertLoudness::Banner,
            "live-test",
            at.0,
        ),
        at.0,
    );
    assert_eq!(calls.lock().unwrap().play, 1);
}

#[test]
fn one_voice_one_waiting_and_sink_failure_never_retries() {
    let (runtime, calls) = setup("live-test");
    runtime
        .shared
        .views
        .alert_policy
        .lock()
        .unwrap()
        .windows_notifications = true;
    let clock = Instant::now();
    let mut delivery = runtime.shared.views.delivery.lock().unwrap();
    delivery.sink = Some(Box::new(Spy {
        calls: calls.clone(),
        focus: Ok(false),
        permission: NotificationPermission::Granted,
        device: true,
        busy: true,
        fail_notify: true,
        panic_play: false,
    }));
    delivery.enqueue(
        candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1),
        &AlertPreferences::default(),
        Timestamp(2),
        clock,
    );
    delivery.tick(&runtime, Timestamp(2), clock + Duration::from_secs(1));
    assert!(delivery.waiting.is_some());
    assert_eq!(calls.lock().unwrap().play, 0);
    delivery.sink.as_mut().unwrap().stop();
    delivery.tick(&runtime, Timestamp(2), clock + Duration::from_secs(2));
    delivery.tick(&runtime, Timestamp(2), clock + Duration::from_secs(4));
    assert!(delivery.waiting.is_none());
    let calls = calls.lock().unwrap();
    assert_eq!((calls.play, calls.notify), (1, 1));
    assert_eq!(
        delivery.message.as_deref(),
        Some("injected notification failure")
    );
}

#[test]
fn demo_replay_packets_preserve_live_hashes_and_never_call_sinks() {
    use crate::runtime::{Namespace, RuntimeOptions};
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let (runtime, calls) = setup("live-test");
    let live = root.path().join("live.sqlite3");
    let writer = maxed_store::start_writer(live.clone(), WriterConfig::default()).unwrap();
    writer.shutdown().unwrap();
    let settings = Arc::new(crate::settings::SettingsStore::new(root.path().into()).unwrap());
    settings
        .update_alert_preferences(AlertPreferences {
            windows_notifications: true,
            ..Default::default()
        })
        .unwrap();
    runtime.configure_alert_settings(settings).unwrap();
    let settings_path = root.path().join("settings.json");
    let before = (
        Sha256::digest(std::fs::read(&live).unwrap()),
        Sha256::digest(std::fs::read(&settings_path).unwrap()),
    );
    let mut packets = Vec::new();
    {
        let mut reader = maxed_recording::PcapReader::new(std::io::Cursor::new(include_bytes!(
            "../../../../../tests/fixtures/scrubbed/maxed-demo/story.pcap"
        )))
        .unwrap();
        let mut writer = maxed_recording::PcapWriter::new(&mut packets).unwrap();
        while let Some(mut frame) = reader.next_frame().unwrap() {
            frame.at.0 += 1_800_000_000_000_000;
            writer.write_frame(&frame).unwrap();
        }
    }
    let mut streams = std::collections::HashSet::new();
    for namespace in [Namespace::Demo, Namespace::Demo, Namespace::Replay] {
        let mut options = RuntimeOptions::live(root.path().into(), None);
        options.namespace = namespace;
        runtime
            .replay_reader(options, std::io::Cursor::new(&packets), Bridge::default())
            .unwrap();
        assert!(streams.insert(runtime.snapshot().stream_id));
        assert!(runtime.test_alert_delivery(AlertKind::WhiteBag).is_err());
        runtime.shared.views.delivery.lock().unwrap().tick(
            &runtime,
            Timestamp(13_000_000),
            Instant::now() + Duration::from_secs(4),
        );
        let db =
            rusqlite::Connection::open(root.path().join(format!("{}.sqlite3", namespace.label())))
                .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM alert_occurrences WHERE kind='white_bag'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        let logs =
            maxed_store::open_reader(&root.path().join(format!("{}.sqlite3", namespace.label())))
                .unwrap()
                .list_alerts(AlertFilter {
                    kinds: vec![],
                    exclude_kinds: vec![],
                    kind: None,
                    range: None,
                    query: String::new(),
                    cursor: None,
                    limit: 100,
                })
                .unwrap();
        assert!(logs.rows.iter().any(|v| matches!(
            v.details,
            Some(AlertDetails::WhiteBag {
                ordinal: Some(1),
                ..
            })
        )));
        drop(db);
        runtime.reset_preview().unwrap();
    }
    let after = (
        Sha256::digest(std::fs::read(&live).unwrap()),
        Sha256::digest(std::fs::read(&settings_path).unwrap()),
    );
    assert_eq!(before, after);
    let calls = calls.lock().unwrap();
    assert_eq!((calls.play, calls.notify), (0, 0));
}

#[test]
fn regressed_clock_suppresses_without_inventing_age() {
    let alert = candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 100);
    assert_eq!(
        effective(&alert, &AlertPreferences::default(), Timestamp(99)),
        AlertLoudness::QuietLogOnly
    );
    assert_eq!(
        effective(&alert, &AlertPreferences::default(), Timestamp(0)),
        AlertLoudness::QuietLogOnly
    );
    assert_eq!(
        effective(&alert, &AlertPreferences::default(), Timestamp(100)),
        AlertLoudness::Banner
    );
    assert_eq!(
        effective(&alert, &AlertPreferences::default(), Timestamp(30_000_100)),
        AlertLoudness::Banner
    );
    assert_eq!(
        effective(&alert, &AlertPreferences::default(), Timestamp(30_000_101)),
        AlertLoudness::QuietLogOnly
    );
}

#[test]
fn original_loudness_caps_every_current_policy() {
    use AlertLoudness::*;
    for original in [Off, QuietLogOnly, Toast, Banner] {
        for current in [Off, QuietLogOnly, Toast, Banner] {
            let (runtime, calls) = setup("live-test");
            runtime.shared.views.alert_policy.lock().unwrap().white_bag = current;
            dispatch(
                &runtime,
                candidate(AlertKind::WhiteBag, original, "live-test", 1),
                2,
            );
            assert_eq!(
                calls.lock().unwrap().play,
                usize::from(
                    matches!(original, Toast | Banner) && matches!(current, Toast | Banner)
                )
            );
        }
    }
}

#[test]
fn finalization_claims_more_than_one_page_without_recursive_alerts() {
    let root = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::<EventEnvelope>::new()));
    let seen = events.clone();
    let mut adapter = Adapter::open(
        root.path().join("pages.sqlite3"),
        Arc::new(
            serde_json::from_str(include_str!(
                "../../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
            ))
            .unwrap(),
        ),
        maxed_engine::EngineConfig {
            stream_id: "demo-pages".into(),
            ..Default::default()
        },
        WriterConfig {
            stream_id: "demo-pages".into(),
            ..Default::default()
        },
        false,
        Arc::new(crate::runtime::join::Views::default()),
        Bridge {
            event: Arc::new(move |event| seen.lock().unwrap().push(event)),
            ..Default::default()
        },
    )
    .unwrap();
    adapter.events(vec![EngineEvent::SessionUpdated(
        demo::demo_snapshot().session.unwrap(),
    )]);
    adapter.tick(Timestamp(10_000_000)).unwrap();
    for i in 0..70 {
        adapter.events(vec![synthetic_white_bag(Timestamp(10_000_000 + i))]);
    }
    adapter.submit_pending_for_test();
    adapter.writer_flush_for_test();
    adapter.drain().unwrap();
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e.event, EngineEvent::AlertFired(_)))
            .count(),
        70
    );
    adapter.close().unwrap();
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, EngineEvent::AlertFired(_)))
            .count(),
        70
    );
    assert!(events
        .windows(2)
        .all(|pair| pair[0].sequence.0 .0 + 1 == pair[1].sequence.0 .0));
}

#[test]
fn delivery_worker_panic_releases_voice_and_queue_without_fallback() {
    let (runtime, calls) = setup("live-test");
    let clock = Instant::now();
    {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        delivery.sink = Some(Box::new(Spy {
            calls: calls.clone(),
            focus: Ok(false),
            permission: NotificationPermission::Granted,
            device: true,
            busy: false,
            fail_notify: false,
            panic_play: true,
        }));
        delivery.enqueue(
            candidate(AlertKind::WhiteBag, AlertLoudness::Banner, "live-test", 1),
            &AlertPreferences::default(),
            Timestamp(2),
            clock,
        );
    }
    assert!(runtime.alert_delivery_cycle(clock + Duration::from_secs(1)));
    assert!(!runtime.shared.views.delivery.is_poisoned());
    assert!(runtime.test_alert_delivery(AlertKind::WhiteBag).is_err());
    let delivery = runtime
        .shared
        .views
        .delivery
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    assert!(
        delivery.shutdown
            && delivery.sink.is_none()
            && delivery.waiting.is_none()
            && delivery.sample.is_none()
    );
    assert!(delivery
        .status()
        .message
        .unwrap()
        .contains("worker unavailable"));
    let calls = calls.lock().unwrap();
    assert_eq!((calls.play, calls.notify), (0, 0));
    assert_eq!(calls.dropped, 2);
}

#[test]
fn device_failure_retries_only_explicit_test_and_never_replays_old_alert() {
    let (runtime, calls) = setup("live-test");
    runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(Spy {
        calls: calls.clone(),
        focus: Ok(false),
        permission: NotificationPermission::Unavailable,
        device: false,
        busy: false,
        fail_notify: false,
        panic_play: false,
    }));
    let at = now();
    dispatch(
        &runtime,
        candidate(
            AlertKind::WhiteBag,
            AlertLoudness::Banner,
            "live-test",
            at.0,
        ),
        at.0,
    );
    assert!(!runtime.alert_delivery_status().sound_available);
    assert!(runtime
        .alert_delivery_status()
        .message
        .unwrap()
        .contains("Sound device unavailable"));
    let clock = Instant::now();
    runtime.alert_delivery_cycle(clock + Duration::from_secs(2));
    assert_eq!(calls.lock().unwrap().recover, 0);
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    runtime.alert_delivery_cycle(clock + Duration::from_secs(4));
    let calls = calls.lock().unwrap();
    assert_eq!((calls.play, calls.recover), (1, 1));
    assert!(runtime.alert_delivery_status().sound_available);
}

#[test]
fn arming_test_preserves_real_cue_and_real_arrival_is_not_discarded() {
    let (runtime, calls) = setup("live-test");
    let at = now();
    let clock = Instant::now();
    {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        delivery.enqueue(
            candidate(
                AlertKind::WhiteBag,
                AlertLoudness::Banner,
                "live-test",
                at.0,
            ),
            &AlertPreferences::default(),
            at,
            clock,
        );
    }
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        assert!(
            delivery.waiting.is_some(),
            "Test must not consume a real claim"
        );
        delivery.tick(&runtime, at, clock + Duration::from_secs(1));
    }
    assert_eq!(calls.lock().unwrap().play, 1);
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    let mut delivery = runtime.shared.views.delivery.lock().unwrap();
    let mut real = candidate(
        AlertKind::PersonalBest,
        AlertLoudness::Banner,
        "live-test",
        at.0,
    );
    real.view.id = "new-real-arrival".into();
    delivery.enqueue(
        real,
        &AlertPreferences::default(),
        at,
        clock + Duration::from_secs(2),
    );
    assert_eq!(
        delivery.waiting.as_ref().unwrap().alert.view.id,
        "new-real-arrival"
    );
    delivery.tick(&runtime, at, clock + Duration::from_secs(3));
    assert_eq!(calls.lock().unwrap().play, 2);
}

#[test]
fn suppressed_test_still_retries_device_without_playing() {
    for level in [AlertLoudness::Off, AlertLoudness::QuietLogOnly] {
        let (runtime, calls) = setup("live-test");
        runtime.shared.views.alert_policy.lock().unwrap().white_bag = level;
        runtime.shared.views.delivery.lock().unwrap().sink = Some(Box::new(Spy {
            calls: calls.clone(),
            focus: Ok(false),
            permission: NotificationPermission::Unavailable,
            device: false,
            busy: false,
            fail_notify: false,
            panic_play: false,
        }));
        runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        let sample = delivery.sample.as_ref().unwrap();
        let at = sample.alert.committed_at;
        let due = sample.due;
        delivery.tick(&runtime, at, due);
        assert_eq!(calls.lock().unwrap().recover, 1);
        assert_eq!(calls.lock().unwrap().play, 0);
        assert!(delivery.status().sound_available);
    }
}

#[test]
fn live_claims_and_sinks_use_packet_time_despite_wall_clock_skew() {
    use std::time::{SystemTime, UNIX_EPOCH};
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    for packet in [wall - 3_600_000_000, wall + 3_600_000_000] {
        for delayed in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (runtime, calls) = setup("live-skew");
            let seen = Arc::new(Mutex::new(Vec::<EventEnvelope>::new()));
            let received = seen.clone();
            let mut adapter = Adapter::open(
                dir.path().join("skew.sqlite3"),
                Arc::new(
                    serde_json::from_str(include_str!(
                        "../../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
                    ))
                    .unwrap(),
                ),
                maxed_engine::EngineConfig {
                    stream_id: "live-skew".into(),
                    ..Default::default()
                },
                WriterConfig {
                    stream_id: "live-skew".into(),
                    flush_millis: 60_000,
                    ..Default::default()
                },
                false,
                runtime.shared.views.clone(),
                Bridge {
                    event: Arc::new(move |e| received.lock().unwrap().push(e)),
                    ..Default::default()
                },
            )
            .unwrap();
            // The source receipt Instant is the only elapsed-time anchor. The
            // second case simulates persistence held for 31 s with no new frames.
            let received_at = Instant::now() - Duration::from_secs(if delayed { 31 } else { 0 });
            runtime.shared.views.delivery.lock().unwrap().observe_clock(
                "live-skew",
                Timestamp(packet),
                received_at,
            );
            adapter.tick(Timestamp(packet)).unwrap();
            adapter.events(vec![
                EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
                synthetic_white_bag(Timestamp(packet)),
            ]);
            adapter.close().unwrap();
            let fired = seen
                .lock()
                .unwrap()
                .iter()
                .find_map(|e| {
                    if let EngineEvent::AlertFired(view) = &e.event {
                        Some(view.clone())
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(
                fired.presentation,
                Some(if delayed {
                    AlertLoudness::QuietLogOnly
                } else {
                    AlertLoudness::Banner
                })
            );
            runtime.alert_delivery_cycle(Instant::now() + Duration::from_secs(1));
            assert_eq!(calls.lock().unwrap().play, usize::from(!delayed));
            // No new capture frame is needed to expire a still-pending cue.
            let mut delivery = runtime.shared.views.delivery.lock().unwrap();
            let at = delivery.event_now("live-skew", Instant::now()).unwrap();
            delivery.enqueue(
                candidate(
                    AlertKind::WhiteBag,
                    AlertLoudness::Banner,
                    "live-skew",
                    packet,
                ),
                &AlertPreferences::default(),
                at,
                Instant::now(),
            );
            drop(delivery);
            runtime.alert_delivery_cycle(Instant::now() + Duration::from_secs(32));
            assert_eq!(calls.lock().unwrap().play, usize::from(!delayed));
        }
    }
}

#[test]
fn reset_preview_does_not_hold_delivery_guard_while_history_prepares() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, _) = setup("live-test");
    let runtime = Arc::new(runtime);
    let (entered, preparing) = crossbeam_channel::bounded(1);
    let (release, resume) = crossbeam_channel::bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    runtime.initialize_home(root.path().into()).unwrap();
    preparing.recv_timeout(Duration::from_secs(5)).unwrap();
    let generation = runtime.shared.home.generation.load(Ordering::Acquire);
    let resetting = runtime.clone();
    let reset = std::thread::spawn(move || resetting.reset_preview());
    let deadline = Instant::now() + Duration::from_secs(2);
    while runtime.shared.home.generation.load(Ordering::Acquire) == generation {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let (done, commands) = crossbeam_channel::bounded(1);
    let querying = runtime.clone();
    let query = std::thread::spawn(move || {
        let status = querying.alert_delivery_status();
        let test = querying.test_alert_delivery(AlertKind::WhiteBag);
        done.send((status, test)).unwrap();
    });
    let early = commands.recv_timeout(Duration::from_millis(250));
    release.send(()).unwrap();
    query.join().unwrap();
    reset.join().unwrap().unwrap();
    assert!(
        early.is_ok(),
        "alert commands blocked behind cold history preparation"
    );
    assert!(
        early.unwrap().1.is_err(),
        "retired preview cannot arm a Test"
    );
}

#[test]
fn delivery_worker_blocks_when_idle_and_wakes_for_each_work_transition() {
    fn wait_until(mut ready: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(3);
        while !ready() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let (runtime, calls) = setup("live-test");
    let runtime = Arc::new(runtime);
    let wake = runtime.shared.views.alert_wake.clone();
    let worker_wake = wake.clone();
    let weak = Arc::downgrade(&runtime);
    let worker = std::thread::spawn(move || CaptureRuntime::run_alert_worker(weak, worker_wake));
    wait_until(|| wake.cycles.load(Ordering::SeqCst) == 1);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(
        wake.cycles.load(Ordering::SeqCst),
        1,
        "idle worker must block indefinitely"
    );
    {
        let mut delivery = runtime.shared.views.delivery.lock().unwrap();
        let at = delivery.event_now("live-test", Instant::now()).unwrap();
        delivery.enqueue(
            candidate(
                AlertKind::WhiteBag,
                AlertLoudness::Banner,
                "live-test",
                at.0,
            ),
            &AlertPreferences::default(),
            at,
            Instant::now(),
        );
    }
    wait_until(|| calls.lock().unwrap().play == 1);
    let cycles = wake.cycles.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(
        wake.cycles.load(Ordering::SeqCst),
        cycles,
        "no polling after queue drains"
    );
    runtime.test_alert_delivery(AlertKind::WhiteBag).unwrap();
    wait_until(|| wake.cycles.load(Ordering::SeqCst) > cycles);
    let cycles = wake.cycles.load(Ordering::SeqCst);
    runtime.retire_alert_delivery();
    wait_until(|| wake.cycles.load(Ordering::SeqCst) > cycles);
    std::thread::sleep(Duration::from_millis(30));
    let cycles = wake.cycles.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(
        wake.cycles.load(Ordering::SeqCst),
        cycles,
        "retirement cancels timed waits"
    );
    runtime.shutdown_alert_delivery();
    worker.join().unwrap();
    assert!(runtime.shared.views.delivery.lock().unwrap().sink.is_none());
}

#[test]
fn idle_worker_does_not_keep_runtime_alive() {
    let runtime = Arc::new(setup("live-test").0);
    let wake = runtime.shared.views.alert_wake.clone();
    let weak = Arc::downgrade(&runtime);
    let inspect = weak.clone();
    let (done, finished) = crossbeam_channel::bounded(1);
    let worker = std::thread::spawn(move || {
        CaptureRuntime::run_alert_worker(weak, wake);
        done.send(()).unwrap();
    });
    drop(runtime);
    finished.recv_timeout(Duration::from_secs(3)).unwrap();
    worker.join().unwrap();
    assert!(inspect.upgrade().is_none());
}

fn synthetic_white_bag(at: Timestamp) -> EngineEvent {
    let mut drop = demo::demo_run_story().drops.remove(0);
    drop.id = format!("delivery-bag-{}", at.0);
    drop.at = at;
    drop.run_id = None;
    drop.bag_rank = Some(255);
    EngineEvent::BagDropped(drop)
}

#[test]
fn queued_key_pops_recheck_saved_dungeon_filter() {
    for kind in [AlertKind::KeyPopped, AlertKind::PersonalBest] {
        for selected in [
            None,
            Some(vec!["Lost Halls".into()]),
            Some(vec!["The Void".into()]),
            Some(vec![]),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, calls) = setup("live-test");
            runtime
                .configure_alert_settings(Arc::new(
                    crate::settings::SettingsStore::new(root.path().into()).unwrap(),
                ))
                .unwrap();
            let mut policy = AlertPreferences {
                windows_notifications: true,
                ..Default::default()
            };
            runtime.update_alert_preferences(policy.clone()).unwrap();
            let clock = Instant::now();
            runtime.shared.views.delivery.lock().unwrap().enqueue(
                candidate(kind.clone(), AlertLoudness::Banner, "live-test", now().0),
                &policy,
                now(),
                clock,
            );
            assert!(runtime
                .shared
                .views
                .delivery
                .lock()
                .unwrap()
                .waiting
                .is_some());
            let expected = usize::from(
                kind != AlertKind::KeyPopped
                    || selected
                        .as_ref()
                        .is_none_or(|names| names.iter().any(|name| name == "Lost Halls")),
            );
            policy.key_pop_dungeons = selected;
            runtime.update_alert_preferences(policy.clone()).unwrap();
            let queued = runtime
                .shared
                .views
                .delivery
                .lock()
                .unwrap()
                .waiting
                .is_some();
            assert_eq!(queued, expected == 1);
            runtime.shared.views.delivery.lock().unwrap().tick(
                &runtime,
                now(),
                clock + Duration::from_secs(1),
            );
            let delivered = {
                let calls = calls.lock().unwrap();
                (calls.play, calls.notify)
            };
            assert_eq!(delivered, (expected, expected));
            policy.key_pop_dungeons = None;
            runtime.update_alert_preferences(policy).unwrap();
            runtime.shared.views.delivery.lock().unwrap().tick(
                &runtime,
                now(),
                clock + Duration::from_secs(2),
            );
            let delivered = {
                let calls = calls.lock().unwrap();
                (calls.play, calls.notify)
            };
            assert_eq!(delivered, (expected, expected));
        }
    }
}

#[test]
fn key_pop_final_delivery_rechecks_current_dungeon_filter() {
    for notifications_only in [false, true] {
        let (runtime, calls) = setup("live-test");
        let runtime = Arc::new(runtime);
        let policy = AlertPreferences {
            windows_notifications: true,
            ..Default::default()
        };
        *runtime.shared.views.alert_policy.lock().unwrap() = policy.clone();
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let (release, resume) = crossbeam_channel::bounded(1);
        if notifications_only {
            runtime.shared.views.delivery.lock().unwrap().sink =
                Some(Box::new(NotificationOnlySink(Arc::new(BlockingNotifier {
                    stage: 1,
                    entered,
                    release: resume,
                    calls: calls.clone(),
                }))));
        }
        runtime.shared.views.delivery.lock().unwrap().enqueue(
            candidate(
                AlertKind::KeyPopped,
                AlertLoudness::Toast,
                "live-test",
                now().0,
            ),
            &policy,
            now(),
            Instant::now(),
        );
        if notifications_only {
            let sending = runtime.clone();
            let worker = std::thread::spawn(move || sending.alert_delivery_cycle(Instant::now()));
            waiting.recv_timeout(Duration::from_secs(5)).unwrap();
            runtime
                .shared
                .views
                .alert_policy
                .lock()
                .unwrap()
                .key_pop_dungeons = Some(vec![]);
            release.send(()).unwrap();
            worker.join().unwrap();
        } else {
            runtime
                .shared
                .views
                .alert_policy
                .lock()
                .unwrap()
                .key_pop_dungeons = Some(vec![]);
            runtime.alert_delivery_cycle(Instant::now());
        }
        let delivered = {
            let calls = calls.lock().unwrap();
            (calls.play, calls.notify)
        };
        assert_eq!(delivered, (0, 0));
    }
}

#[test]
fn explicit_key_pop_test_remains_available_with_no_dungeons_selected() {
    let (runtime, calls) = setup("live-test");
    *runtime.shared.views.alert_policy.lock().unwrap() = AlertPreferences {
        key_pop_dungeons: Some(vec![]),
        windows_notifications: true,
        ..Default::default()
    };
    runtime.test_alert_delivery(AlertKind::KeyPopped).unwrap();
    runtime.alert_delivery_cycle(Instant::now());
    assert_eq!(calls.lock().unwrap().play, 1);
    assert_eq!(calls.lock().unwrap().notify, 1);
}

#[test]
fn run_time_pb_is_live_only_and_uses_personal_best_delivery() {
    for (stream, ephemeral) in [
        ("live-time-pb", false),
        ("demo-time-pb", true),
        ("replay-time-pb", true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, calls) = setup(stream);
        runtime
            .shared
            .views
            .alert_policy
            .lock()
            .unwrap()
            .windows_notifications = true;
        let seen = Arc::new(Mutex::new(Vec::<EventEnvelope>::new()));
        let received = seen.clone();
        let path = dir.path().join("time-pb.sqlite3");
        let mut adapter = Adapter::open(
            path.clone(),
            Arc::new(
                serde_json::from_str(include_str!(
                    "../../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
                ))
                .unwrap(),
            ),
            maxed_engine::EngineConfig {
                stream_id: stream.into(),
                ..Default::default()
            },
            WriterConfig {
                stream_id: stream.into(),
                flush_millis: 60_000,
                ..Default::default()
            },
            false,
            runtime.shared.views.clone(),
            Bridge {
                event: Arc::new(move |e| received.lock().unwrap().push(e)),
                ..Default::default()
            },
        )
        .unwrap();
        adapter.ephemeral = ephemeral;
        adapter.live = !ephemeral;
        adapter.events(vec![EngineEvent::SessionUpdated(
            demo::demo_snapshot().session.unwrap(),
        )]);
        adapter.tick(Timestamp(200_000_000)).unwrap();
        for (id, duration) in [
            ("prior", 87_000_000),
            ("faster", 86_000_000),
            ("faster", 86_000_000),
        ] {
            let mut run = demo::demo_run_story().run;
            run.id = id.into();
            run.character_id = None;
            run.dungeon = "Lair of Shaitan".into();
            run.area_kind = AreaKind::Dungeon;
            run.started = Timestamp(1_000_000);
            run.completed_at = Some(Timestamp(1_000_000 + duration));
            run.ended = Some(Timestamp(200_000_000));
            run.outcome = Some(Outcome::Cleared);
            run.complete_capture = false;
            run.issues = vec![CoverageIssue::MissingStats];
            run.evidence = vec![EvidenceAt {
                at: run.completed_at.unwrap(),
                evidence: Evidence::Victory,
            }];
            adapter.events(vec![EngineEvent::RunEnded(run)]);
        }
        adapter.drain().unwrap();
        adapter.writer_flush_for_test();
        adapter.tick(Timestamp(201_000_000)).unwrap();
        let count = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e.event, EngineEvent::AlertFired(_)))
            .count();
        assert_eq!(count, usize::from(!ephemeral), "{stream}");
        runtime.shared.views.delivery.lock().unwrap().tick(
            &runtime,
            Timestamp(201_000_000),
            Instant::now() + Duration::from_secs(1),
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.play, usize::from(!ephemeral), "{stream}");
        assert_eq!(calls.notify, usize::from(!ephemeral), "{stream}");
        let logs = maxed_store::open_reader(&path)
            .unwrap()
            .list_alerts(AlertFilter {
                kinds: vec![],
                exclude_kinds: vec![],
                kind: None,
                range: None,
                query: String::new(),
                cursor: None,
                limit: 100,
            })
            .unwrap();
        assert_eq!(logs.rows.len(), usize::from(!ephemeral), "{stream}");
        drop(calls);
        adapter.close().unwrap();
    }
}

#[test]
fn key_pop_test_uses_a_catalog_dungeon_and_prefers_portal_artwork() {
    let (runtime, _) = setup("live-test");
    let mut catalog = maxed_types::demo::demo_catalog();
    for dungeon in &mut catalog.dungeons {
        dungeon.portal_type = None;
    }
    let dungeon = catalog
        .dungeons
        .iter_mut()
        .find(|d| d.kind == AreaKind::Dungeon)
        .unwrap();
    dungeon.portal_type = Some(catalog.objects[0].object_type);
    dungeon.name = "Synthetic portal dungeon".into();
    runtime
        .test_alert_delivery_with_catalog(AlertKind::KeyPopped, Some(&catalog))
        .unwrap();
    let delivery = runtime.shared.views.delivery.lock().unwrap();
    assert_eq!(
        delivery.sample.as_ref().unwrap().alert.view.details,
        Some(AlertDetails::KeyPopped {
            dungeon: "Synthetic portal dungeon".into()
        })
    );
}

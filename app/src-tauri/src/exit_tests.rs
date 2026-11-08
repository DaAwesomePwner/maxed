use super::*;
use std::cell::{Cell, RefCell};

#[test]
fn startup_failure_decision_across_automation_states() {
    let variables = [
        "MAXED_GUI_SMOKE",
        "MAXED_MEASURE_STARTUP",
        "MAXED_MEASURE_DEMO",
    ];
    for mask in 0..8 {
        let shown = AtomicBool::new(false);
        let failure = startup_failure(&shown, |name| {
            variables
                .iter()
                .enumerate()
                .any(|(bit, variable)| name == *variable && mask & (1 << bit) != 0)
        });
        assert_eq!(failure.exit_code, 1, "mask {mask}");
        assert_eq!(failure.show_dialog, mask == 0, "mask {mask}");
        assert_eq!(shown.load(Ordering::Acquire), mask == 0);
        // Suppressed reports must not consume the one visible notification.
        let next = startup_failure(&shown, |_| false);
        assert_eq!(next.show_dialog, mask != 0);
        assert_eq!(next.exit_code, 1);
    }
}

#[test]
fn startup_failure_dialog_is_claimed_at_most_once_across_threads() {
    let shown = AtomicBool::new(false);
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    startup_failure(&shown, |_| false)
                })
            })
            .collect();
        let mut dialogs = 0;
        for worker in workers {
            let failure = worker.join().unwrap();
            assert_eq!(failure.exit_code, 1);
            dialogs += usize::from(failure.show_dialog);
        }
        assert_eq!(dialogs, 1);
    });
    assert!(!startup_failure(&shown, |_| false).show_dialog);
}

#[derive(Default)]
struct Fake {
    installing: bool,
    delivery_failed: Cell<bool>,
    update_error: bool,
    unsaved: Cell<bool>,
    shutdown_error: bool,
    retry_error: bool,
    calls: RefCell<Vec<&'static str>>,
    code: Cell<Option<i32>>,
}
impl ExitHost for Fake {
    fn installing(&self) -> bool {
        self.installing
    }
    fn stop_updates(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("updates");
        if self.update_error {
            Err("busy".into())
        } else {
            Ok(())
        }
    }
    fn prepare(&self) {
        self.calls.borrow_mut().push("prepare");
    }
    fn resume(&self) {
        self.calls.borrow_mut().push("resume");
    }
    fn delivery_failed(&self) -> bool {
        self.delivery_failed.get()
    }
    fn unsaved(&self) -> bool {
        self.unsaved.get()
    }
    fn retry_save(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("retry");
        if self.retry_error {
            Err("failed".into())
        } else {
            self.unsaved.set(false);
            Ok(())
        }
    }
    fn shutdown(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("shutdown");
        if self.shutdown_error {
            Err("failed".into())
        } else {
            Ok(())
        }
    }
    fn discard(&self) {
        self.calls.borrow_mut().push("discard");
        self.unsaved.set(false);
    }
    fn warn(&self, native_fallback: bool) {
        self.calls.borrow_mut().push(if native_fallback {
            "native-warn"
        } else {
            "warn"
        });
    }
    fn exit(&self, code: i32) {
        self.code.set(Some(code));
    }
}
#[test]
fn busy_update_and_shutdown_error_release_guard_before_setup() {
    for installing in [true, false] {
        let control = ExitControl::default();
        let host = Fake {
            installing,
            update_error: true,
            ..Default::default()
        };
        assert!(run(&control, &host, false).is_err());
        assert!(!control.exiting.load(Ordering::Acquire));
        assert!(!host.calls.borrow().contains(&"prepare"));
        assert_eq!(host.code.get(), None);
    }
}
#[test]
fn cancelled_unsaved_close_preserves_data_until_explicit_discard() {
    let control = ExitControl::default();
    let host = Fake {
        unsaved: Cell::new(true),
        shutdown_error: true,
        retry_error: true,
        ..Default::default()
    };
    for _ in 0..2 {
        assert!(run(&control, &host, false).is_err());
        assert!(host.unsaved.get());
        assert!(!control.exiting.load(Ordering::Acquire));
        assert_eq!(host.code.get(), None);
    }
    assert_eq!(
        &host.calls.borrow()[..4],
        &["updates", "prepare", "shutdown", "resume"]
    );
    run(&control, &host, true).unwrap();
    assert!(!host.unsaved.get());
    assert!(control.exiting.load(Ordering::Acquire));
    assert!(control.finished.load(Ordering::Acquire));
    assert_eq!(host.code.get(), Some(0));
    assert!(run(&control, &host, true).is_err());
}
#[test]
fn durable_shutdown_reports_delivery_error_and_preserves_startup_failure() {
    for (failure, startup, code) in [(false, false, 0), (true, false, 1), (false, true, 1)] {
        let control = ExitControl::default();
        control.startup_failed.store(startup, Ordering::Release);
        let host = Fake {
            shutdown_error: failure,
            ..Default::default()
        };
        run(&control, &host, false).unwrap();
        assert_eq!(host.code.get(), Some(code));
        assert!(control.finished.load(Ordering::Acquire));
    }
}
#[test]
fn discard_requires_prior_warning() {
    let control = ExitControl::default();
    let host = Fake {
        unsaved: Cell::new(true),
        shutdown_error: true,
        retry_error: true,
        ..Default::default()
    };
    assert!(run(&control, &host, true).is_err());
    assert!(host.unsaved.get());
    assert!(!host.calls.borrow().contains(&"discard"));
}

#[test]
fn dead_presentation_requires_native_warning_then_second_close() {
    let control = ExitControl::default();
    let host = Fake {
        unsaved: Cell::new(true),
        shutdown_error: true,
        retry_error: true,
        ..Default::default()
    };
    // A prior UI-only warning does not authorize a later native discard.
    assert!(run(&control, &host, false).is_err());
    host.delivery_failed.set(true);
    assert!(run(&control, &host, false).is_err());
    assert!(host.unsaved.get());
    assert_eq!(host.calls.borrow().last(), Some(&"native-warn"));
    // Repeated close events cannot discard before the warning can be read.
    assert!(run(&control, &host, false).is_err());
    assert!(!host.calls.borrow().contains(&"discard"));
    *control.native_warning_at.lock().unwrap() = Some(Instant::now() - NATIVE_DISCARD_DELAY);
    host.calls.borrow_mut().clear();
    run(&control, &host, false).unwrap();
    assert_eq!(
        &host.calls.borrow()[..],
        &["updates", "prepare", "retry", "discard"]
    );
    assert!(!host.unsaved.get());
    assert!(control.exiting.load(Ordering::Acquire));
    assert_eq!(host.code.get(), Some(0));
}
#[test]
fn recovered_presentation_does_not_discard_on_second_close() {
    let control = ExitControl::default();
    let host = Fake {
        unsaved: Cell::new(true),
        shutdown_error: true,
        retry_error: true,
        delivery_failed: Cell::new(true),
        ..Default::default()
    };
    assert!(run(&control, &host, false).is_err());
    host.delivery_failed.set(false);
    assert!(run(&control, &host, false).is_err());
    assert!(host.unsaved.get());
    assert!(control.native_warning_at.lock().unwrap().is_none());
}

#[test]
fn native_close_after_gap_saves_without_discard_when_storage_recovers() {
    let control = ExitControl::default();
    let mut host = Fake {
        unsaved: Cell::new(true),
        shutdown_error: true,
        retry_error: true,
        delivery_failed: Cell::new(true),
        ..Default::default()
    };
    assert!(run(&control, &host, false).is_err());
    *control.native_warning_at.lock().unwrap() = Some(Instant::now() - NATIVE_DISCARD_DELAY);
    host.shutdown_error = false;
    host.retry_error = false;
    host.calls.borrow_mut().clear();
    run(&control, &host, false).unwrap();
    assert_eq!(
        &host.calls.borrow()[..],
        &["updates", "prepare", "retry", "shutdown"]
    );
    assert_eq!(host.code.get(), Some(0));
}

#[test]
fn reconnect_clears_native_confirmation_and_restores_unsaved_title() {
    let control = ExitControl::default();
    control.warned.store(true, Ordering::Release);
    *control.native_warning_at.lock().unwrap() = Some(Instant::now() - NATIVE_DISCARD_DELAY);
    assert_eq!(control.presentation_reconnected(), UNSAVED_TITLE);
    assert!(control.native_warning_at.lock().unwrap().is_none());
    control.warned.store(false, Ordering::Release);
    assert_eq!(control.presentation_reconnected(), "Maxed");
}

#[test]
fn early_and_expired_closes_only_renew_warning() {
    for age in [
        Duration::ZERO,
        NATIVE_DISCARD_EXPIRY + Duration::from_secs(1),
    ] {
        let control = ExitControl::default();
        control.warned.store(true, Ordering::Release);
        // On a freshly booted runner, wait until an expired timestamp is
        // representable rather than skipping the expiry assertion.
        let old = loop {
            if let Some(old) = Instant::now().checked_sub(age) {
                break old;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        *control.native_warning_at.lock().unwrap() = Some(old);
        let host = Fake {
            unsaved: Cell::new(true),
            delivery_failed: Cell::new(true),
            shutdown_error: true,
            retry_error: true,
            ..Default::default()
        };
        assert!(run(&control, &host, false).is_err());
        assert_eq!(&host.calls.borrow()[..], &["native-warn"]);
        assert!(control.native_warning_at.lock().unwrap().unwrap() > old);
        assert!(!control.exiting.load(Ordering::Acquire));
        assert!(host.unsaved.get());
        assert_eq!(host.code.get(), None);
    }
}

#[test]
fn native_title_uses_confirmation_delay() {
    assert!(native_warning_title()
        .contains(&format!("Wait {} seconds", NATIVE_DISCARD_DELAY.as_secs())));
}

#[test]
fn successful_retry_never_discards_even_if_shutdown_fails() {
    let control = ExitControl::default();
    control.warned.store(true, Ordering::Release);
    *control.native_warning_at.lock().unwrap() = Some(Instant::now() - NATIVE_DISCARD_DELAY);
    let host = Fake {
        unsaved: Cell::new(true),
        delivery_failed: Cell::new(true),
        shutdown_error: true,
        ..Default::default()
    };
    run(&control, &host, false).unwrap();
    assert_eq!(
        &host.calls.borrow()[..],
        &["updates", "prepare", "retry", "shutdown"]
    );
    assert_eq!(host.code.get(), Some(1));
}

#[test]
fn setup_failure_allows_exit_without_using_incomplete_services() {
    let control = ExitControl::default();
    control.setup_failed();
    assert!(control.finished.load(Ordering::Acquire));
    assert!(control.exiting.load(Ordering::Acquire));
    assert_eq!(control.exit_code(), STARTUP_FAILURE_EXIT_CODE);
    let host = Fake::default();
    assert!(run(&control, &host, false).is_err());
    assert!(host.calls.borrow().is_empty());
    control.setup_failed();
    assert_eq!(control.exit_code(), STARTUP_FAILURE_EXIT_CODE);
}

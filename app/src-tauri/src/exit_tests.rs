use super::*;
use std::cell::{Cell, RefCell};
#[derive(Default)]
struct Fake {
    installing: bool,
    delivery_failed: Cell<bool>,
    update_error: bool,
    unsaved: Cell<bool>,
    shutdown_error: bool,
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
        &["updates", "prepare", "shutdown", "discard"]
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
        delivery_failed: Cell::new(true),
        ..Default::default()
    };
    assert!(run(&control, &host, false).is_err());
    *control.native_warning_at.lock().unwrap() = Some(Instant::now() - NATIVE_DISCARD_DELAY);
    host.shutdown_error = false;
    host.calls.borrow_mut().clear();
    run(&control, &host, false).unwrap();
    assert_eq!(
        &host.calls.borrow()[..],
        &["updates", "prepare", "shutdown"]
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

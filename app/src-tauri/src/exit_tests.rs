use super::*;
use std::cell::{Cell, RefCell};
#[derive(Default)]
struct Fake {
    installing: bool,
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
    fn warn(&self) {
        self.calls.borrow_mut().push("warn");
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

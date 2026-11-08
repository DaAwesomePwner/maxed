//! Shutdown decisions, independent of the window and async executor.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

pub(crate) const NATIVE_DISCARD_DELAY: Duration = Duration::from_secs(2);
pub(crate) const STARTUP_FAILURE_EXIT_CODE: i32 = 1;

pub(crate) struct StartupFailure {
    pub exit_code: i32,
    pub show_dialog: bool,
}

/// Presence alone suppresses a modal, including empty or non-Unicode values.
pub(crate) fn startup_failure(mut env_is_set: impl FnMut(&str) -> bool) -> StartupFailure {
    StartupFailure {
        exit_code: STARTUP_FAILURE_EXIT_CODE,
        show_dialog: ![
            "MAXED_GUI_SMOKE",
            "MAXED_MEASURE_STARTUP",
            "MAXED_MEASURE_DEMO",
        ]
        .into_iter()
        .any(&mut env_is_set),
    }
}

const NATIVE_DISCARD_EXPIRY: Duration = Duration::from_secs(60);
pub(crate) const UNSAVED_TITLE: &str =
    "Maxed — Unsaved changes. Retry saving or confirm exit in Maxed";

pub(crate) fn native_warning_title() -> String {
    format!(
        "Maxed — Unsaved changes. Wait {} seconds, then close again to exit without saving",
        NATIVE_DISCARD_DELAY.as_secs()
    )
}

#[derive(Default)]
pub(crate) struct ExitControl {
    pub exiting: AtomicBool,
    pub finished: AtomicBool,
    pub warned: AtomicBool,
    native_warning_at: Mutex<Option<Instant>>,
    pub startup_failed: AtomicBool,
}
impl ExitControl {
    pub fn clear_native_warning(&self) {
        *self.native_warning_at.lock().unwrap() = None;
    }
    pub fn presentation_reconnected(&self) -> &'static str {
        self.clear_native_warning();
        if self.warned.load(Ordering::Acquire) {
            UNSAVED_TITLE
        } else {
            "Maxed"
        }
    }
    pub fn exit_code(&self) -> i32 {
        if self.startup_failed.load(Ordering::Acquire) {
            STARTUP_FAILURE_EXIT_CODE
        } else {
            0
        }
    }
}

pub(crate) trait ExitHost {
    fn installing(&self) -> bool;
    fn stop_updates(&self) -> Result<(), String>;
    fn prepare(&self);
    fn resume(&self);
    fn unsaved(&self) -> bool;
    fn delivery_failed(&self) -> bool;
    fn retry_save(&self) -> Result<(), String>;
    fn shutdown(&self) -> Result<(), String>;
    fn discard(&self);
    fn warn(&self, native_fallback: bool);
    fn exit(&self, code: i32);
}

pub(crate) fn run(
    control: &ExitControl,
    host: &impl ExitHost,
    discard: bool,
) -> Result<(), String> {
    if control.exiting.swap(true, Ordering::AcqRel) {
        return Err("Exit is already in progress.".into());
    }
    let result = attempt(control, host, discard);
    if !control.finished.load(Ordering::Acquire) {
        control.exiting.store(false, Ordering::Release);
    }
    result
}

fn attempt(control: &ExitControl, host: &impl ExitHost, discard: bool) -> Result<(), String> {
    if host.installing() {
        return Err("Update operation is busy.".into());
    }
    // Check the gap at entry, before a potentially slow save retry. Rapid
    // repeated close events must not become discard consent while waiting.
    let warning_at = *control.native_warning_at.lock().unwrap();
    let native_pending = !discard
        && host.delivery_failed()
        && control.warned.load(Ordering::Acquire)
        && host.unsaved();
    let native_discard = native_pending
        && warning_at.is_some_and(|at| {
            (NATIVE_DISCARD_DELAY..=NATIVE_DISCARD_EXPIRY).contains(&at.elapsed())
        });
    if native_pending && warning_at.is_some() && !native_discard {
        // Auto-repeat and stale confirmation only refresh the warning. Do not
        // stop updates, prepare setup, or stop/resume capture on these closes.
        return warn_pending(control, host);
    }
    host.stop_updates()?;
    host.prepare();
    // shutdown alone cannot retry a writer retained in recovery.
    let retry_failed = native_discard && host.retry_save().is_err();
    let native_discard_allowed = retry_failed
        && host.delivery_failed()
        && *control.native_warning_at.lock().unwrap() == warning_at;
    let code = if (discard || native_discard_allowed)
        && control.warned.load(Ordering::Acquire)
        && host.unsaved()
    {
        host.discard();
        control.exit_code()
    } else if host.shutdown().is_ok() {
        control.exit_code()
    } else if !host.unsaved() {
        1
    } else {
        host.resume();
        return warn_pending(control, host);
    };
    control.finished.store(true, Ordering::Release);
    host.exit(code);
    Ok(())
}

fn warn_pending(control: &ExitControl, host: &impl ExitHost) -> Result<(), String> {
    control.warned.store(true, Ordering::Release);
    let native_fallback = host.delivery_failed();
    host.warn(native_fallback);
    // Start (or restart) the reading interval after issuing the warning.
    *control.native_warning_at.lock().unwrap() = native_fallback.then(Instant::now);
    Err("Changes are not saved; retry storage or exit without saving".into())
}

#[cfg(test)]
#[path = "exit_tests.rs"]
mod tests;

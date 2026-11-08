//! Shutdown decisions, independent of the window and async executor.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

const NATIVE_DISCARD_DELAY: Duration = Duration::from_secs(2);
pub(crate) const UNSAVED_TITLE: &str =
    "Maxed — Unsaved changes. Retry saving or confirm exit in Maxed";

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
        i32::from(self.startup_failed.load(Ordering::Acquire))
    }
}

pub(crate) trait ExitHost {
    fn installing(&self) -> bool;
    fn stop_updates(&self) -> Result<(), String>;
    fn prepare(&self);
    fn resume(&self);
    fn unsaved(&self) -> bool;
    fn delivery_failed(&self) -> bool;
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
    let native_discard =
        host.delivery_failed() && warning_at.is_some_and(|at| at.elapsed() >= NATIVE_DISCARD_DELAY);
    host.stop_updates()?;
    host.prepare();
    let code = if discard && control.warned.load(Ordering::Acquire) && host.unsaved() {
        host.discard();
        control.exit_code()
    } else if host.shutdown().is_ok() {
        control.exit_code()
    } else if native_discard
        && control.warned.load(Ordering::Acquire)
        && host.unsaved()
        && host.delivery_failed()
        && *control.native_warning_at.lock().unwrap() == warning_at
    {
        host.discard();
        control.exit_code()
    } else if !host.unsaved() {
        1
    } else {
        host.resume();
        control.warned.store(true, Ordering::Release);
        let native_fallback = host.delivery_failed();
        host.warn(native_fallback);
        // Start (or restart) the reading interval after issuing the warning.
        *control.native_warning_at.lock().unwrap() = native_fallback.then(Instant::now);
        return Err("Changes are not saved; retry storage or exit without saving".into());
    };
    control.finished.store(true, Ordering::Release);
    host.exit(code);
    Ok(())
}

#[cfg(test)]
#[path = "exit_tests.rs"]
mod tests;

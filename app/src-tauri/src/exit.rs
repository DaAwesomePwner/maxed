//! Shutdown decisions, independent of the window and async executor.
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(crate) struct ExitControl {
    pub exiting: AtomicBool,
    pub finished: AtomicBool,
    pub warned: AtomicBool,
    pub native_warned: AtomicBool,
    pub startup_failed: AtomicBool,
}
impl ExitControl {
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
    host.stop_updates()?;
    host.prepare();
    let native_discard = host.delivery_failed() && control.native_warned.load(Ordering::Acquire);
    let code = if (discard || native_discard)
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
        control.warned.store(true, Ordering::Release);
        let native_fallback = host.delivery_failed();
        control
            .native_warned
            .store(native_fallback, Ordering::Release);
        host.warn(native_fallback);
        return Err("Changes are not saved; retry storage or exit without saving".into());
    };
    control.finished.store(true, Ordering::Release);
    host.exit(code);
    Ok(())
}

#[cfg(test)]
#[path = "exit_tests.rs"]
mod tests;

//! One opt-in native receipt; the caller never supplies timing or output text.
use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct InteractiveRail {
    enabled: bool,
    received: AtomicBool,
}
impl InteractiveRail {
    pub fn new(value: Option<&str>) -> Self {
        Self {
            enabled: value == Some("1"),
            received: AtomicBool::new(false),
        }
    }
    pub fn report(&self, main_window: bool, mut output: impl Write) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if !main_window {
            return Err("Unsupported startup receipt.".into());
        }
        if self.received.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        output
            .write_all(b"MAXED_INTERACTIVE_RAIL_V1\n")
            .and_then(|_| output.flush())
            .map_err(|_| "Startup receipt could not be written.".into())
    }
}

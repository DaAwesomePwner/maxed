#![deny(unsafe_code)]

mod error;
pub use error::CaptureError;
mod parse;
pub use parse::{parse_frame, ParseError};
mod queue;
pub use queue::send_latest;
#[allow(unsafe_code)]
mod ffi;
mod live;
pub use live::{list_adapters, start_capture, AdapterInfo, CaptureConfig, CaptureHandle};

/// Injectable safe backend boundary, also used by the production worker.
#[doc(hidden)]
pub mod backend {
    pub use crate::live::{start_with_backend, Backend, Device};
}

pub fn probe_npcap() -> Result<(), CaptureError> {
    ffi::Api::load().map(|_| ())
}

pub fn npcap_candidates(root: &std::path::Path) -> [std::path::PathBuf; 2] {
    [
        root.join("System32").join("Npcap").join("wpcap.dll"),
        "wpcap.dll".into(),
    ]
}

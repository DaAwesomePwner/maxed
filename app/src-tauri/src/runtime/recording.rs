use super::CaptureRuntime;
use std::path::PathBuf;

impl CaptureRuntime {
    /// Native setup supplies the resolved per-user app-data root. No IPC path.
    pub fn configure_recording(
        &self,
        app_data: PathBuf,
        enabled: bool,
        limit_bytes: u32,
        live: bool,
    ) -> Result<(), String> {
        self.shared.recording.configure(app_data.join("recordings"), enabled, limit_bytes, live)
            .map_err(|_| "Recording could not start. Check recording storage and retry; capture and summaries remain available.".into())
    }
}

impl CaptureRuntime {
    /// Native filesystem injection seam; unavailable through the WebView bridge.
    #[doc(hidden)]
    pub fn configure_recording_with_disk(
        &self,
        app_data: PathBuf,
        limit_bytes: u32,
        disk: std::sync::Arc<dyn maxed_recording::RecordingDisk>,
    ) -> Result<(), crate::recording::RecordingError> {
        self.shared.recording.configure_with_disk(
            app_data.join("recordings"),
            true,
            limit_bytes,
            true,
            disk,
        )
    }
}

#[cfg(test)]
#[path = "../../tests/support/recording_playback.rs"]
mod tests;

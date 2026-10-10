#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("Npcap is not installed")]
    Missing,
    #[error("Npcap is incompatible; repair or update Npcap")]
    Incompatible,
    #[error("Capture permission denied; check Npcap access settings")]
    PermissionDenied,
    #[error("No eligible capture adapters found")]
    NoAdapters,
    #[error("Capture adapter could not be opened or read; retry capture")]
    OpenFailed,
    #[error("Capture stopped")]
    Stopped,
    #[error("Invalid capture configuration")]
    InvalidConfig,
    #[error("Live capture requires Windows")]
    UnsupportedPlatform,
}

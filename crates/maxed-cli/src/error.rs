#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("catalog-root v1 is Windows-only")]
    CatalogRootWindowsOnly,
    #[error(transparent)]
    Domain(#[from] crate::domain_compare::DomainError),
    #[error(transparent)]
    Capture(#[from] maxed_capture::CaptureError),
    #[error(transparent)]
    Parse(#[from] maxed_capture::ParseError),
    #[error(transparent)]
    Recording(#[from] maxed_recording::RecordingError),
    #[error(transparent)]
    Protocol(#[from] maxed_protocol::ProtocolError),
    #[error("File I/O failed")]
    Io(#[from] std::io::Error),
    #[error("Could not install Ctrl+C handler")]
    SignalHandler(#[source] std::io::Error),
    #[error("Capture frame loss interrupted recording; any complete prefix was retained")]
    FrameLoss,
    #[error("Invalid arguments or output location; use a private recordings directory")]
    InvalidArguments,
}

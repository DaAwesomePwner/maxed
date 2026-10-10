use maxed_types::EventEnvelope;

/// Messages deliberately omit paths, SQLite text, and domain values.
#[derive(Clone, Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Could not open summary storage")]
    Open,
    #[error("Summary schema validation or migration failed")]
    Migration,
    #[error("Summary storage was created by a newer version")]
    NewerSchema,
    #[error("Summary storage is busy; pending work is not saved")]
    Busy,
    #[error("Summary storage is full; pending work is not saved")]
    Full,
    #[error("Summary storage I/O failed; pending work is not saved")]
    Io,
    #[error("Invalid summary event or query")]
    InvalidEvent,
    #[error("Saved history exceeds the supported query size limit")]
    Limit,
    #[error("Summary event sequence has a gap")]
    SequenceGap,
    #[error("Summary writer is closed")]
    Closed,
    #[error("Summary worker failed; retained events are not saved")]
    WorkerFailed {
        /// Shutdown consumes its writer. On failure, ownership of every accepted
        /// but uncommitted event travels in this error, so callers can retain and
        /// resubmit them to a restarted writer. Never discard this error.
        pending: Vec<EventEnvelope>,
    },
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode;
        match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Self::Busy,
            Some(ErrorCode::DiskFull) => Self::Full,
            Some(ErrorCode::ConstraintViolation | ErrorCode::TypeMismatch) => Self::InvalidEvent,
            _ => Self::Io,
        }
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidEvent
    }
}

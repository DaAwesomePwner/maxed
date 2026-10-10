use thiserror::Error;

/// Deliberately excludes input bytes and private filesystem paths.
#[derive(Debug, Error)]
pub enum AssetError {
    #[error("asset input/output failed")]
    Io,
    #[error("unsupported Unity version")]
    UnsupportedVersion,
    #[error("invalid asset structure")]
    InvalidStructure,
    #[error("asset exceeds the configured extraction limit")]
    LimitExceeded,
    #[error("unsupported texture format or layout")]
    UnsupportedTexture,
    #[error("invalid catalog XML")]
    InvalidXml,
    #[error("invalid XML in catalog section {section}")]
    InvalidXmlSection { section: &'static str },
    #[error("invalid catalog")]
    InvalidCatalog,
    #[error("invalid asset cache")]
    CacheInvalid,
    #[error("asset extraction cancelled")]
    Cancelled,
}

impl From<std::io::Error> for AssetError {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Self::InvalidStructure
        } else {
            Self::Io
        }
    }
}

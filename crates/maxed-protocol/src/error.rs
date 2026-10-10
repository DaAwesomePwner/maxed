/// Safe, metadata-only protocol failures. Never include wire contents in errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("Truncated protocol field")]
    Truncated,
    #[error("Invalid protocol length")]
    Length,
    #[error("Compressed integer overflow")]
    CompressedOverflow,
    #[error("Invalid UTF-8 field")]
    Utf8,
    #[error("Nonfinite protocol number")]
    NonFinite,
    #[error("Invalid packet framing")]
    Framing,
    #[error("Invalid protocol definition")]
    Definition,
}

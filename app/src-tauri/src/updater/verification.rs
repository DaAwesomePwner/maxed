//! Tauri updater 2.13.1 encoding and verification order, using its exact verifier.
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use semver::Version;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerificationError {
    #[error("Update verification key is unresolved or invalid.")]
    Key,
    #[error("Update signature is missing or invalid.")]
    Signature,
    #[error("Update signed version is missing or does not match.")]
    Version,
}

pub fn public_key(encoded: &str) -> Result<PublicKey, VerificationError> {
    if encoded.len() > 4096 {
        return Err(VerificationError::Key);
    }
    let text = STANDARD
        .decode(encoded.trim())
        .map_err(|_| VerificationError::Key)?;
    let text = std::str::from_utf8(&text).map_err(|_| VerificationError::Key)?;
    PublicKey::decode(text).map_err(|_| VerificationError::Key)
}

pub fn verify(
    bytes: &[u8],
    encoded_signature: &str,
    key: &PublicKey,
    version: &Version,
) -> Result<(), VerificationError> {
    if encoded_signature.len() > 8192 {
        return Err(VerificationError::Signature);
    }
    let text = STANDARD
        .decode(encoded_signature.trim())
        .map_err(|_| VerificationError::Signature)?;
    let text = std::str::from_utf8(&text).map_err(|_| VerificationError::Signature)?;
    let signature = Signature::decode(text).map_err(|_| VerificationError::Signature)?;
    // Matches Tauri: verify both artifact and global signature BEFORE using the comment.
    key.verify(bytes, &signature, true)
        .map_err(|_| VerificationError::Signature)?;
    let versions: Vec<_> = signature
        .trusted_comment()
        .split('\t')
        .filter_map(|field| field.strip_prefix("version:"))
        .collect();
    // Require one authenticated version. Unlike Tauri's optional legacy setting,
    // Maxed never accepts a signature without a version or ambiguous duplicate fields.
    if versions.len() != 1
        || !Version::parse(versions[0].trim_start_matches('v'))
            .is_ok_and(|signed| &signed == version)
    {
        return Err(VerificationError::Version);
    }
    Ok(())
}

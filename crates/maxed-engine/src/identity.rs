use hmac::{Hmac, Mac};
use sha2::Sha256;

pub(crate) fn account(key: &[u8; 32], value: &str) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(b"maxed/account/v1\0");
    mac.update(value.as_bytes());
    mac.finalize().into_bytes().into()
}

pub(crate) fn character(key: &[u8; 32], account: &[u8; 32], number: i32) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(b"maxed/character/v1\0");
    mac.update(account);
    mac.update(&number.to_be_bytes());
    format!(
        "character-{}",
        maxed_protocol::lower_hex(&mac.finalize().into_bytes())
    )
}

pub(crate) fn account_id(hash: &[u8; 32]) -> String {
    format!("account-{}", maxed_protocol::lower_hex(hash))
}

/// Recompute a character ID using a local account pseudonym and signed CREATE_SUCCESS ID.
/// Accepts only the canonical lowercase account-<64 hex> representation.
pub fn character_id_for(key: &[u8; 32], account_id: &str, number: i32) -> Option<String> {
    let hex = account_id.strip_prefix("account-")?;
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut hash = [0; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(character(key, &hash, number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_scope_helper_matches_identity_kernel() {
        let key = [7; 32];
        let hash = account(&key, "synthetic-account");
        let account_id = account_id(&hash);
        for number in [i32::MIN, -1, 0, 1, 65_535, i32::MAX] {
            assert_eq!(
                character_id_for(&key, &account_id, number),
                Some(character(&key, &hash, number))
            );
        }
        for invalid in [
            format!("account-{}", "g".repeat(64)),
            format!("account-{}", "é".repeat(32)),
            account_id.to_uppercase(),
        ] {
            assert_eq!(character_id_for(&key, &invalid, 1), None);
        }
    }
}

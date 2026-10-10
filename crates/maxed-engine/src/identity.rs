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

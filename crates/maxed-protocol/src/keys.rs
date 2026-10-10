//! Public protocol constants from RealmShark RotMGRC4Keys (see NOTICE.md).
use crate::ProtocolError;
use maxed_types::Direction;

pub const INCOMING_HEX: &str = "c91d9eec420160730d825604e0";
pub const OUTGOING_HEX: &str = "5a4d2016bc16dc64883194ffd9";

pub fn key_bytes(dir: Direction) -> Result<Vec<u8>, ProtocolError> {
    let hex = match dir {
        Direction::ClientToServer => OUTGOING_HEX,
        Direction::ServerToClient => INCOMING_HEX,
    };
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(ProtocolError::Definition);
    }
    pairs
        .iter()
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| ProtocolError::Definition)?;
            u8::from_str_radix(pair, 16).map_err(|_| ProtocolError::Definition)
        })
        .collect()
}

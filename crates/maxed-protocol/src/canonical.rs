use crate::{DecodedPacket, ProtocolError};
use maxed_types::Direction;
use serde::{Deserialize, Serialize, Serializer};
use std::io::Write;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalLine {
    pub i: u64,
    pub t: u64,
    pub dir: String,
    pub id: u8,
    pub name: String,
    pub f: serde_json::Value,
}

pub fn canonical_line(i: u64, packet: &DecodedPacket) -> Result<CanonicalLine, ProtocolError> {
    Ok(CanonicalLine {
        i,
        t: packet.at.0,
        dir: match packet.dir {
            Direction::ClientToServer => "c2s",
            Direction::ServerToClient => "s2c",
        }
        .into(),
        id: packet.id,
        name: packet.packet.name().into(),
        f: packet.packet.fields()?,
    })
}

pub fn write_jsonl<W: Write>(writer: &mut W, line: &CanonicalLine) -> Result<(), std::io::Error> {
    serde_json::to_writer(&mut *writer, line).map_err(std::io::Error::other)?;
    writer.write_all(b"\n")
}

pub fn six_significant(value: f32) -> Result<f64, ProtocolError> {
    if !value.is_finite() {
        return Err(ProtocolError::NonFinite);
    }
    if value == 0.0 {
        return Ok(0.0);
    }
    format!("{:.5e}", f64::from(value))
        .parse()
        .map_err(|_| ProtocolError::Definition)
}

pub(crate) fn float<S: Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(six_significant(*value).map_err(serde::ser::Error::custom)?)
}

pub fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}

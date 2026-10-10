#![forbid(unsafe_code)]

mod error;
mod framing;
pub mod keys;
mod rc4;
mod reader;

pub use error::ProtocolError;
pub use framing::{framed_len, Framer, FramerOutput, MAX_FRAME};
pub use keys::key_bytes;
pub use rc4::Rc4;
pub use reader::Reader;
pub mod canonical;
mod decoder;
pub mod defs;
pub mod direction;
pub mod explanation;
pub use canonical::{canonical_line, lower_hex, six_significant, write_jsonl, CanonicalLine};
pub use decoder::{DecodeEvent, DecodedPacket, Diagnostic, PacketDiagnostic};
pub use defs::{decode_packet, Packet};
pub mod alignment;
pub use alignment::{find_tick_offset, Alignment, SEARCH_SIZE};
pub use decoder::Decoder;

pub mod notices;
pub mod privacy;

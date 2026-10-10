#![forbid(unsafe_code)]

pub mod catalog_root;
pub mod decode;
pub mod demo;
pub mod domain_compare;
pub mod domain_export;
mod error;
pub mod parity;
pub mod pipeline;
pub mod record;
pub mod replay;
pub use decode::decode;
pub use error::CliError;
pub use parity::{parity, Diff, ParityReport};
pub use pipeline::Pipeline;
pub use record::record;
pub use replay::{replay, ReplayReport};

pub mod scrub;

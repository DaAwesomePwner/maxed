//! Durable summary projections. No packet payloads or raw-hit history are stored.
//!
//! `try_submit` accepts bounded, volatile work. Only a flush receipt or the durable
//! status watermark acknowledges a commit. Call flush/retry/shutdown off the UI
//! thread. A process crash can lose all accepted but uncommitted events.

#![forbid(unsafe_code)]

mod alerts;
mod chat;
mod error;
mod home;
mod instance;
mod loot;
mod migrate;
mod project;
mod queries;
mod run_rows;
mod run_screen;
mod run_strip;
mod stats;
mod visits;
mod writer;

#[cfg(test)]
#[path = "../tests/common/runs_workload.rs"]
mod runs_workload;

#[cfg(test)]
#[path = "../tests/common/alerts_workload.rs"]
mod alerts_workload;

pub use alerts::{
    AlertContext, ChatContext, CommittedAlert, ContextualEvent, ContextualShutdownError,
};
pub use chat::{
    event_fts_probe_for_tests, event_page_sql_for_tests, maintain_chat_file, ChatMaintenance,
    HiddenLists, CHAT_CLEAR_BATCH, CHAT_PRUNE_BATCH,
};
pub use error::StoreError;
pub use migrate::{migrate, open_reader, upgrade_history, StoreReader};
pub use project::{apply_batch, apply_contexts_for_tests};
pub use queries::{clear_comparison, validate_scope_loot};
pub use stats::SessionRunParty;
pub use writer::{
    start_writer, take_abandoned_events, FlushReceipt, StoreWriter, SubmitError, WriterConfig,
};

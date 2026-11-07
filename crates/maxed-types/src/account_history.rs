use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccountHistoryChangeReason {
    AccountSwitched,
    LegacyRecovered,
}

/// Privacy-safe invalidation notice. No identity or installation secrets.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct AccountHistoryChanged {
    #[ts(type = "number")]
    pub revision: u64,
    pub reason: AccountHistoryChangeReason,
}

//! Chat and instance-event history for the Logs tab. Chat is stored locally;
//! see PRIVACY.md. Matching on hide lists folds ASCII letters only, the same
//! folding SQLite's built-in lower() and NOCASE use.
use crate::{InstanceEventView, TimeRange, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_CHAT_TEXT_BYTES: usize = 1024;
pub const MAX_LOGS_PAGE: u16 = 100;
pub const MAX_WHISPER_PARTNERS: u16 = 200;
pub const MAX_CHAT_QUERY_CHARS: usize = 128;
pub const MAX_HIDDEN_SENDERS: usize = 256;
pub const MAX_HIDDEN_SENDER_BYTES: usize = 32;
pub const MAX_HIDDEN_KEYWORDS: usize = 128;
pub const MAX_HIDDEN_KEYWORD_BYTES: usize = 64;
pub const DEFAULT_CHAT_RETENTION_DAYS: u16 = 90;
pub const MAX_CHAT_RETENTION_DAYS: u16 = 3650;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel {
    Public,
    Guild,
    Party,
    Whisper,
    Unknown,
}
impl ChatChannel {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Guild => "guild",
            Self::Party => "party",
            Self::Whisper => "whisper",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WhisperDirection {
    Incoming,
    Outgoing,
    Unknown,
}
impl WhisperDirection {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatSenderKind {
    Player,
    Npc,
    System,
}
impl ChatSenderKind {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Npc => "npc",
            Self::System => "system",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatMessageView {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run_id: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub map: Option<String>,
    pub at: Timestamp,
    pub channel: ChatChannel,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub direction: Option<WhisperDirection>,
    pub sender_kind: ChatSenderKind,
    pub sender: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub counterpart: Option<String>,
    pub is_local: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub stars: Option<u8>,
    pub supporter: bool,
    pub text: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChatStorage {
    pub public: bool,
    pub guild: bool,
    pub party: bool,
    pub whisper: bool,
}
impl Default for ChatStorage {
    fn default() -> Self {
        Self {
            public: true,
            guild: true,
            party: true,
            whisper: true,
        }
    }
}
impl ChatStorage {
    /// Unknown-channel lines are kept whenever any channel is kept.
    pub fn stores(&self, channel: ChatChannel) -> bool {
        match channel {
            ChatChannel::Public => self.public,
            ChatChannel::Guild => self.guild,
            ChatChannel::Party => self.party,
            ChatChannel::Whisper => self.whisper,
            ChatChannel::Unknown => self.public || self.guild || self.party || self.whisper,
        }
    }
}

fn default_retention() -> u16 {
    DEFAULT_CHAT_RETENTION_DAYS
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChatSettings {
    #[serde(default)]
    pub store: ChatStorage,
    #[serde(default = "default_retention")]
    pub retention_days: u16,
    #[serde(default)]
    pub hidden_senders: Vec<String>,
    #[serde(default)]
    pub hidden_keywords: Vec<String>,
}
impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            store: ChatStorage::default(),
            retention_days: DEFAULT_CHAT_RETENTION_DAYS,
            hidden_senders: vec![],
            hidden_keywords: vec![],
        }
    }
}
impl ChatSettings {
    pub fn valid(&self) -> bool {
        let entry = |value: &String, max: usize| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        };
        (1..=MAX_CHAT_RETENTION_DAYS).contains(&self.retention_days)
            && self.hidden_senders.len() <= MAX_HIDDEN_SENDERS
            && self
                .hidden_senders
                .iter()
                .all(|s| entry(s, MAX_HIDDEN_SENDER_BYTES))
            && self.hidden_keywords.len() <= MAX_HIDDEN_KEYWORDS
            && self
                .hidden_keywords
                .iter()
                .all(|k| entry(k, MAX_HIDDEN_KEYWORD_BYTES))
    }
    /// Same ASCII folding as the reader's SQL (`lower()` / `NOCASE`).
    pub fn hides(&self, message: &ChatMessageView) -> bool {
        self.hidden_senders
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&message.sender))
            || {
                let text = message.text.to_ascii_lowercase();
                self.hidden_keywords
                    .iter()
                    .any(|k| text.contains(&k.to_ascii_lowercase()))
            }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChatCursor {
    pub at: Timestamp,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChatFilter {
    /// None reads every channel (Chat › All).
    pub channel: Option<ChatChannel>,
    /// Exact sender or whisper counterpart, ASCII case-insensitive.
    pub sender: Option<String>,
    pub query: String,
    pub run_id: Option<String>,
    pub show_npc: bool,
    pub show_hidden: bool,
    pub cursor: Option<ChatCursor>,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatRow {
    pub message: ChatMessageView,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatPage {
    pub rows: Vec<ChatRow>,
    pub next: Option<ChatCursor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatOverviewRequest {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChannelCounts {
    pub public: u32,
    pub guild: u32,
    pub party: u32,
    pub whisper: u32,
    pub unknown: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatOverview {
    pub lines: u32,
    pub by_channel: ChannelCounts,
    pub first_at: Option<Timestamp>,
    pub latest_at: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatSenderRequest {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatSenderSummary {
    pub name: String,
    pub lines: u32,
    pub first_at: Timestamp,
    pub last_at: Timestamp,
    pub channels: Vec<ChatChannel>,
    pub stars: Option<u8>,
    pub supporter: bool,
    pub whispers: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WhisperPartnerFilter {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub show_hidden: bool,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WhisperPartner {
    pub name: String,
    pub lines: u32,
    pub last_at: Timestamp,
    pub last_text: String,
    pub last_direction: WhisperDirection,
    pub stars: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WhisperPartners {
    pub rows: Vec<WhisperPartner>,
    pub people: u32,
    pub lines: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum InstanceEventTag {
    PlayerDeath,
    PlayerLeft,
    PortalOpened,
    RealmEventKilled,
}
impl InstanceEventTag {
    pub const ALL: [Self; 4] = [
        Self::PlayerDeath,
        Self::PlayerLeft,
        Self::PortalOpened,
        Self::RealmEventKilled,
    ];
    /// The `instance_events.kind` column value written by `instance::project`.
    pub fn tag(self) -> &'static str {
        match self {
            Self::PlayerDeath => "player_death",
            Self::PlayerLeft => "player_left",
            Self::PortalOpened => "portal_opened",
            Self::RealmEventKilled => "realm_event_killed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct EventCursor {
    pub at: Timestamp,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct EventFilter {
    /// Empty reads all four kinds.
    pub kinds: Vec<InstanceEventTag>,
    pub run_id: Option<String>,
    /// Portal opened events only; matches the stored dungeon text.
    pub dungeon: Option<String>,
    pub cursor: Option<EventCursor>,
    pub limit: u16,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub range: Option<TimeRange>,
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    /// Token-prefix search across player, opener, killer and boss names (like chat).
    pub query: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub opener_local: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventPage {
    #[serde(default)]
    #[ts(as = "Option<Vec<EnteredRun>>", optional)]
    pub entered_runs: Vec<EnteredRun>,
    pub rows: Vec<InstanceEventView>,
    pub next: Option<EventCursor>,
}

/// A matched run; portals whose dungeon text is a catalog alias don't match a run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EnteredRun {
    pub event_id: String,
    pub run_id: String,
    pub dungeon: String,
    pub started_at: Timestamp,
    pub outcome: Option<String>,
}

/// Summary scope uses range + run_id only, ignoring query and page filters.
/// flagged_since remains global.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventSummaryRequest {
    pub filter: EventFilter,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KindCounts {
    pub player_death: u32,
    pub player_left: u32,
    pub portal_opened: u32,
    pub realm_event_killed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DungeonPops {
    pub dungeon: String,
    pub pops: u32,
    pub last_at: Timestamp,
}

/// Counts, dungeon/local aggregates, total and first_at use range + run_id only.
/// Query and page filters are ignored; flagged_since remains global.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EventSummary {
    pub total: u32,
    pub by_kind: KindCounts,
    pub dungeons: Vec<DungeonPops>,
    pub local_pops: u32,
    pub last_local_pop: Option<DungeonPops>,
    // Earliest flagged portal globally, independent of the summary scope.
    pub flagged_since: Option<Timestamp>,
    pub first_at: Option<Timestamp>,
}

//! Domain contracts. Unknown values remain explicit nulls.

use crate::{CaptureStatus, RunTotals, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Cleared,
    Died,
    Left,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    Victory,
    CompletionDialogue {
        rule: String,
    },
    CompletionCounter {
        before: i32,
        after: i32,
        delay_micros: u32,
    },
    DeathPacket,
    ObservedExit,
    CaptureGap,
    AppClosed,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EvidenceAt {
    pub at: Timestamp,
    pub evidence: Evidence,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CoverageIssue {
    Midstream,
    DecodeFailure,
    StreamGap,
    MissingIdentity,
    MissingCatalog,
    MissingStats,
    MissingProjectile,
    AmbiguousHit,
    ResourceLimit,
    ClockRegression,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AreaKind {
    Nexus,
    Realm,
    Dungeon,
    OtherHub,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Weapon,
    Ability,
    Summon,
    ItemEffect,
    Other,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EnchantState {
    Missing,
    Invalid,
    RecordedEmpty,
    Recorded,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EnchantView {
    pub state: EnchantState,
    pub slots: Option<u8>,
    pub applied: Option<u8>,
    pub ids: Vec<i32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EquipmentView {
    // True only for wire -1, false for a valid item type; absent/invalid stays unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub observed_empty: Option<bool>,
    pub slot: u8,
    pub item_type: Option<u16>,
    pub enchant: EnchantView,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatView {
    pub stat_id: u8,
    pub value: i32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CharacterView {
    // Local account HMAC formatted as account-<64 lowercase hex>; never raw stat 38.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub account_id: Option<String>,
    // Presentation-only; engine snapshots and durable events leave this unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub calculation_revision: Option<CharacterInputRevision>,
    pub id: String,
    pub name: Option<String>,
    pub class_id: Option<u16>,
    pub level: Option<u8>,
    pub maxed: Option<u8>,
    pub fame: Option<i32>,
    pub stats: Vec<StatView>,
    pub equipment: Vec<EquipmentView>,
    pub observed_at: Timestamp,
    pub identity_verified: bool,
    pub died_at: Option<Timestamp>,
    pub issues: Vec<CoverageIssue>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub stat_panel: Option<StatPanel>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SourceDamage {
    pub item_type: Option<u16>,
    pub ability: bool,
    pub kind: SourceKind,
    pub enchant: EnchantView,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub damage: f64,
    pub hits: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub biggest_hit: Option<f64>,
    pub estimated: bool,
    pub reasons: Vec<String>,
}
/// Attribution-only inference preserves the observed damage amount.
pub const INFERRED_EQUIPPED_WEAPON: &str = "inferred-equipped-weapon";

impl SourceDamage {
    /// Whether evidence makes the damage amount uncertain, rather than just its label.
    pub fn damage_uncertain(&self) -> bool {
        self.reasons
            .iter()
            .any(|reason| reason != INFERRED_EQUIPPED_WEAPON)
            || (self.estimated && self.reasons.is_empty())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlayerDamage {
    pub object_id: i32,
    pub player_id: String,
    pub name: Option<String>,
    pub class_id: Option<u16>,
    pub is_local: bool,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub damage: f64,
    pub hits: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub biggest_hit: Option<f64>,
    pub deaths: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub dps: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub share: Option<f64>,
    pub rank: Option<u32>,
    pub sources: Vec<SourceDamage>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FightEnd {
    MapChanged,
    CaptureStopped,
    SessionIdle,
    AppClosed,
    ResourceLimit,
    Killed,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DamageBucket {
    pub from: Timestamp,
    pub until: Timestamp,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub total: f64,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub local: Option<f64>,
    #[serde(default, with = "crate::finite::optional_f64")]
    #[ts(optional = nullable, as = "Option<f64>")]
    pub others_damage: Option<f64>,
    // Non-local player-seconds; sum alongside damage when buckets merge.
    #[serde(default, with = "crate::finite::optional_f64")]
    #[ts(optional = nullable, as = "Option<f64>")]
    pub others_presence: Option<f64>,
}
/// Version of the per-player DPS formula persisted with a fight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DpsBasis {
    #[default]
    WholeTargetV0,
    PlayerActiveWindowV1,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct FightView {
    pub id: String,
    /// Realm targets may have no enclosing dungeon run.
    pub run_id: Option<String>,
    pub session_id: String,
    pub target_id: i32,
    pub target_generation: u32,
    /// Set only when the catalog carries the BOSS tag.
    pub boss_type: Option<u16>,
    pub target_type: Option<u16>,
    pub max_hp: Option<i32>,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
    pub first_hit: Option<Timestamp>,
    pub last_hit: Option<Timestamp>,
    pub end_reason: Option<FightEnd>,
    pub players: Vec<PlayerDamage>,
    #[serde(default)]
    pub dps_basis: DpsBasis,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub total_damage: f64,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub unattributed_damage: f64,
    pub buckets: Vec<DamageBucket>,
    pub complete_capture: bool,
    // Local hit reports still awaiting their counterpart or pair expiry.
    #[serde(default)]
    pub pending_local_reconciliation: bool,
    pub issues: Vec<CoverageIssue>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub kill: Option<KillView>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub target_hp: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub target_kind: Option<TargetKind>,
    // Live quest arrow state; historical target_kind is retained separately.
    #[serde(default)]
    pub is_current_quest: bool,
    // Latest observed local contribution, including pending estimates. The UI
    // must still verify the local player's identity before using it for focus.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub local_last_hit: Option<Timestamp>,
    // Local player's first-to-last hit span, clipped at fight end, without a
    // minimum duration. None for legacy fights or an unknown/ambiguous local.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable, as = "Option<f64>")]
    pub local_active_micros: Option<u64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    // snapshot series bounded to 16 prioritized fights; events carry full series
    pub player_series: Option<Vec<PlayerSeries>>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub scope_id: Option<String>,
    // Character at creation, unchanged by revisions. Resolve provisional IDs via CharacterMerged aliases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub owner_character_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DropItem {
    pub slot: u8,
    pub item_type: u16,
    pub quantity: u32,
    pub enchant: EnchantView,
    // First observed removal/replacement of this (bag, slot, item type).
    // Retained on reappearance; does not identify a taker or ownership.
    #[serde(default)]
    pub taken_observed_at: Option<Timestamp>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DropView {
    pub id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub at: Timestamp,
    pub bag_type: u16,
    /// Ordered projection of contents, including repeated item types.
    pub items: Vec<u16>,
    /// Derived label when exactly one captured item has a valid summary; never raw stats.
    pub enchant: Option<String>,
    // Retained appearances, unique by (slot, item_type), including taken items.
    pub contents: Vec<DropItem>,
    pub source_enemy: Option<u16>,
    pub source_estimated: bool,
    pub source_reason: Option<String>,
    /// False distinguishes unseen contents from an observed empty bag.
    pub contents_known: bool,
    pub bag_rank: Option<u8>,
    pub issues: Vec<CoverageIssue>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub capture_scope: Option<CaptureScope>,
    // Character at creation, unchanged by revisions. Resolve provisional IDs via CharacterMerged aliases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub owner_character_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HpSample {
    pub at: Timestamp,
    pub hp: i32,
    pub max_hp: Option<i32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DeathDetail {
    pub killed_by: Option<String>,
    pub killed_by_type: Option<u16>,
    pub fame: Option<i32>,
    pub hp_trace: Vec<HpSample>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunView {
    pub id: String,
    pub session_id: String,
    pub character_id: Option<String>,
    pub dungeon: String,
    pub area_kind: AreaKind,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    pub died_at: Option<Timestamp>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub death: Option<DeathDetail>,
    pub outcome: Option<Outcome>,
    pub evidence: Vec<EvidenceAt>,
    pub party_size: Option<u32>,
    pub party_size_estimated: bool,
    pub complete_capture: bool,
    pub issues: Vec<CoverageIssue>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub context: Option<AreaContext>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlaySessionView {
    pub id: String,
    pub started: Timestamp,
    pub last_traffic: Timestamp,
    pub ended: Option<Timestamp>,
    #[serde(default)]
    pub issues: Vec<CoverageIssue>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    WhiteBag,
    PersonalBest,
    KeyPopped,
    FlawlessRun,
    Whisper,
    GuildMessage,
    PartyMessage,
    /// Legacy saved alerts remain deserializable; no new occurrences are produced.
    Death,
}
impl AlertKind {
    pub const ALL: [Self; 8] = [
        Self::WhiteBag,
        Self::PersonalBest,
        Self::KeyPopped,
        Self::FlawlessRun,
        Self::Whisper,
        Self::GuildMessage,
        Self::PartyMessage,
        Self::Death,
    ];
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AlertView {
    pub id: String,
    pub at: Timestamp,
    pub kind: AlertKind,
    pub run_id: Option<String>,
    pub explanation: String,
    #[serde(default)]
    pub details: Option<crate::AlertDetails>,
    #[serde(default)]
    pub presentation: Option<crate::AlertLoudness>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EngineEvent {
    CaptureStateChanged(CaptureStatus),
    MapEntered {
        // Exact MAPINFO tuple, only for realm maps. Older events have no key.
        #[serde(default)]
        #[ts(optional = nullable)]
        realm_instance_key: Option<String>,
        at: Timestamp,
        name: String,
        kind: AreaKind,
    },
    SessionUpdated(PlaySessionView),
    RunStarted(RunView),
    RunUpdated(RunView),
    RunEnded(RunView),
    FightStarted(FightView),
    FightUpdated(FightView),
    FightEnded(FightView),
    BagDropped(DropView),
    DropUpdated(DropView),
    PlayerDied {
        at: Timestamp,
        character_id: Option<String>,
        run_id: Option<String>,
    },
    PlayerObserved(crate::PlayerProfileView),
    ScopeTimelineUpdated(crate::ScopeTimelineUpdate),
    CharacterUpdated(CharacterView),
    /// Emitted after character materialization, merge and fame events; loss has no IDs.
    LocalIdentityChanged {
        at: Timestamp,
        character_id: Option<String>,
        account_id: Option<String>,
    },
    CharacterMerged {
        provisional_id: String,
        verified_id: String,
    },
    FameChanged {
        at: Timestamp,
        character_id: String,
        fame: i32,
    },
    AlertFired(AlertView),
    InstanceEvent(InstanceEventView),
    Chat(crate::ChatMessageView),
}
/// Decimal string on the wire, preserving the full u64 range in JavaScript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct Sequence(pub Timestamp);
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EventEnvelope {
    pub stream_id: String,
    pub sequence: Sequence,
    pub at: Timestamp,
    pub event: EngineEvent,
    // Presentation-only: both fight chart fields were omitted from this update.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub fight_chart_omitted: bool,
}
/// One contiguous, bounded presentation transaction. Full envelopes retain their
/// sequence for validation and exactly-once side effects after a snapshot resync.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EventBatch {
    pub stream_id: String,
    pub first_sequence: Sequence,
    pub last_sequence: Sequence,
    pub events: Vec<EventEnvelope>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct LiveSnapshot {
    pub at: Timestamp,
    pub stream_id: String,
    pub sequence: Sequence,
    pub capture: CaptureStatus,
    pub session: Option<PlaySessionView>,
    pub character: Option<CharacterView>,
    pub map: Option<String>,
    pub area_kind: AreaKind,
    pub run: Option<RunView>,
    pub fights: Vec<FightView>,
    pub drops: Vec<DropView>,
    pub issues: Vec<CoverageIssue>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub instance: Option<InstanceLive>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run_totals: Option<RunTotals>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AreaContext {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub instance_cap: Option<u16>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub realm_name: Option<String>,
    pub entered_from_realm: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub difficulty: Option<f64>,
    // Server epoch seconds, encoded as a decimal string like Timestamp.
    // Keep seconds here rather than mixing server and capture time units.
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "optional_epoch_seconds")]
    #[ts(as = "Option<String>")]
    pub game_opened_epoch_s: Option<i64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[ts(as = "Option<f64>")]
    pub instance_age_micros: Option<u64>,
    pub modifiers: Vec<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub grade: Option<String>,
}

mod optional_epoch_seconds {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error> {
        value
            .map(|seconds| seconds.to_string())
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<i64>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|seconds| seconds.parse::<i64>())
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RealmScore {
    pub current: i32,
    pub max: i32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestTarget {
    pub object_id: i32,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub object_type: Option<u16>,
    pub since: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct InstanceLive {
    pub context: AreaContext,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub population: Option<u32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub population_peak: Option<u32>,
    pub population_provisional: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub quest_target: Option<QuestTarget>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub realm_score: Option<RealmScore>,
    // Gain since this realm visit's first materialized local entry observation.
    // Unknown if the entry fame was missing or the local incarnation changed.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub visit_fame_delta: Option<i32>,
    pub events_dropped: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum KillEvidence {
    VictoryQuestTarget,
    VictorySoleBoss,
    HpZeroRemoved,
    RealmAnnouncement,
    AnnouncedDeathRemoved,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KillView {
    pub at: Timestamp,
    pub evidence: KillEvidence,
    pub provisional: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Boss,
    EventBoss,
    Miniboss,
    Quest,
    Ordinary,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlayerSeries {
    pub player_id: String,
    pub object_id: i32,
    pub is_local: bool,
    #[serde(with = "crate::finite::vec_f64")]
    #[ts(as = "Vec<f64>")]
    pub points: Vec<f64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CaptureScope {
    RealmShark,
    Extended,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatLine {
    // Missing evidence is legacy, never verified exalt placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub split_evidence: Option<StatSplitEvidence>,
    pub stat_id: u8,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub total: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub base: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub cap: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub at_cap: Option<bool>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub boost: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub gear: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub exalt: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub other: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub current: Option<i32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ConditionView {
    pub name: String,
    pub bit: u8,
    pub secondary: bool,
    pub confirmed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatPanel {
    pub lines: Vec<StatLine>,
    pub conditions: Vec<ConditionView>,
    pub breakdown_provisional: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InstanceEventKind {
    PlayerDeath {
        #[serde(default)]
        #[ts(optional = nullable)]
        player_id: Option<String>,
        player_name: String,
        #[serde(default)]
        #[ts(optional = nullable)]
        level: Option<u16>,
        #[serde(default)]
        #[ts(optional = nullable)]
        killed_by: Option<String>,
        #[serde(default)]
        #[ts(optional = nullable)]
        grave_type: Option<i32>,
        #[serde(default)]
        #[ts(optional = nullable)]
        object_id: Option<i32>,
        #[serde(default)]
        #[ts(optional = nullable)]
        class_id: Option<u16>,
        is_local: bool,
    },
    PlayerLeft {
        object_id: i32,
        #[serde(default)]
        #[ts(optional = nullable)]
        player_name: Option<String>,
        #[serde(default)]
        #[ts(optional = nullable)]
        class_id: Option<u16>,
    },
    PortalOpened {
        #[serde(default)]
        #[ts(optional = nullable)]
        opener_is_local: Option<bool>,
        #[serde(default)]
        #[ts(optional = nullable)]
        opener: Option<String>,
        #[serde(default)]
        #[ts(optional = nullable)]
        portal_type: Option<u16>,
        #[serde(default)]
        #[ts(optional = nullable)]
        dungeon: Option<String>,
    },
    RealmEventKilled {
        boss_key: String,
        #[serde(default)]
        #[ts(optional = nullable)]
        object_type: Option<u16>,
        #[serde(default)]
        #[ts(optional = nullable)]
        killer: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InstanceEventView {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run_id: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub map: Option<String>,
    pub at: Timestamp,
    pub kind: InstanceEventKind,
    pub provisional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ExaltPlacement {
    Unresolved,
    ProvisionalInsideBoost,
    VerifiedInsideBoost,
    VerifiedInsideBase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StatSplitReason {
    MissingExalt,
    InvalidExalt,
    MissingOperands,
    Overflow,
    NegativeBase,
    NegativeOther,
    UnverifiedPlacement,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatSplitEvidence {
    pub placement: ExaltPlacement,
    pub cap_eligible: bool,
    pub reasons: Vec<StatSplitReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CharacterInputRevision {
    #[serde(deserialize_with = "character_contract_validation::identifier")]
    pub stream_id: String,
    #[serde(deserialize_with = "character_contract_validation::identifier")]
    pub character_id: String,
    #[serde(deserialize_with = "character_contract_validation::hash")]
    pub input_hash: String,
    #[serde(deserialize_with = "character_contract_validation::optional_hash")]
    pub catalog_fingerprint: Option<String>,
}

impl CharacterInputRevision {
    // Same identifier limits as native history requests; hashes are lowercase SHA-256 hex.
    pub fn validate(&self) -> Result<(), &'static str> {
        use character_contract_validation::{valid_hash, valid_identifier};
        if !valid_identifier(&self.stream_id) || !valid_identifier(&self.character_id) {
            return Err("invalid_character_revision_identifier");
        }
        if !valid_hash(&self.input_hash)
            || self
                .catalog_fingerprint
                .as_deref()
                .is_some_and(|hash| !valid_hash(hash))
        {
            return Err("invalid_character_revision_hash");
        }
        Ok(())
    }
}

/// Only calculator controls cross the bridge; observed player inputs belong to native code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CharacterCalculationRequest {
    #[serde(deserialize_with = "character_contract_validation::defense")]
    pub defense: u16,
    #[serde(deserialize_with = "character_contract_validation::comparisons")]
    pub comparison_item_types: Vec<u16>,
    pub expected_revision: CharacterInputRevision,
}

impl CharacterCalculationRequest {
    // Also validates directly constructed Rust requests before calculation.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.defense > 10_000 {
            return Err("invalid_character_defense");
        }
        if !character_contract_validation::valid_comparisons(&self.comparison_item_types) {
            return Err("invalid_character_comparisons");
        }
        self.expected_revision.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CharacterFreshness {
    Live,
    LastSeen,
    Died,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CalculationEvidence {
    Catalog,
    Modeled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CalculationCompleteness {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CharacterValue {
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub value: Option<f64>,
    pub evidence: CalculationEvidence,
    pub completeness: CalculationCompleteness,
    // Controlled reason codes; never raw observed strings.
    pub reasons: Vec<String>,
    pub exclusions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CharacterRange {
    pub min: Option<i32>,
    pub max: Option<i32>,
    pub evidence: CalculationEvidence,
    pub completeness: CalculationCompleteness,
    pub reasons: Vec<String>,
    pub exclusions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CharacterCalculation {
    pub revision: CharacterInputRevision,
    pub observed_at: Timestamp,
    pub catalog_fingerprint: Option<String>,
    pub formula_version: String,
    pub freshness: CharacterFreshness,
    pub issues: Vec<CoverageIssue>,
    pub applied_defense: u16,
    // Equipped weapon first, followed by at most two comparisons at the same observed stats.
    pub weapons: Vec<CharacterWeaponCalculation>,
    pub regeneration: CharacterRegeneration,
    pub exalt_damage: DeferredExaltDamage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CharacterWeaponCalculation {
    pub item_type: Option<u16>,
    pub groups: Vec<CharacterAttackGroupCalculation>,
    pub applied_att: Option<i32>,
    pub applied_dex: Option<i32>,
    pub applied_primary_mask: Option<u32>,
    // Unavailable when heterogeneous groups have no single rate/range.
    pub rate_factor: CharacterValue,
    pub catalog_range: CharacterRange,
    pub hit_range: CharacterRange,
    pub hit_mean: CharacterValue,
    pub attacks_per_sec: CharacterValue,
    pub projectiles_per_sec: CharacterValue,
    pub dps_subtotal: CharacterValue,
    pub dps_total: CharacterValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CharacterAttackGroupCalculation {
    pub group_id: String,
    pub projectile_index: i16,
    // Missing projectile metadata cannot establish piercing.
    pub armor_piercing: Option<bool>,
    pub count: u16,
    pub rate_factor: CharacterValue,
    pub catalog_range: CharacterRange,
    pub hit_range: CharacterRange,
    pub hit_mean: CharacterValue,
    pub attacks_per_sec: CharacterValue,
    pub projectiles_per_sec: CharacterValue,
    pub dps: CharacterValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CharacterRegeneration {
    pub applied_wis: Option<i32>,
    pub wis: CharacterValue,
    pub effects: Vec<CharacterRegenContribution>,
    // Supported components only; never label these as full recovery.
    pub hp_subtotal: CharacterValue,
    pub mp_subtotal: CharacterValue,
    pub hp_total: CharacterValue,
    pub mp_total: CharacterValue,
    // Excluded/unavailable until separately verified; not assumed zero.
    pub base_vit: CharacterValue,
    pub base_mp: CharacterValue,
}

/// One HP or MP effect from one observed slot/enchant; coefficients retain catalog evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CharacterRegenContribution {
    pub source_slot: u8,
    pub enchant_id: i32,
    pub resource: CharacterRegenResource,
    pub applied_maximum: Option<i32>,
    pub flat_per_sec: CharacterValue,
    pub fraction_of_max_per_sec: CharacterValue,
    pub source_units: Vec<String>,
    pub subtotal: CharacterValue,
    pub total: CharacterValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CharacterRegenResource {
    Hp,
    Mp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ExaltDamageStatus {
    Deferred,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DeferredExaltDamage {
    pub status: ExaltDamageStatus,
    #[serde(with = "character_contract_validation::deferred_value")]
    #[ts(type = "null")]
    pub value: Option<f64>,
    pub reason: String,
}

mod character_contract_validation {
    use serde::{Deserialize, Deserializer};

    pub(super) fn valid_identifier(value: &str) -> bool {
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
    }

    pub(super) fn valid_hash(value: &str) -> bool {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    pub(super) fn valid_comparisons(values: &[u16]) -> bool {
        values.len() <= 2 && (values.len() < 2 || values[0] != values[1])
    }

    pub(super) fn identifier<'de, D: Deserializer<'de>>(de: D) -> Result<String, D::Error> {
        let value = String::deserialize(de)?;
        if !valid_identifier(&value) {
            return Err(serde::de::Error::custom(
                "invalid_character_revision_identifier",
            ));
        }
        Ok(value)
    }

    pub(super) fn hash<'de, D: Deserializer<'de>>(de: D) -> Result<String, D::Error> {
        let value = String::deserialize(de)?;
        if !valid_hash(&value) {
            return Err(serde::de::Error::custom("invalid_character_revision_hash"));
        }
        Ok(value)
    }

    pub(super) fn optional_hash<'de, D: Deserializer<'de>>(
        de: D,
    ) -> Result<Option<String>, D::Error> {
        let value = Option::<String>::deserialize(de)?;
        if value.as_deref().is_some_and(|hash| !valid_hash(hash)) {
            return Err(serde::de::Error::custom("invalid_character_revision_hash"));
        }
        Ok(value)
    }

    pub(super) fn defense<'de, D: Deserializer<'de>>(de: D) -> Result<u16, D::Error> {
        let value = u16::deserialize(de)?;
        if value > 10_000 {
            return Err(serde::de::Error::custom("invalid_character_defense"));
        }
        Ok(value)
    }

    pub(super) fn comparisons<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<u16>, D::Error> {
        let values = Vec::<u16>::deserialize(de)?;
        if !valid_comparisons(&values) {
            return Err(serde::de::Error::custom("invalid_character_comparisons"));
        }
        Ok(values)
    }

    pub(super) mod deferred_value {
        use serde::{Deserializer, Serializer};

        pub fn serialize<S: Serializer>(
            value: &Option<f64>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            if value.is_some() {
                return Err(serde::ser::Error::custom("exalt_damage_deferred"));
            }
            crate::finite::optional_f64::serialize(value, serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<f64>, D::Error> {
            let value = crate::finite::optional_f64::deserialize(de)?;
            if value.is_some() {
                return Err(serde::de::Error::custom("exalt_damage_deferred"));
            }
            Ok(value)
        }
    }
}

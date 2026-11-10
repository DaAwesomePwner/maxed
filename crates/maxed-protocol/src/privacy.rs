//! Fill-by-default fixture policy, driven by the ordinary packet schemas.
//! Names below are schema constants, never values obtained from the recording.
use crate::{defs, ProtocolError};
use maxed_types::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretKind {
    Credential,
    Identity,
    PlayerText,
    Unclassified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPolicy {
    Keep,
    Pseudonym,
    Fill,
    FillUnclassified,
}
pub struct FieldTreatment {
    pub offset: usize,
    pub len: usize,
    pub name: &'static str,
    pub policy: FieldPolicy,
    /// Exact public literals/formats may explain player-text coincidences only.
    pub public_literal: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SensitiveSpan {
    pub offset: usize,
    pub len: usize,
    pub kind: SecretKind,
}
/// Schema-only evidence, including numeric IDs but never stat values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldEvidence {
    pub field: &'static str,
    pub stat_id: Option<u8>,
    pub reason: EvidenceReason,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceReason {
    UnmappedNumericStat,
    UnclassifiedStringStat,
    UnknownObjectName,
    NonPlayerObjectName,
    UnlistedPlayerRangeType,
    UnknownNotificationLayout,
}
impl EvidenceReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::NonPlayerObjectName => "non-player NAME_STAT may be player-authored; filled as player-text",
            Self::UnlistedPlayerRangeType => "unlisted player-range type; NAME_STAT filled as player-text",
            Self::UnknownObjectName => "object type unavailable; NAME_STAT filled as player-text",
            Self::UnmappedNumericStat => "unmapped statTypeNum; compressed-integer primary/secondary values kept per RS StatData rules",
            Self::UnclassifiedStringStat => "string stat has no privacy policy; known length-prefixed extent filled",
            Self::UnknownNotificationLayout => "unknown ProgressBar extra flag bits; conditional payload extent unproven",
        }
    }
    pub fn refuses(self) -> bool {
        self == Self::UnknownNotificationLayout
    }
}
/// Contains offsets only; never contains wire values or a pseudonym map.
#[derive(Default)]
pub struct Inventory {
    pub spans: Vec<SensitiveSpan>,
    pub fields: Vec<(usize, usize)>,
    pub binary: Vec<usize>,
    pub treatments: Vec<FieldTreatment>,
    pub evidence: Vec<FieldEvidence>,
}

/// Static class allowlist for scrubbing without a loaded asset catalog, from players.xml (via RealmShark asset extraction, 2026-10-02).
pub const PLAYER_CLASSES: &[(u16, &str)] = &[
    (768, "Rogue"),
    (775, "Archer"),
    (782, "Wizard"),
    (784, "Priest"),
    (785, "Samurai"),
    (796, "Bard"),
    (797, "Warrior"),
    (798, "Knight"),
    (799, "Paladin"),
    (800, "Assassin"),
    (801, "Necromancer"),
    (802, "Huntress"),
    (803, "Mystic"),
    (804, "Trickster"),
    (805, "Sorcerer"),
    (806, "Ninja"),
    (817, "Summoner"),
    (818, "Kensei"),
    (819, "Druid"),
];

/// Connection-local state only; never serialized into a scrub report.
#[derive(Default)]
pub struct ObjectContext {
    types: std::collections::HashMap<i32, u16>,
    local_player: Option<i32>,
}
impl ObjectContext {
    pub fn observe(&mut self, packet: &defs::Packet) {
        match packet {
            defs::Packet::MapInfo(_) | defs::Packet::Reconnect(_) => self.clear(),
            defs::Packet::CreateSuccess(p) => self.local_player = Some(p.objectId),
            defs::Packet::Update(p) => {
                for id in &p.drops {
                    self.types.remove(id);
                    if self.local_player == Some(*id) {
                        self.local_player = None;
                    }
                }
                for object in &p.newObjects {
                    self.types.insert(object.status.objectId, object.objectType);
                }
            }
            _ => {}
        }
    }
    pub fn clear(&mut self) {
        self.types.clear();
        self.local_player = None;
    }
    pub(crate) fn name_policy(
        &self,
        object_id: i32,
        inline_type: Option<u16>,
    ) -> (FieldPolicy, Option<EvidenceReason>) {
        if self.local_player == Some(object_id) {
            return (FieldPolicy::Pseudonym, None);
        }
        match inline_type.or_else(|| self.types.get(&object_id).copied()) {
            Some(kind) if PLAYER_CLASSES.iter().any(|entry| entry.0 == kind) => {
                (FieldPolicy::Pseudonym, None)
            }
            // Reserve the player-class range for classes added after this snapshot.
            Some(768..=1023) => (
                FieldPolicy::FillUnclassified,
                Some(EvidenceReason::UnlistedPlayerRangeType),
            ),
            // No non-player name allowlist: graves and pets can carry player names.
            Some(_) => (
                FieldPolicy::FillUnclassified,
                Some(EvidenceReason::NonPlayerObjectName),
            ),
            None => (
                FieldPolicy::FillUnclassified,
                Some(EvidenceReason::UnknownObjectName),
            ),
        }
    }
}

/// Complete string-stat policy: RS packets/data/StatData.java::isStringStat
/// supplies the IDs; packets/data/enums/StatType.java supplies their meanings.
pub const STRING_STATS: &[(u8, &str, FieldPolicy)] = &[
    (6, "EXP_STAT", FieldPolicy::Keep), // Server experience game data.
    (31, "NAME_STAT", FieldPolicy::Pseudonym), // Contextual: player alias; all other object names filled.
    (38, "ACCOUNT_ID_STAT", FieldPolicy::Fill), // Account identifier.
    (54, "OWNER_ACCOUNT_ID_STAT", FieldPolicy::Fill), // Owner account identifier.
    (62, "GUILD_NAME_STAT", FieldPolicy::Pseudonym), // Player-created guild name.
    (71, "MATERIAL_STAT", FieldPolicy::Keep),  // Server crafting material data.
    (72, "MATERIAL_CAP_STAT", FieldPolicy::Keep), // Server material capacity.
    (80, "UNIQUE_DATA_STRING", FieldPolicy::Keep), // Item/enchantment game data.
    (82, "PET_NAME_STAT", FieldPolicy::Pseudonym), // Potentially named pet.
    (115, "GRAVE_ACCOUNT_ID", FieldPolicy::Fill), // Grave account identifier.
    (121, "MODIFIERS_STAT", FieldPolicy::Keep), // Dungeon modifier game data.
    (127, "DUST_STAT", FieldPolicy::Keep),     // Server enchantment dust data.
    (128, "CRUCIBLE_STAT", FieldPolicy::Keep), // Server crucible game data.
    (147, "DUST_AMOUNT_STAT", FieldPolicy::Keep), // Server enchantment dust quantity.
    (155, "BloodRitualStat", FieldPolicy::Keep), // Server ritual game data.
];

// RS outgoing/HelloPacket.java: buildVersion is a dotted numeric version.
pub(crate) fn version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()))
}
// RS data/enums/FailureMessage.java: exact server templates, no interpolated text.
pub(crate) const FAILURE_MESSAGES: &[&str] = &[
    "Character is dead",
    "Character not found",
    "Your IP has been temporarily banned for abuse/hacking on this server",
];
// RS packetcapture/logger/CompletionDialogue.java: exact NPC/line pairs prove
// server dialogue; '#' alone cannot prove arbitrary text contains no player name.
pub(crate) const NPC_DIALOGUE: &[(&str, &str)] = &[
    (
        "#Kitsune Umi",
        "This fully concludes the Moonlight Festival!",
    ),
    ("#Dancer Miko", "Thank you all for coming tonight."),
    (
        "#Umi, Goddess of Revelry",
        "This fully concludes the Moonlight Festival.",
    ),
    (
        "#Void Entity",
        "You fools... You can never truly defeat me! I am in all of you! I AM all of you!",
    ),
    (
        "#The Accursed King",
        "...do you truly think your end will be any different?",
    ),
    ("#King Azamoth", "This fate is mine to bear... not hers."),
];
pub fn inventory(id: u8, dir: Direction, body: &[u8]) -> Result<Inventory, ProtocolError> {
    defs::privacy_inventory(id, dir, body, None)
}
pub fn inventory_with_context(
    id: u8,
    dir: Direction,
    body: &[u8],
    context: &ObjectContext,
) -> Result<Inventory, ProtocolError> {
    defs::privacy_inventory(id, dir, body, Some(context))
}
pub fn sensitive_spans(
    id: u8,
    dir: Direction,
    body: &[u8],
) -> Result<Vec<SensitiveSpan>, ProtocolError> {
    let inventory = inventory(id, dir, body)?;
    if inventory.evidence.iter().any(|e| e.reason.refuses()) {
        return Err(ProtocolError::Definition);
    }
    Ok(inventory.spans)
}

// RS RealmCharacterStats.parseStats order, resolved through CharacterStatistics.
// Not numeric bit order: the counter stream follows these explicit reads.
pub(crate) const CHARACTER_STAT_ORDER: &[u8] = &[
    24, 25, 26, 27, 28, 29, 30, 31, 16, 17, 18, 19, 20, 21, 22, 23, 8, 9, 10, 12, 13, 14, 15, 0, 1,
    2, 3, 4, 5, 6, 7, 58, 59, 60, 61, 62, 63, 48, 49, 50, 51, 52, 53, 55, 40, 41, 42, 43, 44, 45,
    46, 47, 32, 33, 34, 35, 36, 37, 38, 39, 88, 89, 90, 91, 92, 93, 94, 95, 80, 81, 82, 83, 84, 85,
    86, 87, 72, 74, 75, 76, 77, 78, 79, 64, 65, 66, 67, 68, 69, 70, 71, 120, 121, 122, 123, 124,
    125, 126, 127, 112, 113, 114, 115, 116, 117, 118, 119, 104, 105, 106, 107, 108, 109, 110, 111,
    96, 97, 98, 99, 100, 101, 102, 103,
];

// RS PcStatsDecoder.sixBitStringToBytes uses the URL-safe alphabet and '=' padding.
// Reject noncanonical encodings and ignored bits instead of retaining opaque data.
pub(crate) fn six_bit_bytes(value: &str) -> Option<Vec<u8>> {
    fn digit(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let a = digit(chunk[0])?;
        let b = digit(chunk[1])?;
        let last = (index + 1) * 4 == bytes.len();
        decoded.push(a << 2 | b >> 4);
        if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' || b & 15 != 0 {
                return None;
            }
        } else {
            let c = digit(chunk[2])?;
            decoded.push(b << 4 | c >> 2);
            if chunk[3] == b'=' {
                if !last || c & 3 != 0 {
                    return None;
                }
            } else {
                decoded.push(c << 6 | digit(chunk[3])?);
            }
        }
    }
    Some(decoded)
}

pub(crate) fn character_stats(value: &str) -> bool {
    fn decode(value: &str) -> Option<()> {
        let data = six_bit_bytes(value)?;
        let mut reader = crate::Reader::new(&data);
        // RS reads a little-endian int; only zero/nonzero matters here.
        let flag = reader.i32().ok()?;
        if flag != 0 {
            let mask = reader.take(16).ok()?;
            let present = |id: u8| mask[usize::from(id / 8)] & (1 << (id % 8)) != 0;
            // RS does not parse these bits; do not admit uninterpreted flags.
            if (0..128).any(|id| present(id) && !CHARACTER_STAT_ORDER.contains(&id)) {
                return None;
            }
            for &id in CHARACTER_STAT_ORDER {
                if present(id) {
                    reader.ci().ok()?;
                }
            }
        }
        (reader.remaining() == 0).then_some(())
    }
    decode(value).is_some()
}

/// Every object key observed in CRUCIBLE_RESPONSE JSON on the owner's recording
/// (docs/parity/protocol-evidence.md, 2026-11-10): season crucible, weekly
/// modifier event and Blood Ritual ladder configuration. None is per-account.
const CRUCIBLE_CONFIG_KEYS: &[&str] = &[
    "amount",
    "array",
    "bloodRitual",
    "bonuses",
    "chance",
    "coverage",
    "crucible",
    "desc",
    "dungeons",
    "end",
    "exclGrounds",
    "exclRoomTypes",
    "excludeLabels",
    "fame",
    "healMethods",
    "id",
    "item",
    "jitter",
    "labels",
    "leavelimitation",
    "max",
    "min",
    "modifierEvent",
    "modifiers",
    "objectIds",
    "percent",
    "restrictions",
    "roomType",
    "seasonalonly",
    "stat",
    "start",
    "title",
    "trials",
    "type",
    "value",
];

/// Server configuration is kept only as a `{"array":[...]}` object whose keys
/// are all allowlisted. Any new key (for example, progress or unlock state) or
/// `\u` escape, which could hide text from residual checks, fills the field.
pub(crate) fn crucible_config(value: &str) -> bool {
    use serde_json::Value;
    fn known(value: &Value, depth: usize) -> bool {
        match value {
            _ if depth > 8 => false,
            Value::Array(items) => items.iter().all(|item| known(item, depth + 1)),
            Value::Object(fields) => fields.iter().all(|(key, item)| {
                CRUCIBLE_CONFIG_KEYS.contains(&key.as_str()) && known(item, depth + 1)
            }),
            _ => true,
        }
    }
    if value.contains("\\u") {
        return false;
    }
    let Ok(top) = serde_json::from_str::<Value>(value) else {
        return false;
    };
    top.as_object()
        .is_some_and(|o| o.len() == 1 && o.get("array").is_some_and(Value::is_array))
        && known(&top, 0)
}

pub(crate) fn oryx_killer_spans(text: &str) -> Option<Vec<std::ops::Range<usize>>> {
    crate::notices::parse_oryx_announcement(text).map(|notice| notice.killer_spans)
}

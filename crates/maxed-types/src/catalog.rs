//! Catalog contracts. Unknown values remain explicit nulls.

use crate::{AreaKind, StatView};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Catalog {
    /// Version 1; collections are sorted by numeric type/index or stable key at publication.
    pub format_version: u32,
    pub client_fingerprint: String,
    pub items: Vec<ItemDef>,
    pub objects: Vec<ObjectDef>,
    pub classes: Vec<ClassDef>,
    pub enchants: Vec<EnchantDef>,
    pub dungeons: Vec<DungeonDef>,
    pub bags: Vec<BagDef>,
    pub sprites: Vec<SpriteRef>,
    pub tiles: Vec<TileDef>,
    /// Empty means no multiplier evidence for an active crucible.
    pub crucible_bonuses: Vec<CrucibleDef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ProjectileDef {
    pub index: i16,
    pub min_damage: i32,
    pub max_damage: i32,
    pub armor_piercing: bool,
    pub lifetime_millis: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ScalingDef {
    pub stat_id: u8,
    pub minimum: i32,
    #[serde(with = "crate::finite::f32_value")]
    #[ts(as = "f32")]
    pub damage_per_stat: f32,
    pub shots: u16,
    pub origin_item: Option<u16>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PotionStat {
    Life,
    Mana,
    Attack,
    Defense,
    Speed,
    Dexterity,
    Vitality,
    Wisdom,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ItemDef {
    // Absent in legacy catalogs: firing metadata has not been extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub weapon_firing: Option<WeaponFiringDef>,
    pub object_type: u16,
    pub name: String,
    pub tier: Option<String>,
    pub slot_type: Option<u8>,
    pub projectiles: Vec<ProjectileDef>,
    pub scaling: Option<ScalingDef>,
    pub sprite_key: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub soulbound: Option<bool>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub stat_bonuses: Option<Vec<StatView>>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub potion_stat: Option<PotionStat>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ObjectDef {
    pub object_type: u16,
    pub name: String,
    pub is_player: bool,
    pub is_boss: bool,
    pub is_miniboss: bool,
    pub sprite_key: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub class_name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub dungeon_name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub event_boss: Option<bool>,
    // XML object ID normalized to the key used by realm announcements.
    // Display-name variants share announcement resolution in the engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub realm_event_key: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ClassDef {
    pub object_type: u16,
    pub stat_caps: Vec<StatView>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EnchantDef {
    pub id: i32,
    pub name: String,
    pub description: String,
    /// Product of StatModMult mutators, evaluated in Java f32 order. None in
    /// older catalogs means combat metadata was not extracted, not neutral.
    #[serde(default, with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub stat_damage_multiplier: Option<f64>,
    // Absent is unknown; supported empty lists establish no regeneration effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub regeneration: Option<EnchantRegenDef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DungeonDef {
    pub name: String,
    pub aliases: Vec<String>,
    pub kind: AreaKind,
    pub completion_index: Option<u16>,
    /// Lowest-typed portal object whose DungeonName is this name or an alias.
    /// Derived at extraction from client XML; None when no portal opens it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub portal_type: Option<u16>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct BagDef {
    pub object_type: u16,
    pub name: String,
    pub base_type: u16,
    pub is_white: bool,
    pub rank: Option<u8>,
    pub capture: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SpriteTrim {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SpriteRef {
    pub key: String,
    /// Cache-relative logical name, never an absolute path or URL.
    pub sheet: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    // Visible bounds relative to the full source rectangle; absent in old catalogs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub trim: Option<SpriteTrim>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct TileDef {
    pub tile_type: u16,
    pub min_damage: i32,
    pub max_damage: i32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CrucibleDef {
    pub id: String,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub damage_multiplier: f64,
}

/// Extraction support is independent of whether any effects were found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSupport {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WeaponSchedule {
    Simultaneous,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct WeaponFiringDef {
    pub support: CatalogSupport,
    pub groups: Vec<WeaponAttackGroup>,
    pub omitted_projectile_indices: Vec<i16>,
    // Controlled reason codes, never source XML or display descriptions.
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct WeaponAttackGroup {
    pub group_id: String,
    pub projectile_index: i16,
    pub count: u16,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub rate_factor: f64,
    pub schedule: WeaponSchedule,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EnchantRegenDef {
    pub support: CatalogSupport,
    pub hp: Vec<RegenEffect>,
    pub mp: Vec<RegenEffect>,
    pub reasons: Vec<String>,
    // Reviewed unit identifiers, not display text or inferred defaults.
    pub source_units: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RegenEffect {
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub flat_per_sec: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub fraction_of_max_per_sec: Option<f64>,
    pub source_units: Vec<String>,
}

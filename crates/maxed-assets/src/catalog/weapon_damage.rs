//! Enchant weapon damage-range mutators (D-weapon-enchant, docs/architecture/assets.md).
//!
//! Observed client shape: `Enchantment/Mutators/MultiplyMinDamage projectileId="-1"`
//! with the multiplier as element text, always paired with an equal
//! `MultiplyMaxDamage`. Anything else that could change a weapon shot's damage is
//! kept `Unsupported` rather than claimed neutral.
use super::{reason, valid_codes};
use crate::xml::Node;
use maxed_types::{CatalogSupport, EnchantWeaponDamageDef};

pub(super) const REASONS: &[&str] = &[
    "invalid_damage_multiplier",
    "unsupported_damage_scope",
    "unsupported_subattack",
    "unsupported_weapon_mutator",
];

// Reviewed mutators that cannot change a weapon projectile's damage range: firing,
// flight, size and mana cost multipliers, and on-event procs that are separate
// damage sources. Equip stat changes (e.g. attack) arrive in observed player stats.
const NEUTRAL_TAGS: &[&str] = &[
    "MultiplyRateOfFire",
    "MultiplySpeed",
    "MultiplyLifetimeMS",
    "MultiplySize",
    "MultiplyMPCost",
    "AddOnPlayerHitActivate",
    "AddOnPlayerShootActivate",
    "AddOnPlayerAbilityActivate",
    "AddOnEnemyHitActivate",
];
const NEUTRAL_EQUIP: &[&str] = &[
    "IncrementStat",
    "BonusStatRelative",
    "LootBonus",
    "XPBonus",
    "DustBonus",
    "FlatRegen",
    "PercentageRegen",
    "StatModMult",
    "DamageMultSelf",
];

/// Multiplies each bound's values in XML order with an f32 accumulator, like
/// `enchant_stat_multiplier`. Only the all-projectile scope (`projectileId="-1"`)
/// is supported; scoped, malformed or negative values make the enchant unsupported.
pub(super) fn parse(n: &Node) -> EnchantWeaponDamageDef {
    let mut reasons = vec![];
    let (mut min, mut max) = (1.0f32, 1.0f32);
    for container in n.children.iter().filter(|c| c.tag == "Mutators") {
        if !container.attrs.is_empty() || !container.text.trim().is_empty() {
            reason(&mut reasons, "unsupported_weapon_mutator");
        }
        for effect in &container.children {
            let tag = effect.tag.as_str();
            if matches!(tag, "MultiplyMinDamage" | "MultiplyMaxDamage") {
                if effect.attrs.len() != 1
                    || effect.attr("projectileId") != Some("-1")
                    || !effect.children.is_empty()
                {
                    reason(&mut reasons, "unsupported_damage_scope");
                    continue;
                }
                let Some(value) = effect
                    .text
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                else {
                    reason(&mut reasons, "invalid_damage_multiplier");
                    continue;
                };
                if tag == "MultiplyMinDamage" {
                    min *= value;
                } else {
                    max *= value;
                }
            } else if matches!(tag, "SetSubAttack" | "AddSubAttack") {
                reason(&mut reasons, "unsupported_subattack");
            } else if tag == "ActivateOnEquip" {
                if !NEUTRAL_EQUIP.contains(&effect.text.trim()) {
                    reason(&mut reasons, "unsupported_weapon_mutator");
                }
            } else if !NEUTRAL_TAGS.contains(&tag) {
                reason(&mut reasons, "unsupported_weapon_mutator");
            }
        }
    }
    if !min.is_finite() || !max.is_finite() {
        reason(&mut reasons, "invalid_damage_multiplier");
    }
    if reasons.is_empty() {
        EnchantWeaponDamageDef {
            support: CatalogSupport::Supported,
            min_multiplier: Some(f64::from(min)),
            max_multiplier: Some(f64::from(max)),
            reasons,
        }
    } else {
        EnchantWeaponDamageDef {
            support: CatalogSupport::Unsupported,
            min_multiplier: None,
            max_multiplier: None,
            reasons,
        }
    }
}

/// Supported carries finite, non-negative f32-range multipliers and no reasons;
/// unsupported carries reasons and no multipliers. `Partial` is never produced.
pub(super) fn validate(d: &EnchantWeaponDamageDef) -> bool {
    let valid =
        |m: Option<f64>| m.is_some_and(|m| m.is_finite() && m >= 0.0 && m <= f64::from(f32::MAX));
    valid_codes(&d.reasons, REASONS)
        && match d.support {
            CatalogSupport::Supported => {
                d.reasons.is_empty() && valid(d.min_multiplier) && valid(d.max_multiplier)
            }
            CatalogSupport::Unsupported => {
                !d.reasons.is_empty() && d.min_multiplier.is_none() && d.max_multiplier.is_none()
            }
            CatalogSupport::Partial => false,
        }
}

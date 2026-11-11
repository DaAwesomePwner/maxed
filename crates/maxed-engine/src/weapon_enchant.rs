//! Weapon enchant damage-range scaling for local weapon shots
//! (D-weapon-enchant in docs/parity/deliberate-differences.md).
use crate::Engine;
use maxed_types::{CatalogSupport, EnchantState, ProjectileDef};

impl Engine {
    /// Shot-time product of the equipped weapon's enchant min/max multipliers,
    /// multiplied in slot order with an f32 accumulator. `None` means the evidence
    /// is missing, invalid, for another item, unknown, legacy or unsupported.
    pub(crate) fn weapon_enchant_scale(&self, owner: i32, weapon: u16) -> Option<(f32, f32)> {
        let e = self.entities.get(&owner)?;
        // Inventory slot 0 (stat 8) holds the weapon; enchant slot 0 is its evidence.
        if e.stat(8) != Some(i32::from(weapon)) {
            return None;
        }
        let view = e.enchants.first()?;
        match view.state {
            EnchantState::Missing | EnchantState::Invalid => return None,
            EnchantState::RecordedEmpty => return Some((1.0, 1.0)),
            EnchantState::Recorded => {}
        }
        let (mut min, mut max) = (1.0f32, 1.0f32);
        // Applied IDs are >= 0; -1/-2 are empty/locked markers and -3 ends the list.
        for id in view
            .ids
            .iter()
            .take_while(|&&id| id != -3)
            .filter(|&&id| id >= 0)
        {
            let def = &self.catalog.enchants[*self.enchant_index.get(id)?];
            let damage = def
                .weapon_damage
                .as_ref()
                .filter(|d| d.support == CatalogSupport::Supported)?;
            min *= damage.min_multiplier? as f32;
            max *= damage.max_multiplier? as f32;
        }
        Some((min, max))
    }
}

/// Damage bounds after enchant scaling, applied before the roll. Each bound is the
/// f32 product truncated toward zero; the recording could not distinguish this
/// from rounding (see D-weapon-enchant). `None` if the scaled range is unusable.
pub(crate) fn scaled_bounds(p: &ProjectileDef, (min, max): (f32, f32)) -> Option<(i32, i32)> {
    if min == 1.0 && max == 1.0 {
        return Some((p.min_damage, p.max_damage));
    }
    let scale = |v: i32, m: f32| {
        let v = v as f32 * m;
        (v.is_finite() && (0.0..=i32::MAX as f32).contains(&v)).then_some(v as i32)
    };
    let (lo, hi) = (scale(p.min_damage, min)?, scale(p.max_damage, max)?);
    (lo <= hi).then_some((lo, hi))
}

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
        // ID 0 is applied here, as in parse_enchant (it makes the view Recorded) and
        // character.rs, so it resolves like any ID: unknown means a counted assumption.
        // The older ability path (combat.rs, D-scaling) skips ID 0 with `> 0`; skipping
        // it here would turn a Recorded view into a silent, uncounted 1.0.
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
/// from rounding (see D-weapon-enchant). `None` if the scaled range is unusable:
/// a negative, non-finite or above-i32::MAX bound, or a minimum above the maximum.
/// Bounds that become equal are kept, so the shot skips its roll.
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

#[cfg(test)]
mod tests {
    use super::scaled_bounds;
    use maxed_types::ProjectileDef;

    fn projectile(min_damage: i32, max_damage: i32) -> ProjectileDef {
        ProjectileDef {
            index: 0,
            min_damage,
            max_damage,
            armor_piercing: false,
            lifetime_millis: None,
        }
    }

    #[test]
    fn unusable_scaled_bounds_are_none() {
        let p = projectile(10, 11);
        // Catalog validation rejects these products; the engine still refuses them.
        assert_eq!(scaled_bounds(&p, (-1.0, -1.0)), None);
        assert_eq!(scaled_bounds(&p, (1.0, -1.0)), None);
        assert_eq!(scaled_bounds(&p, (f32::NAN, f32::NAN)), None);
        assert_eq!(scaled_bounds(&p, (2.0, 0.5)), None);
        assert_eq!(scaled_bounds(&p, (1e10, 1e10)), None);
        assert_eq!(scaled_bounds(&p, (f32::MAX, f32::MAX)), None);
    }

    #[test]
    fn equal_scaled_bounds_are_kept() {
        assert_eq!(scaled_bounds(&projectile(10, 11), (0.8, 0.8)), Some((8, 8)));
        assert_eq!(scaled_bounds(&projectile(0, 0), (0.8, 0.8)), Some((0, 0)));
    }
}

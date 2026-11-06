//! Pure, theoretical normal-fire and baseline regeneration calculations.
//! Every shot hits one target once; regeneration exposes only modeled contributions.
//!
//! Observed totals already contain stat boosts. These results never feed combat
//! reconstruction, and excluded damage channels are not asserted to be neutral.
use maxed_types::*;
use sha2::{Digest, Sha256};

const FORMULA_VERSION: &str = "character-v1";
const WORK_LIMIT: usize = 65_536;
// Match the catalog's bounded projectile collection; counts are bounded by u16
// and are multiplied, never expanded into individual shots.
const GROUP_LIMIT: usize = 256;
const EXCLUSIONS: &[&str] = &[
    "berserk",
    "dazed",
    "stunned",
    "firing_restrictions",
    "special_effects",
    "weapon_enchant_damage",
    "weapon_enchant_rate",
    "exalt_damage",
    "crucible",
    "blood_ritual",
    "enemy_conditions",
    "misses",
    "range",
    "repeat_hits",
    "downtime",
];

type Number = Result<f64, &'static str>;

/// Immutable local observation supplied by the engine owner.
pub struct CharacterInput<'a> {
    pub character: &'a CharacterView,
    pub stream_id: &'a str,
    pub capture: &'a CaptureStatus,
}

/// Hash only calculation inputs, in deterministic order. Presentation refreshes,
/// fight state, stat breakdowns and the revision itself cannot invalidate it.
pub fn input_revision(
    stream_id: &str,
    character: &CharacterView,
    catalog: Option<&Catalog>,
) -> CharacterInputRevision {
    let mut stats: Vec<_> = character
        .stats
        .iter()
        .filter(|s| [0, 3, 20, 27, 28, 29].contains(&s.stat_id))
        .map(|s| (s.stat_id, s.value))
        .collect();
    stats.sort_unstable();
    let mut equipment: Vec<_> = character.equipment.iter().collect();
    equipment.sort_by_key(|e| e.slot);
    let fingerprint = catalog.map(|c| c.client_fingerprint.clone());
    // This tuple contains only strings, integers, enums and lists, so JSON
    // serialization cannot fail through non-finite numbers or non-string keys.
    let bytes = serde_json::to_vec(&(
        stream_id,
        &character.id,
        character.identity_verified,
        character.class_id,
        character.died_at,
        &character.issues,
        stats,
        equipment,
        &fingerprint,
    ))
    .expect("integer/string Character revision inputs serialize");
    CharacterInputRevision {
        stream_id: stream_id.into(),
        character_id: character.id.clone(),
        input_hash: format!("{:x}", Sha256::digest(bytes)),
        catalog_fingerprint: fingerprint,
    }
}

pub fn calculate(
    input: CharacterInput<'_>,
    catalog: Option<&Catalog>,
    options: &CharacterCalculationRequest,
) -> CharacterCalculation {
    let character = input.character;
    let revision = input_revision(input.stream_id, character, catalog);
    let invalid = options
        .validate()
        .err()
        .or_else(|| revision.validate().err())
        .or_else(|| (options.expected_revision != revision).then_some("stale_character_revision"));
    let mut slots = character.equipment.iter().filter(|e| e.slot == 0);
    let slot = slots.next();
    let equipped = slot.and_then(|e| e.item_type);
    let equipment_reason = if slots.next().is_some() {
        Some("ambiguous_weapon_slot")
    } else if slot.is_some_and(|e| e.observed_empty == Some(true)) {
        Some(if equipped.is_some() {
            "invalid_weapon_slot"
        } else {
            "empty_weapon_slot"
        })
    } else if equipped.is_none() {
        Some("missing_weapon_slot")
    } else {
        None
    };
    let mut weapons = vec![weapon(
        character,
        catalog,
        equipped,
        options.defense,
        invalid.or(equipment_reason),
    )];
    // Invalid requests never amplify work through an unbounded comparison list.
    if options.validate().is_ok() {
        weapons.extend(
            options
                .comparison_item_types
                .iter()
                .map(|&item| weapon(character, catalog, Some(item), options.defense, invalid)),
        );
    }
    let mut regeneration = regeneration(character, catalog);
    if let Some(reason) = invalid {
        // A rejected/stale request cannot publish current modeled contributions.
        for row in [
            &mut regeneration.wis,
            &mut regeneration.hp_subtotal,
            &mut regeneration.mp_subtotal,
            &mut regeneration.hp_total,
            &mut regeneration.mp_total,
        ] {
            row.value = None;
            row.completeness = CalculationCompleteness::Unavailable;
            row.reasons.push(reason.into());
        }
        for effect in &mut regeneration.effects {
            for row in [&mut effect.subtotal, &mut effect.total] {
                row.value = None;
                row.completeness = CalculationCompleteness::Unavailable;
                row.reasons.push(reason.into());
            }
        }
    }
    CharacterCalculation {
        catalog_fingerprint: revision.catalog_fingerprint.clone(),
        revision,
        observed_at: character.observed_at,
        formula_version: FORMULA_VERSION.into(),
        freshness: if character.died_at.is_some() {
            CharacterFreshness::Died
        } else if input.capture.state == CaptureState::Live {
            CharacterFreshness::Live
        } else {
            CharacterFreshness::LastSeen
        },
        issues: character.issues.clone(),
        applied_defense: options.defense,
        weapons,
        regeneration,
        exalt_damage: DeferredExaltDamage {
            status: ExaltDamageStatus::Deferred,
            value: None,
            reason: "exalt_damage_deferred".into(),
        },
    }
}

fn observed(character: &CharacterView, stat_id: u8) -> Result<i32, &'static str> {
    let mut values = character.stats.iter().filter(|s| s.stat_id == stat_id);
    let value = values
        .next()
        .ok_or(match stat_id {
            20 => "missing_att",
            28 => "missing_dex",
            29 => "missing_primary_mask",
            _ => "missing_stat",
        })?
        .value;
    if values.next().is_some() {
        return Err("ambiguous_stat");
    }
    Ok(value)
}

fn multiplier(character: &CharacterView) -> Result<f32, &'static str> {
    let att = observed(character, 20)?;
    let mask = observed(character, 29)? as u32;
    if att < 0 {
        return Err("invalid_att");
    }
    if mask & 4 != 0 {
        return Ok(0.5);
    }
    let mut m = att.checked_add(25).ok_or("invalid_att")? as f32 * 0.02;
    if mask & 0x40000 != 0 {
        m = (f64::from(m) * 1.25) as f32;
    }
    if !m.is_finite() {
        return Err("invalid_damage");
    }
    Ok(m)
}

fn normal_rate(character: &CharacterView) -> Number {
    let dex = observed(character, 28)?;
    if dex < 0 {
        return Err("invalid_dex");
    }
    finite(1.5 + 6.5 * f64::from(dex) / 75.0)
}

fn finite(number: f64) -> Number {
    if number.is_finite() && number >= 0.0 {
        Ok(number)
    } else {
        Err("calculation_overflow")
    }
}

fn exclusions() -> Vec<String> {
    EXCLUSIONS.iter().map(|s| (*s).into()).collect()
}

fn value(number: Number, evidence: CalculationEvidence) -> CharacterValue {
    let number = number.and_then(finite);
    CharacterValue {
        value: number.ok(),
        evidence,
        completeness: if number.is_ok() {
            CalculationCompleteness::Complete
        } else {
            CalculationCompleteness::Unavailable
        },
        reasons: number.err().into_iter().map(String::from).collect(),
        exclusions: if evidence == CalculationEvidence::Modeled {
            exclusions()
        } else {
            vec![]
        },
    }
}

fn range(
    bounds: Result<(i32, i32), &'static str>,
    evidence: CalculationEvidence,
) -> CharacterRange {
    CharacterRange {
        min: bounds.ok().map(|b| b.0),
        max: bounds.ok().map(|b| b.1),
        evidence,
        completeness: if bounds.is_ok() {
            CalculationCompleteness::Complete
        } else {
            CalculationCompleteness::Unavailable
        },
        reasons: bounds.err().into_iter().map(String::from).collect(),
        exclusions: if evidence == CalculationEvidence::Modeled {
            exclusions()
        } else {
            vec![]
        },
    }
}

fn number(value: &CharacterValue) -> Number {
    value.value.ok_or("incomplete_group")
}

fn append_reasons(target: &mut Vec<String>, reasons: &[String]) {
    for reason in reasons {
        // Catalog contracts specify controlled codes. Reject malformed codes
        // rather than returning source descriptions through calculation errors.
        let safe = if !reason.is_empty()
            && reason.len() <= 96
            && reason
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            reason.as_str()
        } else {
            "unsupported_metadata"
        };
        if !target.iter().any(|r| r == safe) {
            target.push(safe.into());
        }
    }
}

fn projectile(item: &ItemDef, index: i16) -> Result<&ProjectileDef, &'static str> {
    if index < 0 {
        return Err("invalid_projectile_reference");
    }
    let mut matches = item.projectiles.iter().filter(|p| p.index == index);
    let projectile = matches.next().ok_or("missing_projectile")?;
    if matches.next().is_some() {
        return Err("ambiguous_projectile_reference");
    }
    Ok(projectile)
}

fn catalog_bounds(projectile: &ProjectileDef) -> Result<(i32, i32), &'static str> {
    if projectile.min_damage < 0 || projectile.max_damage < projectile.min_damage {
        return Err("invalid_damage_bounds");
    }
    Ok((projectile.min_damage, projectile.max_damage))
}

fn hit(roll: i32, multiplier: f32, piercing: bool, defense: u16) -> Result<i32, &'static str> {
    let product = roll as f32 * multiplier;
    // Same f32 product and exclusive conversion ceiling as combat::finite_damage.
    if !product.is_finite() || !(0.0..i32::MAX as f32).contains(&product) {
        return Err("invalid_damage");
    }
    let damage = product as i32;
    Ok(if piercing {
        damage
    } else {
        (damage / 10).max(damage - i32::from(defense))
    })
}

/// Both f32 conversion/multiplication and the defense function are monotone for
/// these nonnegative operands. Equal endpoint hits therefore prove a constant
/// interval. Each visited node consumes budget; failure discards the whole sum.
fn exact_mean(
    low: i32,
    high: i32,
    multiplier: f32,
    piercing: bool,
    defense: u16,
    budget: &mut usize,
) -> Number {
    let mut intervals = vec![(low, high)];
    let mut sum = 0_u64;
    while let Some((lo, hi)) = intervals.pop() {
        *budget = budget.checked_sub(1).ok_or("calculation_work_limit")?;
        let left = hit(lo, multiplier, piercing, defense)?;
        let right = hit(hi, multiplier, piercing, defense)?;
        if left == right {
            let length = (i64::from(hi) - i64::from(lo) + 1) as u64;
            let amount = (left as u64)
                .checked_mul(length)
                .ok_or("calculation_overflow")?;
            sum = sum.checked_add(amount).ok_or("calculation_overflow")?;
        } else {
            let mid = lo + (hi - lo) / 2;
            intervals.push((mid + 1, hi));
            intervals.push((lo, mid));
        }
    }
    // Only this final conversion/division rounds to the DTO's f64 precision.
    finite(sum as f64 / (i64::from(high) - i64::from(low) + 1) as f64)
}

fn group(
    character: &CharacterView,
    item: &ItemDef,
    definition: &WeaponAttackGroup,
    defense: u16,
    blocked: Option<&'static str>,
    rate_blocked: Option<&'static str>,
    budget: &mut usize,
) -> CharacterAttackGroupCalculation {
    let p = projectile(item, definition.projectile_index);
    let bounds = p.and_then(catalog_bounds);
    let adjusted = blocked
        .map_or_else(|| multiplier(character), Err)
        .and_then(|m| {
            let (lo, max) = bounds?;
            let hi = if lo == max { max } else { max - 1 };
            let p = p?;
            Ok((lo, hi, m, p.armor_piercing))
        });
    let hits = adjusted.and_then(|(lo, hi, m, piercing)| {
        Ok((
            hit(lo, m, piercing, defense)?,
            hit(hi, m, piercing, defense)?,
        ))
    });
    let mean = hits.and_then(|_| {
        let (lo, hi, m, piercing) = adjusted?;
        exact_mean(lo, hi, m, piercing, defense, budget)
    });
    let factor = if definition.rate_factor.is_finite() && definition.rate_factor > 0.0 {
        Ok(definition.rate_factor)
    } else {
        Err("invalid_rate_factor")
    };
    let attacks = blocked.or(rate_blocked).map_or_else(
        || {
            if definition.schedule != WeaponSchedule::Simultaneous {
                return Err("unsupported_schedule");
            }
            finite(normal_rate(character)? * factor?)
        },
        Err,
    );
    let projectiles = attacks.and_then(|attacks| {
        if definition.count == 0 {
            return Err("invalid_projectile_count");
        }
        finite(attacks * f64::from(definition.count))
    });
    let dps = mean.and_then(|mean| finite(mean * projectiles?));
    let mut result = CharacterAttackGroupCalculation {
        group_id: definition.group_id.clone(),
        projectile_index: definition.projectile_index,
        armor_piercing: p.ok().map(|p| p.armor_piercing),
        count: definition.count,
        rate_factor: value(factor, CalculationEvidence::Catalog),
        catalog_range: range(bounds, CalculationEvidence::Catalog),
        hit_range: range(hits, CalculationEvidence::Modeled),
        hit_mean: value(mean, CalculationEvidence::Modeled),
        attacks_per_sec: value(attacks, CalculationEvidence::Modeled),
        projectiles_per_sec: value(projectiles, CalculationEvidence::Modeled),
        dps: value(dps, CalculationEvidence::Modeled),
    };
    for value in [
        &mut result.attacks_per_sec,
        &mut result.projectiles_per_sec,
        &mut result.dps,
    ] {
        if value.value.is_none() {
            append_reasons(&mut value.reasons, &definition.reasons);
        }
    }
    // Preserve the actual upstream reasons instead of reducing missing rate or
    // damage evidence to an unexplained unavailable DPS.
    append_reasons(&mut result.dps.reasons, &result.hit_mean.reasons);
    append_reasons(&mut result.dps.reasons, &result.projectiles_per_sec.reasons);
    result
}

fn common_value(
    groups: &[CharacterAttackGroupCalculation],
    get: impl Fn(&CharacterAttackGroupCalculation) -> &CharacterValue,
) -> CharacterValue {
    let first = get(&groups[0]);
    if groups.iter().all(|g| get(g) == first) {
        first.clone()
    } else {
        value(Err("heterogeneous_groups"), first.evidence)
    }
}

fn common_range(
    groups: &[CharacterAttackGroupCalculation],
    get: impl Fn(&CharacterAttackGroupCalculation) -> &CharacterRange,
) -> CharacterRange {
    let first = get(&groups[0]);
    if groups.iter().all(|g| get(g) == first) {
        first.clone()
    } else {
        range(Err("heterogeneous_groups"), first.evidence)
    }
}

fn weapon(
    character: &CharacterView,
    catalog: Option<&Catalog>,
    item_type: Option<u16>,
    defense: u16,
    blocked: Option<&'static str>,
) -> CharacterWeaponCalculation {
    let item = catalog.ok_or("missing_catalog").and_then(|catalog| {
        let id = item_type.ok_or("missing_weapon")?;
        let mut items = catalog.items.iter().filter(|i| i.object_type == id);
        let item = items.next().ok_or("missing_item")?;
        if items.next().is_some() {
            return Err("ambiguous_item");
        }
        Ok(item)
    });
    let firing = item.and_then(|i| i.weapon_firing.as_ref().ok_or("missing_firing_metadata"));
    let metadata = firing.and_then(|f| {
        if f.groups.is_empty() {
            return Err("missing_attack_groups");
        }
        if f.groups.len() > GROUP_LIMIT || item?.projectiles.len() > GROUP_LIMIT {
            return Err("catalog_work_limit");
        }
        Ok(f)
    });
    let unavailable = blocked.or(metadata.err()).unwrap_or("incomplete_weapon");
    let unknown = || value(Err(unavailable), CalculationEvidence::Modeled);
    let mut result = CharacterWeaponCalculation {
        item_type,
        groups: vec![],
        applied_att: observed(character, 20).ok(),
        applied_dex: observed(character, 28).ok(),
        applied_primary_mask: observed(character, 29).ok().map(|mask| mask as u32),
        rate_factor: value(Err(unavailable), CalculationEvidence::Catalog),
        catalog_range: range(Err(unavailable), CalculationEvidence::Catalog),
        hit_range: range(Err(unavailable), CalculationEvidence::Modeled),
        hit_mean: unknown(),
        attacks_per_sec: unknown(),
        projectiles_per_sec: unknown(),
        dps_subtotal: unknown(),
        dps_total: unknown(),
    };
    let (Ok(item), Ok(firing)) = (item, metadata) else {
        return result;
    };
    let mut budget = WORK_LIMIT;
    let mut reasons = Vec::new();
    if firing.support != CatalogSupport::Supported {
        reasons.push("incomplete_firing_metadata".into());
    }
    if !firing.omitted_projectile_indices.is_empty() {
        reasons.push("omitted_projectiles".into());
    }
    append_reasons(&mut reasons, &firing.reasons);
    for definition in &firing.groups {
        let duplicate = firing
            .groups
            .iter()
            .filter(|g| g.group_id == definition.group_id)
            .count()
            != 1;
        let group_blocked = blocked.or_else(|| {
            if duplicate || definition.group_id.is_empty() {
                Some("invalid_attack_group_id")
            } else if firing.support == CatalogSupport::Unsupported {
                Some("unsupported_firing_metadata")
            } else {
                None
            }
        });
        result.groups.push(group(
            character,
            item,
            definition,
            defense,
            blocked,
            group_blocked,
            &mut budget,
        ));
    }
    let groups = &result.groups;
    result.rate_factor = common_value(groups, |g| &g.rate_factor);
    result.catalog_range = common_range(groups, |g| &g.catalog_range);
    result.hit_range = common_range(groups, |g| &g.hit_range);
    result.hit_mean = common_value(groups, |g| &g.hit_mean);
    result.attacks_per_sec = common_value(groups, |g| &g.attacks_per_sec);
    result.projectiles_per_sec = value(
        groups
            .iter()
            .try_fold(0.0, |sum, g| finite(sum + number(&g.projectiles_per_sec)?)),
        CalculationEvidence::Modeled,
    );
    // The projectile sum also cannot claim a complete rate for omitted groups.
    if !reasons.is_empty() {
        result.projectiles_per_sec = value(
            Err("incomplete_firing_metadata"),
            CalculationEvidence::Modeled,
        );
    }
    let mut sum = None;
    let mut overflow = false;
    for group in groups {
        append_reasons(&mut reasons, &group.dps.reasons);
        if let Some(dps) = group.dps.value {
            match finite(sum.unwrap_or(0.0) + dps) {
                Ok(total) => sum = Some(total),
                Err(_) => overflow = true,
            }
        }
    }
    if overflow {
        sum = None;
        reasons.push("calculation_overflow".into());
    }
    // A metadata omission permits a supported subtotal; an exhausted numeric
    // calculation must not expose whichever groups happened to run first.
    if reasons
        .iter()
        .any(|reason| reason == "calculation_work_limit")
    {
        sum = None;
    }
    result.dps_subtotal = value(
        sum.ok_or("no_supported_groups"),
        CalculationEvidence::Modeled,
    );
    if !reasons.is_empty() {
        if sum.is_some() {
            result.dps_subtotal.completeness = CalculationCompleteness::Partial;
        }
        append_reasons(&mut result.dps_subtotal.reasons, &reasons);
        result.dps_total = value(Err("incomplete_weapon"), CalculationEvidence::Modeled);
        append_reasons(&mut result.dps_total.reasons, &reasons);
    } else {
        result.dps_total = result.dps_subtotal.clone();
    }
    result
}

// These controlled identifiers supply the HP/MP included/excluded captions.
// Base rows are deliberately outside the modeled contribution totals.
fn regen_value(number: Number, resource: CharacterRegenResource) -> CharacterValue {
    let mut result = value(number, CalculationEvidence::Modeled);
    result.exclusions = match resource {
        CharacterRegenResource::Hp => &[
            "base_vit",
            "pets",
            "healing_effects",
            "combat_state_modifiers",
        ][..],
        CharacterRegenResource::Mp => &[
            "base_mp_regeneration",
            "pets",
            "consumables",
            "ability_costs",
            "combat_state_modifiers",
        ][..],
    }
    .iter()
    .map(|s| (*s).into())
    .collect();
    result
}

fn regen_sum<'a>(
    components: impl Iterator<Item = &'a CharacterValue>,
    gaps: &[String],
    resource: CharacterRegenResource,
) -> (CharacterValue, CharacterValue) {
    let mut reasons = gaps.to_vec();
    let mut sum = None;
    for component in components {
        if component.completeness != CalculationCompleteness::Complete {
            append_reasons(&mut reasons, &component.reasons);
        }
        if let Some(amount) = component.value {
            match finite(sum.unwrap_or(0.0) + amount) {
                Ok(total) => sum = Some(total),
                Err(reason) => append_reasons(&mut reasons, &[reason.into()]),
            }
        }
    }
    if reasons.is_empty() {
        // An exhaustive empty list is known zero, unlike an unresolved effect.
        let total = regen_value(Ok(sum.unwrap_or(0.0)), resource);
        return (total.clone(), total);
    }
    if reasons.iter().any(|r| r == "calculation_overflow") {
        sum = None;
    }
    let mut subtotal = regen_value(sum.ok_or("no_supported_regeneration"), resource);
    if sum.is_some() {
        subtotal.completeness = CalculationCompleteness::Partial;
    }
    append_reasons(&mut subtotal.reasons, &reasons);
    let mut total = regen_value(Err("incomplete_regeneration"), resource);
    append_reasons(&mut total.reasons, &reasons);
    (subtotal, total)
}

fn regen_effect(
    character: &CharacterView,
    slot: u8,
    enchant_id: i32,
    resource: CharacterRegenResource,
    effect: &RegenEffect,
) -> CharacterRegenContribution {
    let maximum = observed(
        character,
        match resource {
            CharacterRegenResource::Hp => 0,
            CharacterRegenResource::Mp => 3,
        },
    )
    .map_err(|_| "missing_or_ambiguous_maximum")
    .and_then(|v| {
        if v >= 0 {
            Ok(v)
        } else {
            Err("invalid_maximum")
        }
    });
    let coefficient = |n: Option<f64>| {
        value(
            n.ok_or("coefficient_not_applicable")
                .and_then(|n| finite(n).map_err(|_| "invalid_regen_coefficient")),
            CalculationEvidence::Catalog,
        )
    };
    let flat = coefficient(effect.flat_per_sec);
    let fraction = coefficient(effect.fraction_of_max_per_sec);
    let invalid = (effect.flat_per_sec.is_none() && effect.fraction_of_max_per_sec.is_none())
        || effect.flat_per_sec.is_some_and(|_| flat.value.is_none())
        || effect
            .fraction_of_max_per_sec
            .is_some_and(|_| fraction.value.is_none());
    let (subtotal, total) = if invalid {
        let unknown = regen_value(Err("invalid_regen_coefficient"), resource);
        (unknown.clone(), unknown)
    } else {
        let mut parts = Vec::with_capacity(2);
        if let Some(n) = flat.value {
            parts.push(regen_value(Ok(n), resource));
        }
        if let Some(n) = fraction.value {
            // A known zero coefficient has no dependency on an unknown maximum.
            let amount = if n == 0.0 {
                Ok(0.0)
            } else {
                maximum.and_then(|m| finite(n * f64::from(m)))
            };
            parts.push(regen_value(amount, resource));
        }
        regen_sum(parts.iter(), &[], resource)
    };
    CharacterRegenContribution {
        source_slot: slot,
        enchant_id,
        resource,
        applied_maximum: maximum.ok(),
        flat_per_sec: flat,
        fraction_of_max_per_sec: fraction,
        source_units: effect.source_units.clone(),
        subtotal,
        total,
    }
}

fn regen_slot<'a>(
    character: &'a CharacterView,
    catalog: Option<&Catalog>,
    slot: u8,
) -> Result<&'a [i32], &'static str> {
    let mut slots = character.equipment.iter().filter(|e| e.slot == slot);
    let equipment = slots.next().ok_or("missing_equipment_slot")?;
    if slots.next().is_some() {
        return Err("ambiguous_equipment_slot");
    }
    if equipment.observed_empty == Some(true) {
        return if equipment.item_type.is_none() {
            Ok(&[])
        } else {
            Err("invalid_equipment_slot")
        };
    }
    let item_type = equipment.item_type.ok_or("missing_equipment_slot")?;
    let catalog = catalog.ok_or("missing_catalog")?;
    let mut items = catalog
        .items
        .iter()
        .filter(|item| item.object_type == item_type);
    items.next().ok_or("missing_item")?;
    if items.next().is_some() {
        return Err("ambiguous_item");
    }
    let enchant = &equipment.enchant;
    match enchant.state {
        EnchantState::Missing => return Err("missing_enchants"),
        EnchantState::Invalid => return Err("invalid_enchants"),
        EnchantState::Recorded | EnchantState::RecordedEmpty => {}
    }
    // The observation parser retains empty/locked markers (-1..=-3).
    // Do not resolve them as IDs or reinterpret display strings.
    let applied = enchant.ids.iter().filter(|&&id| id >= 0).count();
    if enchant.ids.len() > 4
        || enchant.ids.iter().any(|&id| id < -3)
        || enchant
            .ids
            .iter()
            .position(|&id| id == -3)
            .is_some_and(|i| i + 1 != enchant.ids.len())
        || enchant.applied.map(usize::from) != Some(applied)
        || enchant
            .slots
            .is_none_or(|slots| usize::from(slots) < applied || slots > 4)
        || (enchant.state == EnchantState::RecordedEmpty) != (applied == 0)
    {
        return Err("invalid_enchants");
    }
    Ok(&enchant.ids)
}

fn regeneration(character: &CharacterView, catalog: Option<&Catalog>) -> CharacterRegeneration {
    use CharacterRegenResource::{Hp, Mp};
    let applied_wis = observed(character, 27)
        .map_err(|_| "missing_or_ambiguous_wis")
        .and_then(|wis| {
            if wis >= 0 {
                Ok(wis)
            } else {
                Err("invalid_wis")
            }
        });
    let mut wis = regen_value(applied_wis.map(|wis| 0.12 * f64::from(wis)), Mp);
    wis.reasons.push("wis_reference_model".into());
    let mut effects = vec![];
    let mut gaps = vec![];
    for slot in 0..4 {
        let ids = match regen_slot(character, catalog, slot) {
            Ok(ids) => ids,
            Err(reason) => {
                append_reasons(&mut gaps, &[reason.into()]);
                continue;
            }
        };
        for &id in ids.iter().filter(|&&id| id >= 0) {
            let mut definitions = catalog
                .into_iter()
                .flat_map(|c| &c.enchants)
                .filter(|e| e.id == id);
            let definition = definitions.next();
            let metadata = definition.and_then(|e| e.regeneration.as_ref());
            let reason = if definitions.next().is_some() {
                Some("ambiguous_enchant")
            } else if definition.is_none() {
                Some("unresolved_enchant")
            } else if metadata.is_none() {
                Some("missing_regen_metadata")
            } else {
                None
            };
            if let Some(reason) = reason {
                append_reasons(&mut gaps, &[reason.into()]);
                continue;
            }
            let metadata = metadata.expect("checked regen metadata");
            if metadata.support != CatalogSupport::Supported || !metadata.reasons.is_empty() {
                append_reasons(&mut gaps, &["unsupported_regen_effect".into()]);
            }
            if metadata.support == CatalogSupport::Unsupported {
                continue;
            }
            if metadata.hp.len().saturating_add(metadata.mp.len()) > GROUP_LIMIT {
                append_reasons(&mut gaps, &["calculation_work_limit".into()]);
                continue;
            }
            for (resource, definitions) in [(Hp, &metadata.hp), (Mp, &metadata.mp)] {
                effects.extend(
                    definitions
                        .iter()
                        .map(|effect| regen_effect(character, slot, id, resource, effect)),
                );
            }
        }
    }
    let (hp_subtotal, hp_total) = regen_sum(
        effects
            .iter()
            .filter(|e| e.resource == Hp)
            .map(|e| &e.subtotal),
        &gaps,
        Hp,
    );
    let (mp_subtotal, mp_total) = regen_sum(
        std::iter::once(&wis).chain(
            effects
                .iter()
                .filter(|e| e.resource == Mp)
                .map(|e| &e.subtotal),
        ),
        &gaps,
        Mp,
    );
    CharacterRegeneration {
        applied_wis: applied_wis.ok(),
        wis,
        effects,
        hp_subtotal,
        mp_subtotal,
        hp_total,
        mp_total,
        base_vit: regen_value(Err("base_vit_unavailable_excluded"), Hp),
        base_mp: regen_value(Err("base_mp_regeneration_unavailable_excluded"), Mp),
    }
}

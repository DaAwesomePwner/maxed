use crate::{
    xml::{self, num, Node},
    AssetError, ExtractedAssets, SheetInfo,
};
use maxed_types::*;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod d_tests;
mod weapon_damage;

enum ConsumableUtRule {
    NameSuffix(&'static str),
    ObjectType(u16),
}

// Bump cache::CACHE_VERSION when changing these catalog rules.
const CONSUMABLE_UT_ALLOWLIST: &[ConsumableUtRule] = &[
    ConsumableUtRule::NameSuffix(" Blueprint"),
    // Keep both Vial of Soul Extract types pending the owner's Penitentiary confirmation.
    ConsumableUtRule::ObjectType(5338),
    ConsumableUtRule::ObjectType(23743),
    ConsumableUtRule::ObjectType(49473), // Kogbold Enhancement Core
    ConsumableUtRule::ObjectType(3984),  // Shiny Kogbold Enhancement Core
];

fn duplicate(section: &'static str, id: i64) -> AssetError {
    tracing::warn!(section, definition_id = id, "duplicate catalog definition");
    AssetError::InvalidXmlSection { section }
}

fn enchant_stat_multiplier(n: &Node) -> Result<f64, AssetError> {
    // ParseEnchants.prepareReload: multiply StatModMult amounts in XML order,
    // with a float accumulator; unrelated mutators do not affect stat damage.
    let mut multiplier = 1.0f32;
    for activation in n
        .children
        .iter()
        .filter(|n| n.tag == "Mutators")
        .flat_map(|n| &n.children)
        .filter(|n| n.tag == "ActivateOnEquip")
    {
        if activation.text.trim() == "StatModMult" {
            if let Some(amount) = activation
                .attr("amount")
                .and_then(|v| v.parse::<f32>().ok())
            {
                multiplier *= amount;
            }
        }
    }
    if !multiplier.is_finite() || multiplier < 0.0 {
        return Err(AssetError::InvalidCatalog);
    }
    Ok(f64::from(multiplier))
}

// Gate X evidence and intentionally unsupported shapes: docs/architecture/assets.md.
const FIRING_REASONS: &[&str] = &[
    "invalid_rate",
    "invalid_count",
    "missing_projectile_reference",
    "unresolved_projectile",
    "unreferenced_projectile",
    "unsupported_schedule",
];
const REGEN_REASONS: &[&str] = &["unsupported_regen_effect"];
const REGEN_UNITS: &[&str] = &["fraction_of_max_per_second", "units_per_second"];

fn support(known: bool, reasons: &[String]) -> CatalogSupport {
    if reasons.is_empty() {
        CatalogSupport::Supported
    } else if known {
        CatalogSupport::Partial
    } else {
        CatalogSupport::Unsupported
    }
}

fn reason(reasons: &mut Vec<String>, code: &str) {
    if !reasons.iter().any(|r| r == code) {
        reasons.push(code.into());
        reasons.sort();
    }
}

// Reject duplicate scalars and extensions rather than silently choosing a value.
fn scalar<'a>(n: &'a Node, tag: &str) -> Result<Option<&'a str>, ()> {
    let mut nodes = n.children.iter().filter(|c| c.tag == tag);
    let Some(value) = nodes.next() else {
        return Ok(None);
    };
    if nodes.next().is_some() || !value.attrs.is_empty() || !value.children.is_empty() {
        return Err(());
    }
    Ok(Some(value.text.trim()))
}

fn firing_rate(n: &Node) -> Option<f64> {
    scalar(n, "RateOfFire")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
}

fn weapon_firing(
    n: &Node,
    projectile_nodes: &[&Node],
    projectiles: &BTreeMap<i16, ProjectileDef>,
) -> Result<Option<WeaponFiringDef>, AssetError> {
    let subattacks: Vec<_> = n.children.iter().filter(|c| c.tag == "Subattack").collect();
    if subattacks.len() > 256 {
        return Err(AssetError::LimitExceeded);
    }
    if projectile_nodes.is_empty() && subattacks.is_empty() && n.child("RateOfFire").is_none() {
        return Ok(None);
    }
    // A deliberately narrow normal-fire shape. Unreviewed item mechanics disable all schedules.
    let normal_shape = n.attrs.keys().all(|k| matches!(k.as_str(), "id" | "type"))
        && n.children.iter().all(|c| {
            matches!(
                c.tag.as_str(),
                "Class"
                    | "DisplayId"
                    | "Description"
                    | "SlotType"
                    | "Tier"
                    | "Labels"
                    | "Texture"
                    | "AnimatedTexture"
                    | "Soulbound"
                    | "RateOfFire"
                    | "NumProjectiles"
                    | "Projectile"
                    | "Subattack"
            )
        });
    let explicit: BTreeSet<i16> = projectile_nodes
        .iter()
        .filter_map(|p| p.attr("id").and_then(|id| num(id).ok()))
        .collect();
    let indices: BTreeSet<i16> = projectile_nodes
        .iter()
        .enumerate()
        .map(|(i, p)| {
            p.attr("id")
                .map(num)
                .transpose()
                .map(|id| id.unwrap_or(i as i16))
        })
        .collect::<Result<_, _>>()?;
    let mut f = WeaponFiringDef {
        subattack_projectile_indices: Some(
            subattacks
                .iter()
                .map(|attack| {
                    attack
                        .attr("projectileId")
                        .and_then(|s| num::<i16>(s).ok())
                        .filter(|v| *v >= 0)
                })
                .collect(),
        ),
        support: CatalogSupport::Unsupported,
        groups: vec![],
        omitted_projectile_indices: vec![],
        reasons: vec![],
    };
    let mut omitted = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    let implicit = subattacks.is_empty();
    let attacks = if implicit { vec![n] } else { subattacks };
    for (i, attack) in attacks.into_iter().enumerate() {
        let index = if implicit {
            Some(0)
        } else {
            attack
                .attr("projectileId")
                .and_then(|s| num::<i16>(s).ok())
                .filter(|v| *v >= 0)
        };
        let Some(index) = index else {
            reason(&mut f.reasons, "missing_projectile_reference");
            continue;
        };
        referenced.insert(index);
        if !explicit.contains(&index) || !projectiles.contains_key(&index) {
            omitted.insert(index);
            reason(&mut f.reasons, "unresolved_projectile");
            continue;
        }
        let rate = if !implicit && attack.child("RateOfFire").is_none() {
            firing_rate(n)
        } else {
            firing_rate(attack)
        };
        let Some(rate_factor) = rate else {
            reason(&mut f.reasons, "invalid_rate");
            continue;
        };
        let count = match scalar(attack, "NumProjectiles") {
            Ok(Some(s)) => num::<u16>(s).ok().filter(|v| (1..=256).contains(v)),
            Ok(None) if implicit => Some(1),
            _ => None,
        };
        let Some(count) = count else {
            reason(&mut f.reasons, "invalid_count");
            continue;
        };
        let normal_group = implicit
            || (attack.attrs.keys().all(|k| k == "projectileId")
                && attack.text.trim().is_empty()
                && attack
                    .children
                    .iter()
                    .all(|c| matches!(c.tag.as_str(), "RateOfFire" | "NumProjectiles")));
        let mut group = WeaponAttackGroup {
            group_id: format!("attack_{i}"),
            projectile_index: index,
            count,
            rate_factor,
            schedule: WeaponSchedule::Simultaneous,
            reasons: vec![],
        };
        if !normal_shape || !normal_group {
            group.schedule = WeaponSchedule::Unsupported;
            reason(&mut group.reasons, "unsupported_schedule");
            reason(&mut f.reasons, "unsupported_schedule");
        }
        f.groups.push(group);
    }
    for index in indices.difference(&referenced) {
        omitted.insert(*index);
        reason(&mut f.reasons, "unreferenced_projectile");
    }
    f.omitted_projectile_indices = omitted.into_iter().collect();
    f.support = support(
        f.groups
            .iter()
            .any(|g| g.schedule == WeaponSchedule::Simultaneous),
        &f.reasons,
    );
    Ok(Some(f))
}

fn enchant_regeneration(n: &Node) -> Result<EnchantRegenDef, AssetError> {
    let mut r = EnchantRegenDef {
        support: CatalogSupport::Supported,
        hp: vec![],
        mp: vec![],
        reasons: vec![],
        source_units: vec![],
    };
    if n.children
        .iter()
        .any(|c| !matches!(c.tag.as_str(), "DisplayId" | "Description" | "Mutators"))
        || n.attrs.keys().any(|k| !matches!(k.as_str(), "type" | "id"))
    {
        reason(&mut r.reasons, "unsupported_regen_effect");
    }
    for container in n.children.iter().filter(|c| c.tag == "Mutators") {
        if !container.attrs.is_empty() || !container.text.trim().is_empty() {
            reason(&mut r.reasons, "unsupported_regen_effect");
            continue;
        }
        for effect in &container.children {
            // These reviewed mutators affect other domains, not baseline regeneration.
            if matches!(
                effect.tag.as_str(),
                "MultiplyMinDamage" | "MultiplyMaxDamage" | "MultiplyRateOfFire"
            ) && effect.attrs.is_empty()
                && effect.children.is_empty()
            {
                continue;
            }
            let kind = effect.text.trim();
            if effect.tag == "ActivateOnEquip"
                && matches!(kind, "LootBonus" | "StatModMult")
                && effect.children.is_empty()
                && effect.attrs.keys().all(|k| k == "amount")
            {
                continue;
            }
            let amount = effect
                .attr("amount")
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0);
            if effect.tag != "ActivateOnEquip"
                || !matches!(kind, "FlatRegen" | "PercentageRegen")
                || !matches!(effect.attr("stat"), Some("HP" | "MP"))
                || amount.is_none()
                || !effect.children.is_empty()
                || effect
                    .attrs
                    .keys()
                    .any(|k| !matches!(k.as_str(), "stat" | "amount"))
            {
                reason(&mut r.reasons, "unsupported_regen_effect");
                continue;
            }
            if r.hp.len() + r.mp.len() >= 256 {
                return Err(AssetError::LimitExceeded);
            }
            let flat = kind == "FlatRegen";
            let units = if flat {
                "units_per_second"
            } else {
                "fraction_of_max_per_second"
            };
            reason(&mut r.source_units, units);
            let normalized = RegenEffect {
                flat_per_sec: if flat { amount } else { None },
                fraction_of_max_per_sec: if flat { None } else { amount },
                source_units: vec![units.into()],
            };
            if effect.attr("stat") == Some("HP") {
                r.hp.push(normalized);
            } else {
                r.mp.push(normalized);
            }
        }
    }
    r.support = support(!r.hp.is_empty() || !r.mp.is_empty(), &r.reasons);
    Ok(r)
}

pub fn parse_equipment(xml: &[u8]) -> Result<Vec<ItemDef>, AssetError> {
    let mut out = BTreeMap::new();
    xml::records(xml, &mut |n| {
        if n.tag == "Object" {
            if let Some(item) = item(&n, &BTreeMap::new())? {
                let object_type = item.object_type;
                if out.insert(object_type, item).is_some() {
                    return Err(duplicate("equipment", i64::from(object_type)));
                }
            }
        }
        Ok(())
    })
    .map_err(|e| match e {
        AssetError::InvalidXml => AssetError::InvalidXmlSection {
            section: "equipment",
        },
        other => other,
    })?;
    Ok(out.into_values().collect())
}

fn id(n: &Node) -> Result<u16, AssetError> {
    num(n.attr("type").ok_or(AssetError::InvalidCatalog)?)
}
fn display<'a>(n: &'a Node, english: &'a BTreeMap<String, String>) -> Option<&'a str> {
    n.value("DisplayId").and_then(|value| {
        match value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
            Some(key) => english
                .get(key)
                .map(String::as_str)
                .filter(|v| !v.is_empty())
                .or(n.attr("id")),
            None => Some(value),
        }
    })
}
fn name(n: &Node, english: &BTreeMap<String, String>) -> Result<String, AssetError> {
    let name = display(n, english)
        .filter(|v| !v.is_empty())
        .or(n.attr("id"))
        .ok_or(AssetError::InvalidCatalog)?;
    if name.len() > 4096 {
        return Err(AssetError::LimitExceeded);
    }
    Ok(name.into())
}
fn sprite(n: &Node) -> Result<Option<String>, AssetError> {
    let Some(t) = n.child("Texture").or_else(|| n.child("AnimatedTexture")) else {
        return Ok(None);
    };
    match (t.value("File"), t.value("Index")) {
        (Some(file), Some(index)) => {
            if file.len() > 200 {
                return Err(AssetError::LimitExceeded);
            }
            Ok(Some(format!("{}:{}", file, num::<u32>(index)?)))
        }
        _ => Ok(None),
    }
}
fn item(n: &Node, english: &BTreeMap<String, String>) -> Result<Option<ItemDef>, AssetError> {
    let projectile_nodes: Vec<_> = n
        .children
        .iter()
        .filter(|c| c.tag == "Projectile")
        .collect();
    if !matches!(n.value("Class"), Some("Equipment" | "Projectile")) && projectile_nodes.is_empty()
    {
        return Ok(None);
    }
    if projectile_nodes.len() > 256 {
        return Err(AssetError::LimitExceeded);
    }
    let mut projectiles = BTreeMap::new();
    let mut indices = BTreeSet::new();
    for (i, p) in projectile_nodes.iter().enumerate() {
        let index = p.attr("id").map(num).transpose()?.unwrap_or(i as i16);
        if index < 0 {
            return Err(AssetError::InvalidCatalog);
        }
        if !indices.insert(index) {
            return Err(duplicate("projectiles", i64::from(index)));
        }
        let damage = p.value("Damage");
        let (Some(min), Some(max)) = (
            damage.or(p.value("MinDamage")),
            damage.or(p.value("MaxDamage")),
        ) else {
            continue;
        };
        let min_damage = num(min)?;
        let max_damage = num(max)?;
        if min_damage < 0 || min_damage > max_damage || index < 0 {
            return Err(AssetError::InvalidCatalog);
        }
        let lifetime_millis = if let Some(s) = p.value("LifetimeMS") {
            let v: f64 = s.parse().map_err(|_| AssetError::InvalidCatalog)?;
            if !v.is_finite() || v < 0.0 || v > u32::MAX as f64 {
                return Err(AssetError::InvalidCatalog);
            }
            // Catalog v1 cannot express fractional milliseconds. Keep unavailable.
            (v.fract() == 0.0).then_some(v as u32)
        } else {
            None
        };
        let def = ProjectileDef {
            index,
            min_damage,
            max_damage,
            armor_piercing: p.child("ArmorPiercing").is_some(),
            lifetime_millis,
        };
        if projectiles.insert(index, def).is_some() {
            return Err(duplicate("projectiles", i64::from(index)));
        }
    }
    let object_type = id(n)?;
    let name = name(n, english)?;
    let slot_type = n.value("SlotType").map(num).transpose()?;
    let tier = n.value("Tier").map(str::to_owned).or_else(|| {
        if slot_type == Some(10) {
            return CONSUMABLE_UT_ALLOWLIST
                .iter()
                .any(|rule| match rule {
                    ConsumableUtRule::NameSuffix(suffix) => name.ends_with(suffix),
                    ConsumableUtRule::ObjectType(id) => object_type == *id,
                })
                .then(|| "UT".into());
        }
        n.value("Labels").and_then(|s| {
            s.split(',')
                .map(str::trim)
                .find(|s| matches!(*s, "UT" | "ST"))
                .map(str::to_owned)
        })
    });
    let mut scaling = None;
    for tag in ["Activate", "OnConditionEndActivate"] {
        for act in n.descendants().filter(|c| c.tag == tag) {
            if scaling.is_none() {
                scaling = parse_scaling(act)?;
            }
        }
    }
    let (bonuses, unknown_stats) = parse_item_stat_bonuses(n)?;
    if unknown_stats > 0 {
        tracing::warn!(unknown_stats, "unsupported equipment bonus stats omitted");
    }
    Ok(Some(ItemDef {
        weapon_firing: weapon_firing(n, &projectile_nodes, &projectiles)?,
        potion_stat: crate::rules::potion_stat(&name),
        soulbound: Some(parse_soulbound(n)),
        stat_bonuses: Some(
            bonuses
                .into_iter()
                .map(|(stat_id, value)| StatView { stat_id, value })
                .collect(),
        ),
        object_type,
        name,
        tier,
        slot_type,
        projectiles: projectiles.into_values().collect(),
        scaling,
        sprite_key: sprite(n)?,
    }))
}
fn parse_scaling(n: &Node) -> Result<Option<ScalingDef>, AssetError> {
    let Some(stat) = n.attr("scalingStat") else {
        return Ok(None);
    };
    let Some(stat_id) = stat_id(stat) else {
        return Ok(None);
    };
    let parse = |s: &str| -> Result<f32, AssetError> {
        let v: f32 = s.parse().map_err(|_| AssetError::InvalidCatalog)?;
        if !v.is_finite() {
            Err(AssetError::InvalidCatalog)
        } else {
            Ok(v)
        }
    };
    let damage_per_stat = if let Some(s) = n.attr("statModDamage") {
        parse(s)?
    } else if let (Some(a), Some(b)) = (n.attr("statModFlat"), n.attr("statModPerc")) {
        parse(a)? + parse(b)? * 100.0
    } else {
        return Ok(None);
    };
    if !damage_per_stat.is_finite() {
        return Err(AssetError::InvalidCatalog);
    }
    if damage_per_stat <= 0.0 {
        return Ok(None);
    }
    // Documented AbilityScalingManager defaults, not guesses for invalid values.
    let scaling = ScalingDef {
        stat_id,
        minimum: n
            .attr("statModScalingMin")
            .map(num)
            .transpose()?
            .unwrap_or(50),
        damage_per_stat,
        shots: n.attr("numShots").map(num).transpose()?.unwrap_or(1),
        origin_item: None,
    };
    if scaling.shots == 0 || scaling.minimum < 0 {
        return Err(AssetError::InvalidCatalog);
    }
    Ok(Some(scaling))
}
fn stat_id(s: &str) -> Option<u8> {
    match s.to_ascii_uppercase().as_str() {
        "HP" | "HEALTH" | "LIFE" | "MAXHITPOINTS" => Some(0),
        "MP" | "MANA" | "MAXMAGICPOINTS" => Some(3),
        "ATT" | "ATTACK" => Some(20),
        "DEF" | "DEFENSE" => Some(21),
        "SPD" | "SPEED" => Some(22),
        "VIT" | "VITALITY" | "HPREGEN" => Some(26),
        "WIS" | "WISDOM" | "MPREGEN" => Some(27),
        "DEX" | "DEXTERITY" => Some(28),
        _ => None,
    }
}

pub(crate) fn parse_item_stat_bonuses(n: &Node) -> Result<(BTreeMap<u8, i32>, usize), AssetError> {
    let mut bonuses = BTreeMap::<u8, i32>::new();
    let mut unknown = 0;
    for activation in n
        .children
        .iter()
        .filter(|n| n.tag == "ActivateOnEquip" && n.text.trim() == "IncrementStat")
    {
        let stat = activation.attr("stat").and_then(|s| {
            stat_id(s).or_else(|| {
                num::<u8>(s)
                    .ok()
                    .filter(|id| matches!(id, 0 | 3 | 20 | 21 | 22 | 26 | 27 | 28))
            })
        });
        let Some(stat) = stat else {
            unknown += 1;
            continue;
        };
        let amount: i32 = num(activation
            .attr("amount")
            .ok_or(AssetError::InvalidCatalog)?)?;
        let total = bonuses.entry(stat).or_default();
        *total = total
            .checked_add(amount)
            .ok_or(AssetError::InvalidCatalog)?;
    }
    Ok((bonuses, unknown))
}
pub(crate) fn parse_soulbound(n: &Node) -> bool {
    n.child("Soulbound").is_some()
}
pub(crate) fn parse_object_class(n: &Node) -> Option<&str> {
    n.value("Class").filter(|s| !s.is_empty())
}
pub(crate) fn parse_dungeon_name(n: &Node) -> Option<&str> {
    (parse_object_class(n) == Some("Portal"))
        .then(|| n.value("DungeonName").filter(|s| !s.is_empty()))
        .flatten()
}
pub(crate) fn parse_event_boss(
    n: &Node,
    english: &BTreeMap<String, String>,
) -> Result<bool, AssetError> {
    // Encounter is the client XML's realm-event declaration. Controllers and
    // spawners can also carry it, but are permanently invincible helpers.
    Ok((n.value("Class") == Some("Character")
        && n.child("Enemy").is_some()
        && n.child("Encounter").is_some()
        && n.child("Invincible").is_none())
        || crate::rules::event_bosses().contains(&name(n, english)?.as_str()))
}

pub fn build_catalog(
    assets: ExtractedAssets,
    client_fingerprint: String,
) -> Result<Catalog, AssetError> {
    let sheets = assets
        .textures
        .iter()
        .map(|t| SheetInfo {
            name: t.name.clone(),
            width: t.width,
            height: t.height,
        })
        .collect::<Vec<_>>();
    build_catalog_with_sheets(assets, client_fingerprint, &sheets)
}

pub fn build_catalog_with_sheets(
    assets: ExtractedAssets,
    client_fingerprint: String,
    sheets: &[SheetInfo],
) -> Result<Catalog, AssetError> {
    build_cancellable(
        assets,
        client_fingerprint,
        sheets,
        &std::sync::atomic::AtomicBool::new(false),
    )
}
pub(crate) fn build_cancellable(
    mut assets: ExtractedAssets,
    client_fingerprint: String,
    sheets: &[SheetInfo],
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Catalog, AssetError> {
    crate::unity::cancelled(cancel)?;
    let input_bytes = assets
        .texts
        .iter()
        .try_fold(assets.sprite_mapping.len(), |sum, t| {
            sum.checked_add(t.bytes.len())
                .ok_or(AssetError::LimitExceeded)
        })?;
    if input_bytes > 128 * 1024 * 1024 || assets.texts.len() > 1_000_000 {
        return Err(AssetError::LimitExceeded);
    }
    let mut c = Catalog {
        format_version: 1,
        client_fingerprint,
        items: vec![],
        objects: vec![],
        classes: vec![],
        enchants: vec![],
        dungeons: crate::rules::dungeons(),
        bags: crate::rules::bags(),
        sprites: {
            let mut sprites = crate::sprites::parse_sprites(&assets.sprite_mapping, sheets)?;
            sprites.extend(crate::sprites::enchant_pips(sheets));
            sprites.sort_by(|a, b| a.key.cmp(&b.key));
            sprites
        },
        tiles: vec![],
        crucible_bonuses: vec![],
    };
    let mut objects = BTreeMap::new();
    let mut items = BTreeMap::new();
    let mut classes = BTreeMap::new();
    let mut enchants = BTreeMap::new();
    let mut tiles = BTreeMap::new();
    let mut origins = BTreeMap::<u16, BTreeSet<u16>>::new();
    let mut count = 0;
    let mut name_bytes = 0usize;
    let mut projectile_count = 0usize;
    // Conservative retained-data charge includes tree/vector overhead and copies.
    // Combined with the 128 MiB text budget, this leaves room for parser buffers.
    let mut retained = c
        .sprites
        .iter()
        .map(|s| s.key.len() + s.sheet.len() + 192)
        .sum::<usize>();
    let reviewed: BTreeSet<String> = c.dungeons.iter().map(|d| d.name.clone()).collect();
    let mut templates = BTreeSet::new();
    let mut portals = Vec::new();
    // IdToAsset applies later records over earlier records. Files are ordered
    // explicitly here (source name, original same-name sequence), not by filesystem.
    assets.texts.sort_by(|a, b| a.name.cmp(&b.name));
    for text in assets.texts {
        crate::unity::cancelled(cancel)?;
        let head = text
            .bytes
            .strip_prefix(&[239, 187, 191])
            .unwrap_or(&text.bytes);
        if !head
            .iter()
            .copied()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|b| b == b'<')
        {
            continue;
        }
        let mut seen = BTreeSet::new();
        xml::records(head, &mut |n| {
            crate::unity::cancelled(cancel)?;
            count += 1;
            if count > 100_000 {
                return Err(AssetError::LimitExceeded);
            }
            let localized_charge = display(&n, &assets.english).map_or(0, str::len);
            let string_charge = localized_charge
                + n.attr("id").map_or(0, str::len)
                + ["DisplayId", "Description", "DungeonName", "Tier", "Class"]
                    .iter()
                    .filter(|tag| **tag != "Description" || n.tag == "Enchantment")
                    .map(|tag| n.value(tag).map_or(0, str::len))
                    .sum::<usize>()
                + n.child("Texture")
                    .or_else(|| n.child("AnimatedTexture"))
                    .and_then(|t| t.value("File"))
                    .map_or(0, str::len);
            let child_charge = n
                .descendants()
                .filter(|n| {
                    matches!(
                        n.tag.as_str(),
                        "Projectile"
                            | "Activate"
                            | "OnPlayerShootActivate"
                            | "Subattack"
                            | "ActivateOnEquip"
                    )
                })
                .count()
                * 512;
            // Includes map/vector overhead, shared item/object names, references,
            // and even overwritten definitions; ignored XML subtrees are freed.
            retained = retained
                .checked_add(string_charge * 2 + child_charge + 512)
                .ok_or(AssetError::LimitExceeded)?;
            if retained > 64 * 1024 * 1024 {
                return Err(AssetError::LimitExceeded);
            }
            if matches!(n.tag.as_str(), "Object" | "Ground") {
                let raw = xml::integer(n.attr("type").ok_or(AssetError::InvalidCatalog)?)?;
                if u16::try_from(raw).is_err() {
                    tracing::warn!(
                        object_type = raw,
                        "definition outside the wire type range omitted"
                    );
                    return Ok(());
                }
            }
            if n.tag == "Object" {
                let object_type = id(&n)?;
                if !seen.insert((0, object_type)) {
                    return Err(duplicate("objects", i64::from(object_type)));
                }
                let label = n.value("Labels").unwrap_or("");
                let is_player = n.value("Class") == Some("Player");
                let name = name(&n, &assets.english)?;
                let event_boss = parse_event_boss(&n, &assets.english)?;
                name_bytes += name.len();
                if name_bytes > 16 * 1024 * 1024 {
                    return Err(AssetError::LimitExceeded);
                }
                objects.insert(
                    object_type,
                    ObjectDef {
                        class_name: parse_object_class(&n).map(str::to_owned),
                        dungeon_name: parse_dungeon_name(&n).map(str::to_owned),
                        event_boss: Some(event_boss),
                        realm_event_key: event_boss
                            .then(|| {
                                n.attr("id")
                                    .filter(|id| !id.is_empty())
                                    .map(|id| id.replace(' ', "_"))
                            })
                            .flatten(),
                        object_type,
                        name,
                        is_player,
                        is_boss: label.split(',').any(|s| s.trim() == "BOSS"),
                        is_miniboss: label.split(',').any(|s| s.trim() == "MINIBOSS"),
                        sprite_key: sprite(&n)?,
                    },
                );
                items.remove(&object_type);
                classes.remove(&object_type);
                if let Some(def) = item(&n, &assets.english)? {
                    projectile_count += def.projectiles.len();
                    if projectile_count > 262144 {
                        return Err(AssetError::LimitExceeded);
                    }
                    items.insert(object_type, def);
                }
                if is_player {
                    let mut caps = BTreeMap::new();
                    for child in &n.children {
                        if let (Some(stat_id), Some(max)) = (stat_id(&child.tag), child.attr("max"))
                        {
                            caps.insert(
                                stat_id,
                                StatView {
                                    stat_id,
                                    value: num(max)?,
                                },
                            );
                        }
                    }
                    classes.insert(
                        object_type,
                        ClassDef {
                            object_type,
                            stat_caps: caps.into_values().collect(),
                        },
                    );
                }
                for act in n
                    .descendants()
                    .filter(|n| matches!(n.tag.as_str(), "OnPlayerShootActivate" | "Activate"))
                {
                    if let Some(value) = act.attr("type").filter(|v| v.starts_with("0x")) {
                        origins.entry(num(value)?).or_default().insert(object_type);
                    }
                }
                if matches!(n.value("Class"), Some("Portal" | "GuildHallPortal")) {
                    if let Some(name) = n.value("DungeonName") {
                        portals.push((
                            name.to_owned(),
                            display(&n, &assets.english).unwrap_or("").to_owned(),
                            n.child("DungeonPortal").is_some(),
                            n.child("SafeZonePortal").is_some(),
                        ));
                        if !c
                            .dungeons
                            .iter()
                            .any(|d| d.name == name || d.aliases.iter().any(|a| a == name))
                        {
                            let kind = if n.child("SafeZonePortal").is_some() {
                                AreaKind::OtherHub
                            } else if n.child("DungeonPortal").is_some() {
                                AreaKind::Dungeon
                            } else {
                                AreaKind::Unknown
                            };
                            c.dungeons.push(DungeonDef {
                                name: name.into(),
                                aliases: vec![],
                                kind,
                                completion_index: None,
                                portal_type: None,
                                difficulty: None,
                                exalt: false,
                                exalt_gateway: false,
                                category: None,
                            });
                        }
                    }
                }
            } else if n.tag == "Ground" {
                let tile_type = id(&n)?;
                if !seen.insert((1, tile_type)) {
                    return Err(duplicate("ground", i64::from(tile_type)));
                }
                if let (Some(min), Some(max)) = (n.value("MinDamage"), n.value("MaxDamage")) {
                    tiles.insert(
                        tile_type,
                        TileDef {
                            tile_type,
                            min_damage: num(min)?,
                            max_damage: num(max)?,
                        },
                    );
                }
            } else if n.tag == "Enchantment" {
                let id: i32 = num(n.attr("type").ok_or(AssetError::InvalidCatalog)?)?;
                let def = EnchantDef {
                    regeneration: Some(enchant_regeneration(&n)?),
                    id,
                    name: name(&n, &assets.english)?,
                    description: n.value("Description").unwrap_or("").into(),
                    stat_damage_multiplier: Some(enchant_stat_multiplier(&n)?),
                    weapon_damage: Some(weapon_damage::parse(&n)),
                };
                if enchants.insert(id, def).is_some() {
                    return Err(duplicate("enchantments", i64::from(id)));
                }
            } else if n.tag == "DungeonModTemplate" {
                if let Some(id) = n.attr("id") {
                    templates.insert(id.to_owned());
                }
            } else if n.tag == "Crucible" {
                if let (Some(id), Some(value)) = (n.attr("id"), n.value("DamageMultiplier")) {
                    c.crucible_bonuses.push(CrucibleDef {
                        id: id.into(),
                        damage_multiplier: value.parse().map_err(|_| AssetError::InvalidCatalog)?,
                    });
                }
            }
            Ok(())
        })
        .map_err(|e| match e {
            AssetError::InvalidXml => AssetError::InvalidXmlSection {
                section: match text.name.as_str() {
                    "equip" => "equipment",
                    "players" => "classes",
                    "enchantments" => "enchantments",
                    _ => "definitions",
                },
            },
            other => other,
        })?;
    }
    let mut displays = BTreeMap::<String, BTreeSet<String>>::new();
    for (name, display, _, _) in &portals {
        if !display.is_empty() {
            if let Some(d) = c
                .dungeons
                .iter()
                .find(|d| d.name == *name || d.aliases.contains(name))
            {
                displays
                    .entry(display.clone())
                    .or_default()
                    .insert(d.name.clone());
            }
        }
    }
    for d in &mut c.dungeons {
        if !reviewed.contains(&d.name) {
            let safe = portals.iter().any(|p| p.0 == d.name && p.3);
            let dungeon =
                templates.contains(&d.name) || portals.iter().any(|p| p.0 == d.name && p.2);
            d.kind = if safe {
                AreaKind::OtherHub
            } else if dungeon {
                AreaKind::Dungeon
            } else {
                AreaKind::Unknown
            };
        }
    }
    let mut existing: BTreeSet<String> = c
        .dungeons
        .iter()
        .flat_map(|d| std::iter::once(d.name.clone()).chain(d.aliases.clone()))
        .collect();
    for (display, names) in displays {
        if names.len() == 1
            && !existing.contains(&display)
            && ![
                "The Tavern",
                "The Shatters",
                "Oryx's Sanctuary",
                "Mysterious Arena",
                "Treasure Room",
            ]
            .contains(&display.as_str())
        {
            if let Some(d) = c.dungeons.iter_mut().find(|d| names.contains(&d.name)) {
                existing.insert(display.clone());
                d.aliases.push(display);
            }
        }
    }
    for (projectile, parents) in origins {
        if parents.len() != 1 {
            continue;
        }
        let parent = *parents.first().ok_or(AssetError::InvalidCatalog)?;
        if let Some(mut inherited) = items.get(&parent).and_then(|p| p.scaling.clone()) {
            if let Some(child) = items.get_mut(&projectile) {
                if child.scaling.is_none() {
                    inherited.origin_item = Some(parent);
                    child.scaling = Some(inherited);
                }
            }
        }
    }
    let keys: BTreeSet<_> = c.sprites.iter().map(|s| s.key.as_str()).collect();
    for obj in objects.values_mut() {
        if obj.sprite_key.as_deref().is_some_and(|k| !keys.contains(k)) {
            obj.sprite_key = None;
        }
    }
    for item in items.values_mut() {
        if item
            .sprite_key
            .as_deref()
            .is_some_and(|k| !keys.contains(k))
        {
            item.sprite_key = None;
        }
    }
    c.items = items.into_values().collect();
    c.objects = objects.into_values().collect();
    c.classes = classes.into_values().collect();
    c.enchants = enchants.into_values().collect();
    c.tiles = tiles.into_values().collect();
    // Several portals can open one dungeon (event, test and art variants), so
    // the choice depends only on catalog data, never on file order.
    for d in &mut c.dungeons {
        if let Some(meta) = maxed_types::dungeon_meta(&d.name) {
            d.difficulty = Some(meta.difficulty);
            d.exalt = meta.exalt;
            d.exalt_gateway = meta.exalt_gateway;
            d.category = Some(meta.category);
        }
        d.portal_type = c
            .objects
            .iter()
            .filter(|o| {
                o.dungeon_name
                    .as_ref()
                    .is_some_and(|n| *n == d.name || d.aliases.contains(n))
            })
            .map(|o| o.object_type)
            .min();
    }
    c.dungeons.sort_by(|a, b| a.name.cmp(&b.name));
    for d in &mut c.dungeons {
        d.aliases.sort();
        d.aliases.dedup();
    }
    c.bags.sort_by_key(|b| b.object_type);
    c.crucible_bonuses.sort_by(|a, b| a.id.cmp(&b.id));
    validate_catalog(&c)?;
    Ok(c)
}

fn valid_codes(values: &[String], allowed: &[&str]) -> bool {
    values.windows(2).all(|w| w[0] < w[1]) && values.iter().all(|v| allowed.contains(&v.as_str()))
}

fn validate_firing(item: &ItemDef, f: &WeaponFiringDef) -> bool {
    let mut ids = BTreeSet::new();
    let known = f
        .groups
        .iter()
        .any(|g| g.schedule == WeaponSchedule::Simultaneous);
    (!f.groups.is_empty() || !f.reasons.is_empty())
        && f.subattack_projectile_indices
            .as_ref()
            .is_none_or(|indices| {
                indices.len() <= 256 && indices.iter().flatten().all(|index| *index >= 0)
            })
        && f.groups.len() <= 256
        && f.omitted_projectile_indices.len() <= 512
        && f.omitted_projectile_indices.windows(2).all(|w| w[0] < w[1])
        && f.omitted_projectile_indices
            .iter()
            .all(|i| *i >= 0 && !f.groups.iter().any(|g| g.projectile_index == *i))
        && (f.support != CatalogSupport::Supported
            || item
                .projectiles
                .iter()
                .all(|p| f.groups.iter().any(|g| g.projectile_index == p.index)))
        && valid_codes(&f.reasons, FIRING_REASONS)
        && f.support == support(known, &f.reasons)
        && (f.support != CatalogSupport::Supported
            || (known && f.omitted_projectile_indices.is_empty()))
        && f.groups.iter().all(|g| {
            !g.group_id.is_empty()
                && g.group_id.len() <= 64
                && g.group_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && ids.insert(&g.group_id)
                && g.rate_factor.is_finite()
                && g.rate_factor > 0.0
                && (1..=256).contains(&g.count)
                && item
                    .projectiles
                    .iter()
                    .any(|p| p.index == g.projectile_index)
                && valid_codes(&g.reasons, &["unsupported_schedule"])
                && g.reasons.iter().all(|r| f.reasons.contains(r))
                && match g.schedule {
                    WeaponSchedule::Simultaneous => g.reasons.is_empty(),
                    WeaponSchedule::Unsupported => !g.reasons.is_empty(),
                }
        })
}

fn validate_regeneration(r: &EnchantRegenDef) -> bool {
    let known = !r.hp.is_empty() || !r.mp.is_empty();
    let mut units = BTreeSet::new();
    r.hp.len() + r.mp.len() <= 256
        && valid_codes(&r.reasons, REGEN_REASONS)
        && valid_codes(&r.source_units, REGEN_UNITS)
        && r.support == support(known, &r.reasons)
        && r.hp.iter().chain(&r.mp).all(|e| {
            let valid = |v: Option<f64>| v.is_none_or(|v| v.is_finite() && v >= 0.0);
            let expected: Vec<String> = [
                e.fraction_of_max_per_sec
                    .map(|_| "fraction_of_max_per_second".into()),
                e.flat_per_sec.map(|_| "units_per_second".into()),
            ]
            .into_iter()
            .flatten()
            .collect();
            units.extend(expected.iter().cloned());
            !expected.is_empty()
                && valid(e.flat_per_sec)
                && valid(e.fraction_of_max_per_sec)
                && e.source_units == expected
        })
        && r.source_units.iter().eq(units.iter())
}

pub fn validate_catalog(c: &Catalog) -> Result<(), AssetError> {
    let invalid = || AssetError::InvalidCatalog;
    if c.format_version != 1
        || c.client_fingerprint.len() != 64
        || !c
            .client_fingerprint
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(invalid());
    }
    fn ordered<T: Ord>(values: impl Iterator<Item = T>) -> bool {
        let mut previous = None;
        for v in values {
            if previous.as_ref().is_some_and(|p| p >= &v) {
                return false;
            }
            previous = Some(v);
        }
        true
    }
    if !ordered(c.items.iter().map(|v| v.object_type))
        || !ordered(c.objects.iter().map(|v| v.object_type))
        || !ordered(c.classes.iter().map(|v| v.object_type))
        || !ordered(c.enchants.iter().map(|v| v.id))
        || !ordered(c.tiles.iter().map(|v| v.tile_type))
        || !ordered(c.bags.iter().map(|v| v.object_type))
        || !ordered(c.dungeons.iter().map(|v| &v.name))
        || !ordered(c.sprites.iter().map(|v| &v.key))
        || !ordered(c.crucible_bonuses.iter().map(|v| &v.id))
    {
        return Err(invalid());
    }
    if c.items.len() > 65536
        || c.objects.len() > 65536
        || c.enchants.len() > 65536
        || c.sprites.len() > 262144
        || c.dungeons.len() > 4096
        || c.crucible_bonuses.len() > 4096
    {
        return Err(AssetError::LimitExceeded);
    }
    let sprites: BTreeSet<_> = c.sprites.iter().map(|s| s.key.as_str()).collect();
    if c.enchants.iter().any(|e| {
        e.stat_damage_multiplier
            .is_some_and(|m| !m.is_finite() || m < 0.0 || m > f64::from(f32::MAX))
            || e.regeneration
                .as_ref()
                .is_some_and(|r| !validate_regeneration(r))
            || e.weapon_damage
                .as_ref()
                .is_some_and(|d| !weapon_damage::validate(d))
    }) {
        return Err(invalid());
    }
    let items: BTreeSet<_> = c.items.iter().map(|i| i.object_type).collect();
    for item in &c.items {
        if item.name.len() > 4096
            || item.projectiles.len() > 256
            || !ordered(item.projectiles.iter().map(|p| p.index))
            || item
                .sprite_key
                .as_deref()
                .is_some_and(|k| !sprites.contains(k))
        {
            return Err(invalid());
        }
        for p in &item.projectiles {
            if p.min_damage > p.max_damage || p.min_damage < 0 || p.index < 0 {
                return Err(invalid());
            }
        }
        if item
            .weapon_firing
            .as_ref()
            .is_some_and(|f| !validate_firing(item, f))
        {
            return Err(invalid());
        }
        if let Some(s) = &item.scaling {
            if !s.damage_per_stat.is_finite()
                || s.damage_per_stat < 0.0
                || s.shots == 0
                || s.origin_item.is_some_and(|id| !items.contains(&id))
            {
                return Err(invalid());
            }
        }
    }
    for o in &c.objects {
        if o.realm_event_key
            .as_ref()
            .is_some_and(|key| key.is_empty() || key.len() > 4096)
            || o.name.len() > 4096
            || o.sprite_key
                .as_deref()
                .is_some_and(|k| !sprites.contains(k))
        {
            return Err(invalid());
        }
    }
    let players: BTreeSet<_> = c
        .objects
        .iter()
        .filter(|o| o.is_player)
        .map(|o| o.object_type)
        .collect();
    for class in &c.classes {
        if !players.contains(&class.object_type)
            || !ordered(class.stat_caps.iter().map(|s| s.stat_id))
            || class.stat_caps.iter().any(|s| s.value < 0)
        {
            return Err(invalid());
        }
    }
    for tile in &c.tiles {
        if tile.min_damage < 0 || tile.min_damage > tile.max_damage {
            return Err(invalid());
        }
    }
    for bonus in &c.crucible_bonuses {
        if bonus.id.is_empty()
            || !bonus.damage_multiplier.is_finite()
            || bonus.damage_multiplier < 0.0
        {
            return Err(invalid());
        }
    }
    for sprite in &c.sprites {
        if !crate::sprites::safe_sheet(&sprite.sheet)
            || sprite.key.len() > 256
            || sprite.width == 0
            || sprite.height == 0
            || sprite.x.checked_add(sprite.width).is_none_or(|x| x > 16384)
            || sprite
                .y
                .checked_add(sprite.height)
                .is_none_or(|y| y > 16384)
        {
            return Err(invalid());
        }
        if sprite.trim.as_ref().is_some_and(|t| {
            t.width == 0
                || t.height == 0
                || t.x
                    .checked_add(t.width)
                    .is_none_or(|end| end > sprite.width)
                || t.y
                    .checked_add(t.height)
                    .is_none_or(|end| end > sprite.height)
        }) {
            return Err(invalid());
        }
    }
    for bag in &c.bags {
        if !c.bags.iter().any(|b| b.object_type == bag.base_type) {
            return Err(invalid());
        }
    }
    let mut aliases = BTreeSet::new();
    for d in &c.dungeons {
        for n in std::iter::once(&d.name).chain(&d.aliases) {
            if n.is_empty() || n.len() > 4096 || !aliases.insert(n) {
                return Err(invalid());
            }
        }
        if let Some(portal) = d.portal_type {
            let opens = c
                .objects
                .binary_search_by_key(&portal, |o| o.object_type)
                .ok()
                .and_then(|i| c.objects[i].dungeon_name.as_ref())
                .is_some_and(|n| *n == d.name || d.aliases.contains(n));
            if !opens {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

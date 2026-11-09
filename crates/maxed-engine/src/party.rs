//! Pure requirement checks and contribution flags for observed party members.

use maxed_types::{
    ContributionFlag, EquipSlot, ItemDef, ParseOutcome, ParseReason, ParseReasonKind, ParseResult,
    PlayerProfileView, RequirementPreset, Timestamp,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemTier {
    Numbered(u8),
    Ut,
    St,
    Unknown,
}

pub fn item_tier(def: Option<&ItemDef>) -> ItemTier {
    match def.and_then(|def| def.tier.as_deref()) {
        Some("UT") => ItemTier::Ut,
        Some("ST") => ItemTier::St,
        Some(tier) => tier.parse().map_or(ItemTier::Unknown, ItemTier::Numbered),
        None => ItemTier::Unknown,
    }
}

pub fn evaluate(
    preset: &RequirementPreset,
    profile: &PlayerProfileView,
    tier_of: impl Fn(u16) -> ItemTier,
) -> ParseOutcome {
    let slots = [
        (0, EquipSlot::Weapon, "weapon", preset.min_tier.weapon),
        (1, EquipSlot::Ability, "ability", preset.min_tier.ability),
        (2, EquipSlot::Armor, "armor", preset.min_tier.armor),
        (3, EquipSlot::Ring, "ring", preset.min_tier.ring),
    ];
    // A preset with no requirements passes even an unobserved profile.
    if preset.min_maxed.is_none() && slots.iter().all(|(_, _, _, min)| min.is_none()) {
        return ParseOutcome {
            result: ParseResult::Pass,
            reasons: vec![],
        };
    }
    let maxed = profile
        .claims_supported()
        .then_some(profile.maxed)
        .flatten();
    if profile.level.is_none() && maxed.is_none() {
        return ParseOutcome {
            result: ParseResult::NotSeen,
            reasons: vec![],
        };
    }

    let mut reasons = Vec::new();
    if let Some(min) = preset.min_maxed {
        let reason = match maxed {
            Some(maxed) if maxed < min => {
                Some((ParseReasonKind::BelowMaxed, format!("{maxed}/8 < {min}/8")))
            }
            None => Some((ParseReasonKind::StatsNotSeen, "stats not observed".into())),
            _ => None,
        };
        if let Some((kind, text)) = reason {
            reasons.push(ParseReason {
                slot: None,
                kind,
                text,
            });
        }
    }

    for (index, slot, name, min) in slots {
        let Some(min) = min else { continue };
        let item = profile
            .equipment
            .iter()
            .find(|equipment| equipment.slot == index)
            .and_then(|equipment| equipment.item_type);
        let reason = match item {
            // Without level, absent gear has not yet been observed as empty.
            None if profile.level.is_some() => {
                Some((ParseReasonKind::SlotEmpty, format!("no {name}")))
            }
            None => Some((ParseReasonKind::NoTierData, format!("{name} not seen"))),
            _ => match item.map_or(ItemTier::Unknown, &tier_of) {
                ItemTier::Numbered(tier) if tier < min => Some((
                    ParseReasonKind::BelowTier,
                    format!("{name} T{tier} < T{min}"),
                )),
                tier @ (ItemTier::Ut | ItemTier::St) if !preset.ut_st_meets => {
                    let label = if tier == ItemTier::Ut { "UT" } else { "ST" };
                    Some((
                        ParseReasonKind::UtStNotAccepted,
                        format!("{name} {label} not accepted"),
                    ))
                }
                ItemTier::Unknown => {
                    Some((ParseReasonKind::NoTierData, format!("{name}: no tier data")))
                }
                _ => None,
            },
        };
        if let Some((kind, text)) = reason {
            reasons.push(ParseReason {
                slot: Some(slot),
                kind,
                text,
            });
        }
    }

    let fails = reasons.iter().any(|reason| {
        matches!(
            reason.kind,
            ParseReasonKind::BelowMaxed
                | ParseReasonKind::SlotEmpty
                | ParseReasonKind::BelowTier
                | ParseReasonKind::UtStNotAccepted
        )
    });
    let result = if fails {
        ParseResult::Fail
    } else if !reasons.is_empty() {
        ParseResult::Check
    } else {
        ParseResult::Pass
    };
    ParseOutcome { result, reasons }
}

pub struct FlagInput {
    pub in_dungeon: bool,
    pub joined: Option<Timestamp>,
    pub departed: bool,
    pub damage: f64,
    pub share: Option<f64>,
    pub boss_share: Option<f64>,
    pub last_boss_started: Option<Timestamp>,
    pub boss_fight_ended: bool,
}

pub fn contribution_flags(input: &FlagInput) -> Vec<ContributionFlag> {
    if !input.in_dungeon {
        return vec![];
    }
    let mut flags = Vec::new();
    if input.damage == 0.0
        && matches!((input.joined, input.last_boss_started), (Some(joined), Some(started)) if joined <= started)
    {
        flags.push(ContributionFlag::NoDamageSeen);
    }
    if input.departed {
        flags.push(ContributionFlag::LeftEarly);
    }
    if input.boss_fight_ended
        && matches!((input.share, input.boss_share), (Some(share), Some(boss_share)) if share >= 0.01 && boss_share < 0.25 * share)
    {
        flags.push(ContributionFlag::MostlyTrash);
    }
    flags
}

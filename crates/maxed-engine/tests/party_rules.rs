use std::collections::BTreeMap;

use maxed_engine::party::{contribution_flags, evaluate, item_tier, FlagInput, ItemTier};
use maxed_types::{
    ContributionFlag, EnchantState, EnchantView, EquipSlot, EquipmentView, ItemDef, ParseOutcome,
    ParseReason, ParseReasonKind, ParseResult, PlayerProfileView, RequirementPreset, SlotTiers,
    Timestamp,
};

fn preset() -> RequirementPreset {
    RequirementPreset {
        id: "synthetic-preset".into(),
        name: "Synthetic requirements".into(),
        min_maxed: None,
        min_tier: SlotTiers {
            weapon: None,
            ability: None,
            armor: None,
            ring: None,
        },
        ut_st_meets: true,
        auto_apply: vec![],
    }
}

fn equipment(slot: u8, item_type: Option<u16>) -> EquipmentView {
    EquipmentView {
        observed_empty: None,
        slot,
        item_type,
        enchant: EnchantView {
            state: EnchantState::Missing,
            slots: None,
            applied: None,
            ids: vec![],
        },
    }
}

fn profile() -> PlayerProfileView {
    PlayerProfileView {
        maxed_gated: false,
        scope_id: "synthetic-scope".into(),
        run_id: None,
        player_id: "synthetic-player".into(),
        object_id: 1,
        is_local: false,
        name: Some("SynthAlpha".into()),
        class_id: None,
        level: Some(20),
        stars: None,
        fame: None,
        skin_id: None,
        guild: None,
        stats: vec![],
        maxed: Some(8),
        maxed_stats: vec![],
        equipment: (0..4).map(|slot| equipment(slot, None)).collect(),
        first_seen: Timestamp(1),
        last_seen: Timestamp(2),
    }
}

fn parse(
    preset: &RequirementPreset,
    profile: &PlayerProfileView,
    tiers: &[(u16, ItemTier)],
) -> ParseOutcome {
    let tiers: BTreeMap<_, _> = tiers.iter().copied().collect();
    evaluate(preset, profile, |item| {
        tiers.get(&item).copied().unwrap_or(ItemTier::Unknown)
    })
}

fn reason(slot: Option<EquipSlot>, kind: ParseReasonKind, text: &str) -> ParseReason {
    ParseReason {
        slot,
        kind,
        text: text.into(),
    }
}

fn outcome(result: ParseResult, reasons: Vec<ParseReason>) -> ParseOutcome {
    ParseOutcome { result, reasons }
}

fn slot_preset(slot: u8) -> RequirementPreset {
    let mut preset = preset();
    match slot {
        0 => preset.min_tier.weapon = Some(11),
        1 => preset.min_tier.ability = Some(11),
        2 => preset.min_tier.armor = Some(11),
        3 => preset.min_tier.ring = Some(11),
        _ => unreachable!(),
    }
    preset
}

fn slots() -> [(u8, EquipSlot, &'static str); 4] {
    [
        (0, EquipSlot::Weapon, "weapon"),
        (1, EquipSlot::Ability, "ability"),
        (2, EquipSlot::Armor, "armor"),
        (3, EquipSlot::Ring, "ring"),
    ]
}

#[test]
fn not_seen_without_level_or_maxed() {
    let mut preset = preset();
    preset.min_maxed = Some(6);
    preset.min_tier.weapon = Some(11);
    for gear in [
        vec![],
        vec![equipment(0, None)],
        vec![equipment(0, Some(1))],
    ] {
        let mut profile = profile();
        profile.level = None;
        profile.maxed = None;
        profile.equipment = gear;
        assert_eq!(
            evaluate(&preset, &profile, |_| panic!("unobserved profile")),
            outcome(ParseResult::NotSeen, vec![])
        );
    }
    let mut profile = profile();
    profile.level = None;
    preset.min_tier.weapon = None;
    assert_eq!(
        parse(&preset, &profile, &[]),
        outcome(ParseResult::Pass, vec![])
    );
}

#[test]
fn maxed_below_minimum_fails_with_text() {
    let mut preset = preset();
    preset.min_maxed = Some(6);
    for (maxed, result, reasons) in [
        (
            5,
            ParseResult::Fail,
            vec![reason(None, ParseReasonKind::BelowMaxed, "5/8 < 6/8")],
        ),
        (6, ParseResult::Pass, vec![]),
        (8, ParseResult::Pass, vec![]),
    ] {
        let mut profile = profile();
        profile.maxed = Some(maxed);
        assert_eq!(parse(&preset, &profile, &[]), outcome(result, reasons));
    }
}

#[test]
fn maxed_missing_with_gear_is_check() {
    let mut preset = preset();
    preset.min_maxed = Some(6);
    let mut profile = profile();
    profile.maxed = None;
    profile.equipment = vec![equipment(0, Some(1))];
    assert_eq!(
        parse(&preset, &profile, &[]),
        outcome(
            ParseResult::Check,
            vec![reason(
                None,
                ParseReasonKind::StatsNotSeen,
                "stats not observed"
            )]
        )
    );
}

#[test]
fn empty_slot_fails() {
    for (index, slot, name) in slots() {
        for gear in [vec![], vec![equipment(index, None)]] {
            let mut profile = profile();
            profile.equipment = gear;
            assert_eq!(
                parse(&slot_preset(index), &profile, &[]),
                outcome(
                    ParseResult::Fail,
                    vec![reason(
                        Some(slot.clone()),
                        ParseReasonKind::SlotEmpty,
                        &format!("no {name}")
                    )]
                )
            );
        }
    }
}

#[test]
fn numbered_tier_below_fails_with_text() {
    for (index, slot, name) in slots() {
        let mut profile = profile();
        profile.equipment = vec![equipment(index, Some(1))];
        assert_eq!(
            parse(&slot_preset(index), &profile, &[(1, ItemTier::Numbered(9))]),
            outcome(
                ParseResult::Fail,
                vec![reason(
                    Some(slot),
                    ParseReasonKind::BelowTier,
                    &format!("{name} T9 < T11")
                )]
            )
        );
    }
}

#[test]
fn numbered_tier_equal_passes() {
    for (index, _, _) in slots() {
        for tier in [11, 12] {
            let mut profile = profile();
            profile.equipment = vec![equipment(index, Some(1))];
            assert_eq!(
                parse(
                    &slot_preset(index),
                    &profile,
                    &[(1, ItemTier::Numbered(tier))]
                ),
                outcome(ParseResult::Pass, vec![])
            );
        }
    }
}

#[test]
fn ut_st_pass_when_toggle_on_fail_when_off() {
    for (index, slot, name) in slots() {
        for (tier, label) in [(ItemTier::Ut, "UT"), (ItemTier::St, "ST")] {
            for accepted in [true, false] {
                let mut preset = slot_preset(index);
                preset.ut_st_meets = accepted;
                let mut profile = profile();
                profile.equipment = vec![equipment(index, Some(1))];
                let expected = if accepted {
                    outcome(ParseResult::Pass, vec![])
                } else {
                    outcome(
                        ParseResult::Fail,
                        vec![reason(
                            Some(slot.clone()),
                            ParseReasonKind::UtStNotAccepted,
                            &format!("{name} {label} not accepted"),
                        )],
                    )
                };
                assert_eq!(parse(&preset, &profile, &[(1, tier)]), expected);
            }
        }
    }
}

#[test]
fn unknown_tier_is_check() {
    for (index, slot, name) in slots() {
        for tiers in [vec![], vec![(1, ItemTier::Unknown)]] {
            let mut profile = profile();
            profile.equipment = vec![equipment(index, Some(1))];
            assert_eq!(
                parse(&slot_preset(index), &profile, &tiers),
                outcome(
                    ParseResult::Check,
                    vec![reason(
                        Some(slot.clone()),
                        ParseReasonKind::NoTierData,
                        &format!("{name}: no tier data")
                    )]
                )
            );
        }
    }
}

#[test]
fn gear_without_level_does_not_imply_empty_slots() {
    let mut profile = profile();
    profile.level = None;
    assert_eq!(
        parse(&slot_preset(1), &profile, &[]),
        outcome(
            ParseResult::Check,
            vec![reason(
                Some(EquipSlot::Ability),
                ParseReasonKind::NoTierData,
                "ability not seen"
            )]
        )
    );
}

#[test]
fn fail_beats_check() {
    let mut preset = slot_preset(0);
    preset.min_maxed = Some(6);
    let mut profile = profile();
    profile.maxed = None;
    assert_eq!(
        parse(&preset, &profile, &[]),
        outcome(
            ParseResult::Fail,
            vec![
                reason(None, ParseReasonKind::StatsNotSeen, "stats not observed"),
                reason(
                    Some(EquipSlot::Weapon),
                    ParseReasonKind::SlotEmpty,
                    "no weapon"
                ),
            ]
        )
    );
    profile.maxed = Some(5);
    profile.equipment = vec![equipment(0, Some(1))];
    assert_eq!(
        parse(&preset, &profile, &[]),
        outcome(
            ParseResult::Fail,
            vec![
                reason(None, ParseReasonKind::BelowMaxed, "5/8 < 6/8"),
                reason(
                    Some(EquipSlot::Weapon),
                    ParseReasonKind::NoTierData,
                    "weapon: no tier data"
                ),
            ]
        )
    );
}

#[test]
fn empty_preset_passes_everyone() {
    for level in [None, Some(20)] {
        for maxed in [None, Some(0), Some(8)] {
            let mut profile = profile();
            profile.level = level;
            profile.maxed = maxed;
            assert_eq!(
                evaluate(&preset(), &profile, |_| panic!("no tier rules")),
                outcome(ParseResult::Pass, vec![])
            );
        }
    }
}

#[test]
fn reasons_in_maxed_then_slot_order() {
    let mut preset = preset();
    preset.min_maxed = Some(6);
    preset.min_tier = SlotTiers {
        weapon: Some(11),
        ability: Some(5),
        armor: Some(12),
        ring: Some(4),
    };
    preset.ut_st_meets = false;
    let mut profile = profile();
    profile.maxed = Some(5);
    profile.equipment = vec![
        equipment(3, Some(3)),
        equipment(2, Some(2)),
        equipment(4, Some(4)),
        equipment(1, None),
        equipment(0, Some(1)),
    ];
    assert_eq!(
        parse(
            &preset,
            &profile,
            &[(1, ItemTier::Numbered(9)), (2, ItemTier::St)]
        ),
        outcome(
            ParseResult::Fail,
            vec![
                reason(None, ParseReasonKind::BelowMaxed, "5/8 < 6/8"),
                reason(
                    Some(EquipSlot::Weapon),
                    ParseReasonKind::BelowTier,
                    "weapon T9 < T11"
                ),
                reason(
                    Some(EquipSlot::Ability),
                    ParseReasonKind::SlotEmpty,
                    "no ability"
                ),
                reason(
                    Some(EquipSlot::Armor),
                    ParseReasonKind::UtStNotAccepted,
                    "armor ST not accepted"
                ),
                reason(
                    Some(EquipSlot::Ring),
                    ParseReasonKind::NoTierData,
                    "ring: no tier data"
                ),
            ]
        )
    );
}

#[test]
fn item_tier_parses_catalog_strings() {
    for (tier, expected) in [
        (Some("12"), ItemTier::Numbered(12)),
        (Some("0"), ItemTier::Numbered(0)),
        (Some("255"), ItemTier::Numbered(255)),
        (Some("UT"), ItemTier::Ut),
        (Some("ST"), ItemTier::St),
        (None, ItemTier::Unknown),
        (Some("x"), ItemTier::Unknown),
        (Some("256"), ItemTier::Unknown),
        (Some("-1"), ItemTier::Unknown),
        (Some(""), ItemTier::Unknown),
    ] {
        let def = ItemDef {
            weapon_firing: None,
            potion_stat: None,
            object_type: 1,
            name: "Synthetic item".into(),
            tier: tier.map(str::to_owned),
            slot_type: None,
            projectiles: vec![],
            scaling: None,
            sprite_key: None,
            soulbound: None,
            stat_bonuses: None,
        };
        assert_eq!(item_tier(Some(&def)), expected, "tier {tier:?}");
    }
    assert_eq!(item_tier(None), ItemTier::Unknown);
}

fn flag_input() -> FlagInput {
    FlagInput {
        in_dungeon: true,
        joined: Some(Timestamp(10)),
        departed: false,
        damage: 0.0,
        share: None,
        boss_share: None,
        last_boss_started: None,
        boss_fight_ended: false,
    }
}

#[test]
fn flags_none_outside_dungeons() {
    let input = FlagInput {
        in_dungeon: false,
        departed: true,
        last_boss_started: Some(Timestamp(10)),
        share: Some(0.04),
        boss_share: Some(0.009),
        boss_fight_ended: true,
        ..flag_input()
    };
    assert!(contribution_flags(&input).is_empty());
}

#[test]
fn no_damage_seen_only_after_a_boss_started_since_joining() {
    for (joined, started, damage, expected) in [
        (Some(9), Some(10), 0.0, true),
        (Some(10), Some(10), 0.0, true),
        (Some(11), Some(10), 0.0, false),
        (None, Some(10), 0.0, false),
        (Some(9), None, 0.0, false),
        (Some(9), Some(10), 1.0, false),
    ] {
        let input = FlagInput {
            joined: joined.map(Timestamp),
            last_boss_started: started.map(Timestamp),
            damage,
            ..flag_input()
        };
        assert_eq!(
            contribution_flags(&input),
            if expected {
                vec![ContributionFlag::NoDamageSeen]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn left_early_when_departed() {
    for departed in [false, true] {
        let input = FlagInput {
            departed,
            ..flag_input()
        };
        assert_eq!(
            contribution_flags(&input),
            if departed {
                vec![ContributionFlag::LeftEarly]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn mostly_trash_threshold() {
    for (share, boss_share, ended, expected) in [
        (Some(0.04), Some(0.009), true, true),
        (Some(0.04), Some(0.011), true, false),
        (Some(0.04), Some(0.01), true, false),
        (Some(0.009), Some(0.0), true, false),
        (Some(0.01), Some(0.0), true, true),
        (Some(0.04), Some(0.009), false, false),
        (None, Some(0.0), true, false),
        (Some(0.04), None, true, false),
    ] {
        let input = FlagInput {
            share,
            boss_share,
            boss_fight_ended: ended,
            damage: 1.0,
            ..flag_input()
        };
        assert_eq!(
            contribution_flags(&input),
            if expected {
                vec![ContributionFlag::MostlyTrash]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn flags_in_no_damage_left_early_mostly_trash_order() {
    let input = FlagInput {
        departed: true,
        last_boss_started: Some(Timestamp(10)),
        share: Some(0.04),
        boss_share: Some(0.009),
        boss_fight_ended: true,
        ..flag_input()
    };
    assert_eq!(
        contribution_flags(&input),
        vec![
            ContributionFlag::NoDamageSeen,
            ContributionFlag::LeftEarly,
            ContributionFlag::MostlyTrash,
        ]
    );
}

#[test]
fn local_maxed_provenance_controls_requirements() {
    let mut requirements = preset();
    requirements.min_maxed = Some(6);
    for (local, gated, count, result, reason) in [
        (
            true,
            false,
            8,
            ParseResult::Check,
            Some(ParseReasonKind::StatsNotSeen),
        ),
        (true, true, 8, ParseResult::Pass, None),
        (
            true,
            true,
            4,
            ParseResult::Fail,
            Some(ParseReasonKind::BelowMaxed),
        ),
        (false, false, 8, ParseResult::Pass, None),
        (
            false,
            false,
            4,
            ParseResult::Fail,
            Some(ParseReasonKind::BelowMaxed),
        ),
    ] {
        let mut p = profile();
        p.is_local = local;
        p.maxed_gated = gated;
        p.maxed = Some(count);
        let outcome = parse(&requirements, &p, &[]);
        assert_eq!(outcome.result, result);
        assert_eq!(outcome.reasons.first().map(|r| r.kind.clone()), reason);
    }
    let mut p = profile();
    p.is_local = true;
    requirements.min_tier.weapon = Some(1);
    let outcome = parse(&requirements, &p, &[]);
    assert_eq!(outcome.result, ParseResult::Fail);
    assert_eq!(outcome.reasons[0].kind, ParseReasonKind::StatsNotSeen);
    assert_eq!(outcome.reasons[1].kind, ParseReasonKind::SlotEmpty);
    assert_eq!(parse(&preset(), &p, &[]).result, ParseResult::Pass);
    p.level = None;
    assert_eq!(parse(&requirements, &p, &[]).result, ParseResult::NotSeen);
}

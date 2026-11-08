//! Shared checked stat split. Gate P evidence establishes exalt inside boost.
use maxed_types::{ExaltPlacement, StatSplitEvidence, StatSplitReason};
use std::collections::BTreeMap;
#[derive(Debug, Default, PartialEq)]
pub(crate) struct StatLine {
    pub split_evidence: Option<StatSplitEvidence>,
    pub stat_id: u8,
    pub total: Option<i32>,
    pub base: Option<i32>,
    pub cap: Option<i32>,
    pub at_cap: Option<bool>,
    pub boost: Option<i32>,
    pub gear: Option<i32>,
    pub exalt: Option<i32>,
    pub other: Option<i32>,
    pub current: Option<i32>,
}
#[derive(Debug, PartialEq)]
pub(crate) struct Condition {
    pub name: &'static str,
    pub bit: u8,
    pub secondary: bool,
    pub confirmed: bool,
}
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Panel {
    pub lines: Vec<StatLine>,
    pub conditions: Vec<Condition>,
    pub breakdown_provisional: bool,
}
// Each of the four slots is Some(known bonuses), including Some(empty) for a
// known empty slot. None means an unknown item/slot, not zero bonuses.
pub(crate) fn panel(
    stats: &BTreeMap<u8, i32>,
    caps: &BTreeMap<u8, i32>,
    equipment: [Option<&[(u8, i32)]>; 4],
) -> Panel {
    let gear_known = equipment.iter().all(Option::is_some);
    let lines = [
        (0, 46, 111),
        (3, 47, 112),
        (20, 48, 105),
        (21, 49, 106),
        (22, 50, 107),
        (26, 51, 108),
        (27, 52, 109),
        (28, 53, 110),
    ]
    .into_iter()
    .map(|(stat_id, boost_id, exalt_id)| {
        let gear = gear_known
            .then(|| {
                equipment
                    .iter()
                    .flatten()
                    .flat_map(|s| s.iter())
                    .filter(|(id, _)| *id == stat_id)
                    .try_fold(0i32, |sum, (_, v)| sum.checked_add(*v))
            })
            .flatten();
        let mut line = split_line(
            StatLine {
                stat_id,
                total: stats.get(&stat_id).copied(),
                boost: stats.get(&boost_id).copied(),
                cap: caps.get(&stat_id).copied(),
                gear,
                current: match stat_id {
                    0 => stats.get(&1).copied(),
                    3 => stats.get(&4).copied(),
                    _ => None,
                },
                ..Default::default()
            },
            stats.get(&exalt_id).copied(),
            ExaltPlacement::VerifiedInsideBoost,
        );
        if gear_known && gear.is_none() {
            line.split_evidence
                .as_mut()
                .unwrap()
                .reasons
                .push(StatSplitReason::Overflow);
        }
        line
    })
    .collect();
    let conditions = CONDITIONS
        .iter()
        .filter(|(_, bit, secondary, _)| {
            stats
                .get(&if *secondary { 96 } else { 29 })
                .is_some_and(|v| (*v as u32 & (1u32 << (*bit - 1))) != 0)
        })
        .map(|&(name, bit, secondary, confirmed)| Condition {
            name,
            bit,
            secondary,
            confirmed,
        })
        .collect();
    Panel {
        lines,
        conditions,
        breakdown_provisional: true,
    }
}
// Missing gear does not erase independently known base or exalt. A demonstrated
// contradiction does: no negative Other, guessed zero, or per-stat placement override.
fn split_line(mut line: StatLine, amount: Option<i32>, placement: ExaltPlacement) -> StatLine {
    use StatSplitReason::*;
    let mut reasons = Vec::new();
    line.exalt = match amount {
        None => {
            reasons.push(MissingExalt);
            None
        }
        Some(n) if n < 0 => {
            reasons.push(InvalidExalt);
            None
        }
        // Stats 105–112 carry bonus amounts, including HP/MP (111/112).
        Some(n) => Some(n),
    };
    let verified = matches!(
        placement,
        ExaltPlacement::VerifiedInsideBase | ExaltPlacement::VerifiedInsideBoost
    );
    let placement_known = verified || line.exalt == Some(0);
    if !placement_known {
        reasons.push(UnverifiedPlacement);
    }
    let inside_base = placement == ExaltPlacement::VerifiedInsideBase;
    if placement != ExaltPlacement::Unresolved || line.exalt == Some(0) {
        line.base = subtract(line.total, line.boost, &mut reasons);
        if inside_base {
            line.base = subtract(line.base, line.exalt, &mut reasons);
        }
        line.other = subtract(line.boost, line.gear, &mut reasons);
        if !inside_base {
            line.other = subtract(line.other, line.exalt, &mut reasons);
        }
        if line.base.is_some_and(|n| n < 0) {
            line.base = None;
            reasons.push(NegativeBase);
        }
        if line.other.is_some_and(|n| n < 0) {
            line.base = None;
            line.other = None;
            reasons.push(NegativeOther);
        }
    }
    let cap = line.cap.filter(|c| *c >= 0);
    if cap.is_none() {
        reasons.push(MissingOperands);
    }
    let cap_eligible =
        placement_known && line.exalt.is_some() && line.base.is_some() && cap.is_some();
    line.at_cap = if cap_eligible {
        line.base.zip(cap).map(|(b, c)| b >= c)
    } else {
        None
    };
    let mut unique = Vec::new();
    for reason in reasons {
        if !unique.contains(&reason) {
            unique.push(reason);
        }
    }
    line.split_evidence = Some(StatSplitEvidence {
        placement,
        cap_eligible,
        reasons: unique,
    });
    line
}

fn subtract(a: Option<i32>, b: Option<i32>, reasons: &mut Vec<StatSplitReason>) -> Option<i32> {
    match a.zip(b) {
        None => {
            reasons.push(StatSplitReason::MissingOperands);
            None
        }
        Some((a, b)) => {
            let difference = a.checked_sub(b);
            if difference.is_none() {
                reasons.push(StatSplitReason::Overflow);
            }
            difference
        }
    }
}

// Effect numbers are 1-based. Secondary masks keep numeric names because
// math.rs establishes their behavior but does not establish semantic names.
const CONDITIONS: &[(&str, u8, bool, bool)] = &[
    ("Quiet", 2, false, false),
    ("Weak", 3, false, true),
    ("Slowed", 4, false, true),
    ("Dazed", 6, false, false),
    ("Stunned", 7, false, false),
    ("Invisible", 13, false, false),
    ("Paralyzed", 14, false, false),
    ("Speedy", 15, false, false),
    ("Bleeding", 16, false, true),
    ("Healing", 18, false, false),
    ("Damaging", 19, false, true),
    ("Berserk", 20, false, false),
    ("Paused", 21, false, false),
    ("Invincible", 24, false, false),
    ("Invulnerable", 25, false, true),
    ("Armored", 26, false, true),
    ("ArmorBroken", 27, false, true),
    ("Hexed", 28, false, false),
    ("Unstable", 30, false, false),
    ("Secondary 0x8", 4, true, true),
    ("Secondary 0x40", 7, true, true),
    ("Secondary 0x20000", 18, true, true),
];
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stat_panel_both_placements_do_not_double_count_or_promote_exalt_to_base() {
        use maxed_types::ExaltPlacement::*;
        for (placement, base, other, at_cap) in [
            (ProvisionalInsideBoost, 75, 3, None),
            (VerifiedInsideBoost, 75, 3, Some(true)),
            (VerifiedInsideBase, 73, 5, Some(false)),
        ] {
            let line = split_line(
                StatLine {
                    stat_id: 20,
                    total: Some(85),
                    boost: Some(10),
                    gear: Some(5),
                    cap: Some(75),
                    ..Default::default()
                },
                Some(2),
                placement,
            );
            assert_eq!(
                (line.base, line.other, line.at_cap),
                (Some(base), Some(other), at_cap)
            );
            assert_eq!(base + 5 + 2 + other, 85);
        }
        let line = split_line(
            StatLine {
                stat_id: 20,
                total: Some(75),
                boost: Some(0),
                gear: Some(0),
                cap: Some(75),
                ..Default::default()
            },
            Some(2),
            VerifiedInsideBase,
        );
        assert_eq!((line.base, line.at_cap), (Some(73), Some(false)));
    }

    #[test]
    fn stat_panel_invalid_arithmetic_and_missing_operands_have_reasons() {
        use maxed_types::{ExaltPlacement::*, StatSplitReason::*};
        for (total, boost, gear, level, placement, reason) in [
            (
                Some(1),
                Some(2),
                Some(0),
                Some(0),
                VerifiedInsideBoost,
                NegativeBase,
            ),
            (
                Some(i32::MAX),
                Some(-1),
                None,
                Some(0),
                VerifiedInsideBoost,
                Overflow,
            ),
            (
                Some(55),
                Some(0),
                Some(0),
                Some(2),
                ProvisionalInsideBoost,
                NegativeOther,
            ),
            (
                Some(75),
                None,
                Some(0),
                Some(0),
                VerifiedInsideBase,
                MissingOperands,
            ),
            (
                Some(75),
                Some(0),
                Some(0),
                None,
                VerifiedInsideBase,
                MissingExalt,
            ),
            (
                Some(75),
                Some(0),
                Some(0),
                Some(-1),
                VerifiedInsideBase,
                InvalidExalt,
            ),
        ] {
            let line = split_line(
                StatLine {
                    stat_id: 20,
                    total,
                    boost,
                    gear,
                    cap: Some(75),
                    ..Default::default()
                },
                level,
                placement,
            );
            assert_eq!(line.base, None);
            assert_eq!(line.at_cap, None);
            assert!(line.split_evidence.unwrap().reasons.contains(&reason));
        }
    }

    #[test]
    fn stat_panel_maps_all_amounts_independently_of_gear() {
        for (exalt_id, stat_id, boost_id, amount) in [
            (105, 20, 48, 2),
            (106, 21, 49, 2),
            (107, 22, 50, 2),
            (108, 26, 51, 2),
            (109, 27, 52, 2),
            (110, 28, 53, 2),
            (111, 0, 46, 10),
            (112, 3, 47, 10),
        ] {
            let stats = BTreeMap::from([(exalt_id, amount), (stat_id, 100), (boost_id, 20)]);
            let p = panel(&stats, &BTreeMap::from([(stat_id, 80)]), [None; 4]);
            let line = p.lines.iter().find(|l| l.stat_id == stat_id).unwrap();
            assert_eq!(line.exalt, Some(amount));
            assert_eq!(line.base, Some(80));
            assert_eq!(
                (line.gear, line.other, line.at_cap),
                (None, None, Some(true))
            );
        }
    }

    #[test]
    fn stat_panel_zero_missing_negative_and_large_amounts_are_distinct() {
        for (level, exalt, eligible) in [
            (Some(0), Some(0), Some(true)),
            (None, None, None),
            (Some(-1), None, None),
            (Some(i32::MAX), Some(i32::MAX), Some(true)),
        ] {
            let mut stats = BTreeMap::from([(0, 750), (46, 0)]);
            if let Some(level) = level {
                stats.insert(111, level);
            }
            let p = panel(&stats, &BTreeMap::from([(0, 750)]), [None; 4]);
            assert_eq!(p.lines[0].exalt, exalt);
            assert_eq!(p.lines[0].at_cap, eligible);
        }
    }

    #[test]
    fn stat_panel_shaitan_observations_preserve_spd_contradiction() {
        // Fixture-derived numeric observations; synthetic identity/catalog/gear.
        for (id, boost_id, exalt_id, total, boost, level, base, other) in [
            (0, 46, 111, 840, 90, 0, Some(750), Some(90)),
            (3, 47, 112, 390, 90, 0, Some(300), Some(90)),
            (20, 48, 105, 81, 6, 2, Some(75), Some(4)),
            (21, 49, 106, 67, 42, 1, Some(25), Some(41)),
            (22, 50, 107, 55, 0, 2, None, None),
            (26, 51, 108, 46, 6, 0, Some(40), Some(6)),
            (27, 52, 109, 57, 7, 1, Some(50), Some(6)),
            (28, 53, 110, 55, 5, 1, Some(50), Some(4)),
        ] {
            let p = panel(
                &BTreeMap::from([(id, total), (boost_id, boost), (exalt_id, level)]),
                &BTreeMap::new(),
                [Some(&[]); 4],
            );
            let line = p.lines.iter().find(|l| l.stat_id == id).unwrap();
            assert_eq!((line.base, line.other), (base, other));
            assert_eq!((line.total, line.boost), (Some(total), Some(boost)));
            if let (Some(b), Some(g), Some(e), Some(o)) =
                (line.base, line.gear, line.exalt, line.other)
            {
                assert_eq!(b + g + e + o, total);
            }
        }
    }

    #[test]
    fn stat_panel_gear_unknown_keeps_other_null() {
        let stats = BTreeMap::from([(20, 80), (48, 10), (0, 800), (1, 650)]);
        let p = panel(
            &stats,
            &BTreeMap::from([(20, 75)]),
            [None, Some(&[]), Some(&[]), Some(&[])],
        );
        let attack = p.lines.iter().find(|l| l.stat_id == 20).unwrap();
        assert_eq!(
            (
                attack.base,
                attack.gear,
                attack.other,
                attack.exalt,
                attack.at_cap
            ),
            (Some(70), None, None, None, None)
        );
        assert_eq!(p.lines[0].current, Some(650));
        assert!(p.breakdown_provisional);
        let p = panel(
            &stats,
            &BTreeMap::new(),
            [Some(&[(20, 4)]), Some(&[]), Some(&[]), Some(&[])],
        );
        let attack = p.lines.iter().find(|l| l.stat_id == 20).unwrap();
        assert_eq!((attack.gear, attack.other), (Some(4), None));
    }
    #[test]
    fn confirmed_bits_match_math_rs_masks() {
        let primary = [0x4, 0x40000, 0x4000000, 0x2000000, 0x1000000, 0x8, 0x8000];
        let secondary = [0x20000, 0x8, 0x40];
        for (stat, masks) in [(29, primary.as_slice()), (96, secondary.as_slice())] {
            for mask in masks {
                let p = panel(
                    &BTreeMap::from([(stat, *mask)]),
                    &BTreeMap::new(),
                    [Some(&[]); 4],
                );
                assert_eq!(p.conditions.len(), 1);
                let c = &p.conditions[0];
                assert!(c.confirmed);
                assert_eq!(1i32 << (c.bit - 1), *mask);
                assert_eq!(c.secondary, stat == 96);
            }
        }
        let p = panel(&BTreeMap::from([(29, 2)]), &BTreeMap::new(), [Some(&[]); 4]);
        assert_eq!(p.conditions[0].name, "Quiet");
        assert!(!p.conditions[0].confirmed);
    }
}

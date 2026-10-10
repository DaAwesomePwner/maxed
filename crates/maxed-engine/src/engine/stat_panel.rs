//! Pure stat breakdown; exalt amounts are intentionally unknown.
use std::collections::BTreeMap;
#[derive(Debug, Default, PartialEq)]
pub(crate) struct StatLine {
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
        (0, 46),
        (3, 47),
        (20, 48),
        (21, 49),
        (22, 50),
        (26, 51),
        (27, 52),
        (28, 53),
    ]
    .into_iter()
    .map(|(stat_id, boost_id)| {
        let total = stats.get(&stat_id).copied();
        let boost = stats.get(&boost_id).copied();
        let base = total.zip(boost).and_then(|(t, b)| t.checked_sub(b));
        let cap = caps.get(&stat_id).copied();
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
        StatLine {
            stat_id,
            total,
            base,
            cap,
            at_cap: base.zip(cap).map(|(b, c)| b >= c),
            boost,
            gear,
            exalt: None,
            other: boost.zip(gear).and_then(|(b, g)| b.checked_sub(g)),
            current: match stat_id {
                0 => stats.get(&1).copied(),
                3 => stats.get(&4).copied(),
                _ => None,
            },
        }
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
            (Some(70), None, None, None, Some(false))
        );
        assert_eq!(p.lines[0].current, Some(650));
        assert!(p.breakdown_provisional);
        let p = panel(
            &stats,
            &BTreeMap::new(),
            [Some(&[(20, 4)]), Some(&[]), Some(&[]), Some(&[])],
        );
        let attack = p.lines.iter().find(|l| l.stat_id == 20).unwrap();
        assert_eq!((attack.gear, attack.other), (Some(4), Some(6)));
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

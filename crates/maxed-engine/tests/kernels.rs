use maxed_engine::{
    damage_with_defense, decode_completion_counts, dps, idle_deadline, parse_enchant, share,
    valid_counter_step,
};
use maxed_types::{EnchantState, Timestamp};

#[test]
fn defense_masks_and_rounding() {
    for (damage, ap, defense, bits, expected) in [
        (100, false, 200, [0, 0], 10),
        (100, true, 200, [0, 0], 100),
        (100, true, 0, [0x1000000, 0], 0),
        (100, false, 20, [0, 0x40], 100),
        (101, false, 21, [0x2000000, 0], 70),
        (101, false, 21, [0x4000000, 0], 101),
        (101, false, 21, [0, 0x20000], 100),
        (101, false, 0, [0, 0x48], 112),
    ] {
        assert_eq!(damage_with_defense(damage, ap, defense, bits), expected);
    }
}

#[test]
fn time_and_ratio_boundaries() {
    assert_eq!(dps(600., Timestamp(1), Timestamp(2_000_001)), Some(300.));
    assert_eq!(dps(600., Timestamp(1), Timestamp(1)), None);
    assert_eq!(dps(f64::MAX, Timestamp(0), Timestamp(1)), None);
    assert_eq!(share(100., 200.), Some(0.5));
    assert_eq!(share(0., 0.), None);
    assert_eq!(idle_deadline(Timestamp(7)), Timestamp(1_800_000_007));
    assert!(valid_counter_step(9, 10, 30_000_000));
    assert!(!valid_counter_step(9, 11, 1));
    assert!(!valid_counter_step(9, 10, 30_000_001));
    assert!(!valid_counter_step(-1, 0, 1));
}

#[test]
fn enchant_evidence_is_strict() {
    assert_eq!(parse_enchant(None).state, EnchantState::Missing);
    assert_eq!(parse_enchant(Some("")).slots, Some(0));
    assert_eq!(
        parse_enchant(Some("AAIE")).state,
        EnchantState::RecordedEmpty
    );
    // Header, unlocked empty, locked, unknown positive ID, terminator.
    let e = parse_enchant(Some("AAIE___-_3sA_f8="));
    assert_eq!(e.state, EnchantState::Recorded);
    assert_eq!((e.slots, e.applied), (Some(2), Some(1)));
    assert_eq!(e.ids, vec![-1, -2, 123, -3]);
    for bad in ["!", "AAIF", "AAIEAA", "AAIE_P8="] {
        assert_eq!(parse_enchant(Some(bad)).state, EnchantState::Invalid);
    }
}

#[test]
fn completion_zero_and_malformed() {
    let zero = decode_completion_counts("AAAAAA==").unwrap();
    assert_eq!(zero.by_index.len(), 75);
    assert!(zero.by_index.iter().all(|n| *n == Some(0)));
    for bad in ["", "!", "AAAA", "AAAAAAA=", "AQAAAA=="] {
        assert!(decode_completion_counts(bad).is_err());
    }
}

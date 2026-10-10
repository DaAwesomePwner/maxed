use maxed_engine::parse_enchant;
use maxed_types::EnchantState;
#[test]
fn invalid_inputs_are_bounded_and_never_panic() {
    assert_eq!(
        parse_enchant(Some(&"A".repeat(4097))).state,
        EnchantState::Invalid
    );
    for value in [
        "é", "=AAA", "AA=A", "A===", "AAIE_f8A", "ABIE", "AAIE_P8=", "AAIE_w==",
    ] {
        assert_eq!(
            parse_enchant(Some(value)).state,
            EnchantState::Invalid,
            "{value}"
        );
    }
}
#[test]
fn locked_and_empty_slots_count_differently() {
    let empty = parse_enchant(Some("AAIE__8="));
    assert_eq!(empty.slots, Some(1));
    assert_eq!(empty.applied, Some(0));
    let locked = parse_enchant(Some("AAIE_v8="));
    assert_eq!(locked.slots, Some(0));
    assert_eq!(locked.applied, Some(0));
    assert_eq!(parse_enchant(Some("AAIE__8")).ids, vec![-1]);
}

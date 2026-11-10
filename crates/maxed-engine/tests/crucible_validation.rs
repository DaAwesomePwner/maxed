use crate::common::*;
use maxed_protocol::{defs::CrucibleResponse, Packet};

fn check(body: &str, expected: Option<f64>) {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(1, vec![string(128, "synthetic")])]),
    );
    apply(
        &mut e,
        4000,
        Packet::CrucibleResponse(CrucibleResponse {
            crucibleIds: vec![0],
            crucibleJsons: vec![format!(r#"{{"array":[{{"id":"synthetic",{body}}}]}}"#)],
        }),
    );
    let learned = e.take_crucible_definitions();
    assert_eq!(learned.len(), usize::from(expected.is_some()), "{body}");
    if let Some(amount) = expected {
        assert_eq!(learned[0].damage_multiplier, amount);
    }
    apply(&mut e, 5000, shoot(1));
    apply(&mut e, 6000, hit(1));
    assert_eq!(
        e.snapshot().fights[0].total_damage,
        100. * expected.unwrap_or(1.)
    );
    assert_eq!(
        e.reconstruction_coverage()["assumedCrucibleDefault"],
        u64::from(expected.is_none())
    );
}

#[test]
fn crucible_numeric_type_accepts_float_but_nonnumeric_type_stays_unknown() {
    for ty in ["5", "5.0", "5e0"] {
        check(
            &format!(r#""crucible":{{"bonuses":[{{"type":{ty},"amount":1.5}}]}}"#),
            Some(1.5),
        );
    }
    for ty in [r#""5""#, "null", "true", "{}", "[]"] {
        check(
            &format!(
                r#""crucible":{{"bonuses":[{{"type":5,"amount":1.5}},{{"type":{ty},"amount":1.5}}]}}"#
            ),
            None,
        );
    }
}

#[test]
fn crucible_nested_bonus_consensus_is_independent_of_key_order() {
    for (first, second) in [("z", "a"), ("a", "z")] {
        for (amount, expected) in [(1.5, Some(1.5)), (2., None)] {
            check(
                &format!(
                    r#""{first}":{{"bonuses":[{{"type":5,"amount":1.5}}]}},"{second}":[{{"nested":{{"bonuses":[{{"type":5.0,"amount":{amount}}}]}}}}]"#
                ),
                expected,
            );
        }
    }
    check(
        r#""crucible":{"bonuses":[{"type":5,"amount":1.5},{"type":5,"amount":2}]}"#,
        None,
    );
}

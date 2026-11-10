use crate::common::*;
use maxed_protocol::{defs::CrucibleResponse, Packet};

fn response(json: &str) -> Packet {
    Packet::CrucibleResponse(CrucibleResponse {
        crucibleIds: vec![0],
        crucibleJsons: vec![json.into()],
    })
}

#[test]
fn late_definitions_resolve_both_channels_without_rewriting_previous_hits() {
    for channel in [128, 155] {
        for (body, multiplier) in [
            (r#"{"bonuses":[{"type":1,"amount":7}]}"#, 1.),
            (
                r#"{"bonuses":[{"type":5,"amount":1.5},{"type":5,"amount":2}]}"#,
                1.5,
            ),
        ] {
            let mut e = engine();
            setup(&mut e);
            apply(
                &mut e,
                3000,
                tick(vec![status(1, vec![string(channel, "synthetic-bonus")])]),
            );
            apply(&mut e, 4000, shoot(1));
            apply(&mut e, 5000, hit(1));
            let reason = if channel == 128 {
                "assumedCrucibleDefault"
            } else {
                "assumedBloodRitualDefault"
            };
            assert_eq!(e.reconstruction_coverage()[reason], 1);
            let json = format!(r#"{{"array":[{{"id":"synthetic-bonus","bloodRitual":{body}}}]}}"#);
            apply(&mut e, 6000, response(&json));
            apply(&mut e, 7000, shoot(2));
            apply(&mut e, 8000, hit(2));
            assert_eq!(e.reconstruction_coverage()[reason], 1);
            assert_eq!(
                e.snapshot().fights[0].total_damage,
                100. + 100. * multiplier
            );
        }
    }
}

fn def(id: &str, amount: f64) -> maxed_types::CrucibleDef {
    maxed_types::CrucibleDef {
        id: id.into(),
        damage_multiplier: amount,
    }
}

#[test]
fn seed_drain_upsert_and_catalog_precedence() {
    use maxed_engine::Engine;
    use std::sync::Arc;
    let mut cat = catalog();
    cat.crucible_bonuses = vec![def("synthetic-bonus", 2.)];
    let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
    e.seed_crucible_definitions([def("synthetic-bonus", 1.5), def("seed-only", 1.25)]);
    e.seed_crucible_definitions([def("synthetic-bonus", 1.5)]);
    assert!(e.take_crucible_definitions().is_empty());
    setup(&mut e);
    let json = r#"{"array":[{"id":"z","crucible":{}},{"id":"synthetic-bonus","crucible":{"bonuses":[{"type":5,"amount":3}]}},{"id":"a"}]}"#;
    apply(&mut e, 3000, response(json));
    apply(&mut e, 3100, response(json));
    let drained = e.take_crucible_definitions();
    assert_eq!(
        drained,
        vec![def("a", 1.), def("synthetic-bonus", 3.), def("z", 1.)]
    );
    assert!(e.take_crucible_definitions().is_empty());
    apply(&mut e, 3200, response(json));
    assert!(e.take_crucible_definitions().is_empty());
    apply(
        &mut e,
        3300,
        tick(vec![status(
            1,
            vec![string(128, "synthetic-bonus"), string(155, "seed-only")],
        )]),
    );
    apply(&mut e, 4000, shoot(1));
    apply(&mut e, 5000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 250.);
    assert_eq!(e.reconstruction_coverage()["assumedCrucibleDefault"], 0);
    assert_eq!(e.reconstruction_coverage()["assumedBloodRitualDefault"], 0);
    apply(
        &mut e,
        6000,
        response(r#"{"array":[{"id":"z","crucible":{"bonuses":[{"type":5,"amount":2}]}}]}"#),
    );
    assert_eq!(e.take_crucible_definitions(), vec![def("z", 2.)]);
}

#[test]
fn learned_definitions_survive_new_connection_and_identity() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        response(r#"{"array":[{"id":"synthetic-bonus","crucible":{}}]}"#),
    );
    connection(&mut e, 2, 4000, map("The Void"));
    connection(&mut e, 2, 5000, create(1, 2));
    let mut p = player();
    p.status.stats.push(string(128, "synthetic-bonus"));
    connection(&mut e, 2, 6000, update(vec![p, target()], vec![]));
    connection(&mut e, 2, 7000, shoot(1));
    connection(&mut e, 2, 8000, hit(1));
    assert_eq!(e.reconstruction_coverage()["assumedCrucibleDefault"], 0);
    assert_eq!(e.snapshot().fights.last().unwrap().total_damage, 100.);
    assert_eq!(
        e.take_crucible_definitions(),
        vec![def("synthetic-bonus", 1.)]
    );
}

#[test]
fn malformed_and_oversized_definitions_do_not_become_known() {
    let mut e = engine();
    setup(&mut e);
    for json in [
        "{".into(),
        "[]".into(),
        r#"{"array":{}}"#.into(),
        r#"{"array":[{"id":12}]}"#.into(),
        r#"{"array":[{"id":"bad","crucible":{"bonuses":{}}}]}"#.into(),
        format!(
            r#"{{"array":[{{"id":"bad","padding":"{}"}}]}}"#,
            "x".repeat(65_535)
        ),
        format!(
            r#"{{"array":[{{"id":"bad","nested":{}{}}}]}}"#,
            "[".repeat(140),
            "]".repeat(140)
        ),
        format!(
            r#"{{"array":[{{"id":"bad","nested":{}{}}}]}}"#,
            "[".repeat(40),
            "]".repeat(40)
        ),
    ] {
        apply(&mut e, 3000, response(&json));
        assert!(e.take_crucible_definitions().is_empty());
    }
    for amount in ["0", "-1", "101", "1e400", "null", r#""NaN""#] {
        let json = format!(
            r#"{{"array":[{{"id":"bad","bloodRitual":{{"bonuses":[{{"type":5,"amount":{amount}}},{{"type":5,"amount":2}}]}}}}]}}"#
        );
        apply(&mut e, 3000, response(&json));
        assert!(e.take_crucible_definitions().is_empty());
    }
    e.seed_crucible_definitions([
        def("bad", f64::NAN),
        def("bad", f64::INFINITY),
        def("bad", 0.),
        def("bad", -1.),
        def("bad", 101.),
    ]);
    apply(
        &mut e,
        4000,
        tick(vec![status(
            1,
            vec![string(128, "bad"), string(155, "absent")],
        )]),
    );
    apply(&mut e, 5000, shoot(1));
    apply(&mut e, 6000, hit(1));
    assert_eq!(e.reconstruction_coverage()["assumedCrucibleDefault"], 1);
    assert_eq!(e.reconstruction_coverage()["assumedBloodRitualDefault"], 1);
}

#[test]
fn definition_capacity_is_bounded_but_existing_ids_can_update() {
    let mut e = engine();
    setup(&mut e);
    for batch in 0..5 {
        let entries: Vec<_> = (0..1000)
            .map(|i| serde_json::json!({"id": format!("synthetic-{}", batch * 1000 + i)}))
            .collect();
        apply(
            &mut e,
            3000,
            response(&serde_json::json!({"array": entries}).to_string()),
        );
    }
    assert_eq!(e.take_crucible_definitions().len(), 4096);
    apply(
        &mut e,
        4000,
        response(
            r#"{"array":[{"id":"synthetic-0","crucible":{"bonuses":[{"type":5,"amount":2}]}},{"id":"overflow"}]}"#,
        ),
    );
    assert_eq!(e.take_crucible_definitions(), vec![def("synthetic-0", 2.)]);
    apply(
        &mut e,
        5000,
        tick(vec![status(1, vec![string(128, "overflow")])]),
    );
    apply(&mut e, 6000, shoot(1));
    apply(&mut e, 7000, hit(1));
    assert_eq!(e.reconstruction_coverage()["assumedCrucibleDefault"], 1);
}

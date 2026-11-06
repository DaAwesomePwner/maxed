use crate::common;
use common::*;
use maxed_types::*;
#[test]
fn character_display_name_strips_suffix_without_changing_identity() {
    let mut e = engine();
    setup(&mut e);
    let original = e.snapshot().character.unwrap();
    for (index, (raw, display)) in [
        ("Name,xxxx", "Name"),
        ("Name,other,more", "Name"),
        ("Plain", "Plain"),
        ("", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let events = apply(
            &mut e,
            3000 + index as u64 * 1000,
            tick(vec![status(1, vec![string(31, raw)])]),
        );
        let character = e.snapshot().character.unwrap();
        assert_eq!(character.name.as_deref(), Some(display));
        assert_eq!(character.id, original.id);
        assert_eq!(character.identity_verified, original.identity_verified);
        assert!(events
            .iter()
            .all(|event| !matches!(event, EngineEvent::CharacterMerged { .. })));
        for event in events {
            if let EngineEvent::CharacterUpdated(view) = event {
                assert_eq!(view.name.as_deref(), Some(display));
            }
        }
    }
}
#[test]
fn character_is_published_before_merge_and_fame_references() {
    let mut e = engine();
    let mut known = std::collections::HashSet::new();
    let mut merges = 0;
    let mut fame_changes = 0;
    for (at, packet) in [
        (0, map("The Void")),
        (1000, create(1, 1)),
        (2000, update(vec![player()], vec![])),
        (3000, tick(vec![status(1, vec![stat(57, 101)])])),
        (4000, tick(vec![status(1, vec![stat(57, 101)])])),
    ] {
        let events = apply(&mut e, at, packet);
        for event in events {
            match event {
                EngineEvent::CharacterUpdated(character) => {
                    known.insert(character.id);
                }
                EngineEvent::CharacterMerged {
                    provisional_id,
                    verified_id,
                } => {
                    assert!(known.contains(&provisional_id));
                    assert!(known.contains(&verified_id));
                    merges += 1;
                }
                EngineEvent::FameChanged { character_id, .. } => {
                    assert!(known.contains(&character_id));
                    fame_changes += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!(merges, 1);
    assert_eq!(fame_changes, 2);
    assert_eq!(e.snapshot().character.unwrap().fame, Some(101));
}
#[test]
fn fame_is_observed_once_and_death_total_is_not_a_delta() {
    let mut e = engine();
    setup(&mut e);
    assert!(
        apply(&mut e, 3000, tick(vec![status(1, vec![stat(39, 500)])]))
            .iter()
            .all(|e| !matches!(e, EngineEvent::FameChanged { .. }))
    );
    let events = apply(&mut e, 4000, tick(vec![status(1, vec![stat(57, 101)])]));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, EngineEvent::FameChanged { fame: 101, .. }))
            .count(),
        1
    );
    assert!(
        apply(&mut e, 5000, tick(vec![status(1, vec![stat(57, 101)])]))
            .iter()
            .all(|e| !matches!(
                e,
                EngineEvent::CharacterUpdated(_) | EngineEvent::FameChanged { .. }
            ))
    );
    assert!(apply(&mut e, 6000, death())
        .iter()
        .all(|e| !matches!(e, EngineEvent::FameChanged { .. })));
    assert_eq!(e.snapshot().character.unwrap().fame, Some(101));
}
#[test]
fn gear_and_unknown_caps_remain_observation_time_values() {
    let mut e = engine();
    setup(&mut e);
    let old = e.snapshot().character.unwrap();
    assert_eq!(old.maxed, None);
    apply(&mut e, 3000, tick(vec![status(1, vec![stat(8, 201)])]));
    assert_eq!(old.equipment[0].item_type, Some(200));
    assert_eq!(
        e.snapshot().character.unwrap().equipment[0].item_type,
        Some(201)
    );
}
#[test]
fn full_class_caps_require_all_observed_values_boosts_and_zero_exalt() {
    use maxed_engine::Engine;
    use std::sync::Arc;
    let mut cat = catalog();
    let pairs = [
        (0, 46),
        (3, 47),
        (20, 48),
        (21, 49),
        (22, 50),
        (28, 53),
        (26, 51),
        (27, 52),
    ];
    cat.classes[0].stat_caps = pairs
        .iter()
        .map(|&(stat_id, _)| StatView { stat_id, value: 50 })
        .collect();
    let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
    setup(&mut e);
    let stats: Vec<_> = pairs
        .into_iter()
        .flat_map(|(s, b)| [stat(s, 60), stat(b, 10)])
        .collect();
    apply(&mut e, 2500, tick(vec![status(1, stats)]));
    assert_eq!(e.snapshot().character.unwrap().maxed, None);
    let stats = (105..=112).map(|id| stat(id, 0)).collect();
    apply(&mut e, 3000, tick(vec![status(1, stats)]));
    assert_eq!(e.snapshot().character.unwrap().maxed, Some(8));
    apply(&mut e, 4000, tick(vec![status(1, vec![stat(20, 59)])]));
    assert_eq!(e.snapshot().character.unwrap().maxed, Some(7));
    for (at, level) in [(5000, 2), (6000, -1)] {
        apply(&mut e, at, tick(vec![status(1, vec![stat(105, level)])]));
        assert_eq!(e.snapshot().character.unwrap().maxed, None);
    }
    apply(&mut e, 7000, tick(vec![status(1, vec![stat(105, 0)])]));
    assert_eq!(e.snapshot().character.unwrap().maxed, Some(7));
}

#[test]
fn equipment_empty_evidence_distinguishes_wire_empty_item_absent_and_invalid() {
    let mut e = engine();
    apply(&mut e, 0, map("Synthetic Map"));
    apply(&mut e, 1000, create(1, 1));
    let absent = serde_json::to_value(e.snapshot().character.unwrap()).unwrap();
    assert!(absent["equipment"][0]["observed_empty"].is_null());
    for (index, (wire, expected, item)) in [
        (-1, Some(true), None),
        (0, Some(false), Some(0)),
        (200, Some(false), Some(200)),
        (65535, Some(false), Some(65535)),
        (-2, None, None),
        (65536, None, None),
    ]
    .into_iter()
    .enumerate()
    {
        let events = apply(
            &mut e,
            2000 + index as u64 * 1000,
            tick(vec![status(1, vec![stat(8, wire)])]),
        );
        let snapshot = e.snapshot();
        let character = snapshot.character.as_ref().unwrap();
        assert_eq!(character.equipment[0].item_type, item);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            json["character"]["equipment"][0]["observed_empty"],
            serde_json::json!(expected)
        );
        assert!(json["character"].get("calculation_revision").is_none());
        for event in events {
            if let EngineEvent::CharacterUpdated(view) = event {
                assert!(serde_json::to_value(view)
                    .unwrap()
                    .get("calculation_revision")
                    .is_none());
            }
        }
    }
}

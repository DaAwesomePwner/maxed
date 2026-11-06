use crate::{common::*, d_support};
use maxed_engine::Engine;
use maxed_protocol::Packet;
use maxed_types::*;
use std::sync::Arc;

fn killed_by(name: &str) -> Packet {
    let Packet::Death(mut packet) = death() else {
        unreachable!()
    };
    packet.killedBy = name.into();
    Packet::Death(packet)
}

fn detail(engine: &Engine) -> DeathDetail {
    engine.snapshot().run.unwrap().death.unwrap()
}

fn hp(engine: &mut Engine, at: u64, value: i32) {
    apply(engine, at, tick(vec![status(1, vec![stat(1, value)])]));
}

#[test]
fn killer_text_and_type_are_kept() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, tick(vec![status(1, vec![stat(57, 123)])]));
    let events = apply(&mut e, 4_000, killed_by("  sYnThEtIc 300  "));
    let expected = DeathDetail {
        killed_by: Some("sYnThEtIc 300".into()),
        killed_by_type: Some(300),
        fame: Some(123),
        hp_trace: vec![],
    };
    assert_eq!(detail(&e), expected);
    assert!(events.iter().any(|event| matches!(event,
        EngineEvent::RunEnded(run) if run.death.as_ref() == Some(&expected))));
}

#[test]
fn empty_killer_is_none() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, killed_by(" \t\n "));
    assert_eq!(detail(&e).killed_by, None);
    assert_eq!(detail(&e).killed_by_type, None);
}

#[test]
fn empty_killer_cannot_match_empty_catalog_name() {
    let mut catalog = catalog();
    catalog.objects[1].name.clear();
    let mut e = Engine::new(config(), Arc::new(catalog)).unwrap();
    setup(&mut e);
    apply(&mut e, 3_000, killed_by(" \t "));
    assert_eq!(detail(&e).killed_by, None);
    assert_eq!(detail(&e).killed_by_type, None);
}

#[test]
fn unmatched_killer_keeps_raw_text() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, killed_by("  Synthetic unknown!  "));
    assert_eq!(detail(&e).killed_by.as_deref(), Some("Synthetic unknown!"));
    assert_eq!(detail(&e).killed_by_type, None);
}

#[test]
fn ambiguous_killer_has_no_type() {
    let mut catalog = catalog();
    catalog.objects[2].name = "SYNTHETIC 300".into();
    let mut e = Engine::new(config(), Arc::new(catalog)).unwrap();
    setup(&mut e);
    apply(&mut e, 3_000, killed_by("Synthetic 300"));
    assert_eq!(detail(&e).killed_by.as_deref(), Some("Synthetic 300"));
    assert_eq!(detail(&e).killed_by_type, None);
}

#[test]
fn hp_buffer_keeps_last_eight_seconds_and_64_samples() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, tick(vec![status(1, vec![stat(0, 800)])]));
    for i in 0..100 {
        hp(&mut e, 100_000 + i * 100_000, i as i32);
    }
    apply(&mut e, 10_000_000, death());
    let trace = detail(&e).hp_trace;
    assert_eq!(trace.len(), 51);
    assert!(trace.len() <= 64);
    for (i, sample) in trace.iter().enumerate() {
        assert_eq!(sample.at, Timestamp(5_000_000 + i as u64 * 100_000));
        assert_eq!(sample.hp, 49 + i as i32);
        assert_eq!(sample.max_hp, Some(800));
    }
}

#[test]
fn dense_hp_samples_drop_oldest_at_64() {
    let mut e = engine();
    setup(&mut e);
    for i in 0..100 {
        hp(&mut e, 10_000 + i * 1_000, i as i32);
    }
    apply(&mut e, 110_000, death());
    let trace = detail(&e).hp_trace;
    assert_eq!(trace.len(), 64);
    assert_eq!(trace.first().unwrap().hp, 36);
    assert_eq!(trace.last().unwrap().hp, 99);
}

#[test]
fn no_hp_samples_gives_empty_trace() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, death());
    assert!(detail(&e).hp_trace.is_empty());
}

#[test]
fn samples_after_death_are_ignored() {
    let mut e = engine();
    setup(&mut e);
    hp(&mut e, 3_000, 100);
    apply(&mut e, 4_000, death());
    let before = detail(&e);
    hp(&mut e, 5_000, 200);
    apply(&mut e, 6_000, killed_by("Synthetic 300"));
    assert_eq!(detail(&e), before);
}

#[test]
fn buffer_clears_on_map_change() {
    let mut e = engine();
    setup(&mut e);
    hp(&mut e, 3_000, 100);
    apply(&mut e, 4_000, map("The Shatters"));
    apply(&mut e, 5_000, death());
    assert!(detail(&e).hp_trace.is_empty());
}

#[test]
fn create_clears_buffer_and_pending_map_hp_is_out_of_scope() {
    let mut e = engine();
    setup(&mut e);
    hp(&mut e, 3_000, 100);
    apply(&mut e, 4_000, create(1, 2));
    hp(&mut e, 5_000, 200);
    apply(&mut e, 6_000, death());
    assert!(detail(&e).hp_trace.is_empty());
}

#[test]
fn create_clears_buffer_and_a_new_life_samples_again() {
    for die_first in [false, true] {
        let mut e = engine();
        setup(&mut e);
        hp(&mut e, 3_000, 100);
        if die_first {
            apply(&mut e, 4_000, death());
        }
        apply(&mut e, 5_000, create(1, 2));
        // A replacing CREATE belongs to the next map; pre-map HP is out of scope.
        hp(&mut e, 6_000, 200);
        apply(&mut e, 7_000, map("The Shatters"));
        hp(&mut e, 8_000, 300);
        apply(&mut e, 9_000, death());
        assert_eq!(
            detail(&e).hp_trace,
            vec![HpSample {
                at: Timestamp(8_000),
                hp: 300,
                max_hp: None,
            }]
        );
    }
}

#[test]
fn only_explicit_numeric_local_hp_is_sampled_with_current_positive_max() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3_000,
        update(
            vec![object(1, 100, vec![stat(1, 500), stat(0, 800)])],
            vec![],
        ),
    );
    apply(
        &mut e,
        4_000,
        tick(vec![
            status(2, vec![stat(1, 900)]),
            status(1, vec![stat(0, 900)]),
        ]),
    );
    apply(&mut e, 5_000, tick(vec![status(1, vec![string(1, "500")])]));
    hp(&mut e, 6_000, 500);
    apply(
        &mut e,
        7_000,
        tick(vec![status(1, vec![stat(1, -1), stat(0, 0)])]),
    );
    apply(&mut e, 8_000, death());
    assert_eq!(
        detail(&e).hp_trace,
        vec![
            HpSample {
                at: Timestamp(3_000),
                hp: 500,
                max_hp: Some(800)
            },
            HpSample {
                at: Timestamp(6_000),
                hp: 500,
                max_hp: Some(900)
            },
            HpSample {
                at: Timestamp(7_000),
                hp: -1,
                max_hp: None
            },
        ]
    );
}

#[test]
fn death_after_clear_keeps_cleared_and_records_detail() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, victory());
    apply(&mut e, 4_000, death());
    let run = e.snapshot().run.unwrap();
    assert_eq!(run.outcome, Some(Outcome::Cleared));
    assert_eq!(run.completed_at, Some(Timestamp(3_000)));
    assert_eq!(run.died_at, Some(Timestamp(4_000)));
    assert_eq!(run.death.unwrap().killed_by.as_deref(), Some("Synthetic"));
}

#[test]
fn realm_death_without_run_keeps_no_detail() {
    let mut e = engine();
    apply(&mut e, 0, map("Realm"));
    apply(&mut e, 1_000, create(1, 1));
    apply(&mut e, 2_000, update(vec![player()], vec![]));
    let events = apply(&mut e, 3_000, death());
    assert!(e.snapshot().run.is_none());
    assert_eq!(
        e.snapshot().character.unwrap().died_at,
        Some(Timestamp(3_000))
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::PlayerDied { run_id: None, .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::RunEnded(_))));
}

#[test]
fn announced_local_death_does_not_set_detail() {
    let mut e = engine();
    setup(&mut e);
    let events = apply(
        &mut e,
        3_000,
        d_support::player_death("Synthetic", 20, "Synthetic 300", 0),
    );
    let snapshot = e.snapshot();
    assert!(snapshot.run.unwrap().death.is_none());
    assert!(snapshot.character.unwrap().died_at.is_none());
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::PlayerDied { .. })));
}

#[test]
fn old_run_json_without_death_deserializes() {
    let mut e = engine();
    setup(&mut e);
    let mut json = serde_json::to_value(e.snapshot().run.unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("death");
    let run: RunView = serde_json::from_value(json).unwrap();
    assert!(run.death.is_none());
}

#[test]
fn death_without_character_has_no_fame() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1_000, death());
    assert_eq!(detail(&e).fame, None);
}

#[test]
fn trace_window_uses_death_capture_time_in_microseconds() {
    let mut e = engine();
    setup(&mut e);
    hp(&mut e, 3_000, 10);
    hp(&mut e, 3_001, 20);
    apply(&mut e, 5_003_001, death());
    assert_eq!(
        detail(&e).hp_trace,
        vec![HpSample {
            at: Timestamp(3_001),
            hp: 20,
            max_hp: None,
        }]
    );
}

mod common;
mod d_support;
use d_support::*;
#[test]
fn snapshot_run_totals_equal_pure_function() {
    let mut e = started();
    kill(&mut e);
    let s = e.snapshot();
    let totals = s.run_totals.unwrap();
    assert_eq!(totals, maxed_engine::run_totals(&s.fights, None));
    assert_eq!(totals.kills, 1);
    assert_eq!(totals.fights, 1);
}
#[test]
fn run_totals_partial_when_any_fight_estimated() {
    let mut e = started();
    apply(
        &mut e,
        4500,
        tick(vec![status(1, vec![string(128, "unknown-active")])]),
    );
    apply(&mut e, 5000, shoot(10));
    apply(&mut e, 6000, hit(10));
    assert!(e.snapshot().run_totals.unwrap().partial);
}
#[test]
#[ignore = "release timing gate"]
fn snapshot_2048_fights_regression_bound() {
    let mut cfg = config();
    cfg.max_active_fights = 2048;
    let mut e = maxed_engine::Engine::new(cfg, std::sync::Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            (3..52).map(|i| player_obj(i, 100, "SynthBravo")).collect(),
            vec![],
        ),
    );
    apply(&mut e, 4000, tick(vec![]));
    for id in 100..2148 {
        apply(&mut e, 5000, update(vec![object(id, 300, vec![])], vec![]));
        for owner in std::iter::once(1).chain(3..52) {
            let maxed_protocol::Packet::Damage(mut p) = damage(owner, owner as u16, 1) else {
                unreachable!()
            };
            p.targetId = id;
            apply(&mut e, 6000, maxed_protocol::Packet::Damage(p));
        }
    }
    // The historical whole-snapshot baseline was 43.219 ms p95; see
    // docs/release/performance.md. The 26.678 ms
    // clone-only measurement is not the whole-snapshot baseline.
    const LEGACY_BASELINE: std::time::Duration = std::time::Duration::from_micros(43_219);
    const PD_ALLOWANCE: std::time::Duration = std::time::Duration::from_millis(2);
    for _ in 0..5 {
        std::hint::black_box(e.snapshot());
    }
    let mut times = Vec::new();
    for _ in 0..100 {
        let now = std::time::Instant::now();
        let s = std::hint::black_box(e.snapshot());
        times.push(now.elapsed());
        assert_eq!(s.fights.len(), 2048);
        assert!(s.fights.iter().all(|f| f.players.len() == 50));
        assert_eq!(
            s.fights
                .iter()
                .filter(|f| f.player_series.is_some())
                .count(),
            maxed_engine::SNAPSHOT_SERIES_LIMIT
        );
        assert_eq!(s.run_totals.unwrap().players.len(), 50);
    }
    times.sort();
    let ceiling = LEGACY_BASELINE + PD_ALLOWANCE;
    println!(
        "whole snapshot median={:?}, p95={:?}, legacy baseline={:?}, ceiling={:?}",
        times[49], times[94], LEGACY_BASELINE, ceiling
    );
    assert!(
        times[94] < ceiling,
        "p95 {:?} must be below legacy baseline {:?} + PD allowance {:?}",
        times[94],
        LEGACY_BASELINE,
        PD_ALLOWANCE
    );
}

#[test]
fn fame_delta_uses_only_fresh_local_samples_in_current_run() {
    let mut e = started();
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, None);
    // setup already supplied the first local fame sample (100).
    apply(&mut e, 5000, tick(vec![]));
    apply(&mut e, 6000, tick(vec![]));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, None);
    apply(&mut e, 7000, tick(vec![status(2, vec![stat(57, 999)])]));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, None);
    apply(&mut e, 8000, tick(vec![status(1, vec![stat(57, 112)])]));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, Some(12));
    apply(&mut e, 9000, map("The Void"));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, None);
}

#[test]
fn fame_samples_do_not_cross_character_or_death_boundaries() {
    let mut e = started();
    apply(&mut e, 5000, tick(vec![status(1, vec![stat(57, 100)])]));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, Some(0));
    apply(&mut e, 6000, create(3, 2));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, None);
    // Replacement CREATE_SUCCESS belongs to the next map until MAPINFO binds it.
    apply(&mut e, 6500, map("The Void"));
    apply(
        &mut e,
        7000,
        update(vec![object(3, 100, vec![stat(57, 200)])], vec![]),
    );
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, None);
    apply(&mut e, 8000, tick(vec![status(3, vec![stat(57, 203)])]));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, Some(3));
    apply(&mut e, 9000, death());
    apply(&mut e, 10000, tick(vec![status(3, vec![stat(57, 500)])]));
    assert_eq!(e.snapshot().run_totals.unwrap().fame_delta, Some(3));
}

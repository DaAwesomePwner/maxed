// Synthetic diagnostics and snapshot projection release gates.
use super::*;
use crate::Engine;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
fn workload() -> Engine {
    let mut cfg = config();
    cfg.max_active_fights = 2048;
    let mut e = Engine::new(cfg, std::sync::Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            (3..52)
                .map(|i| object(i, 100, vec![string(31, "SynthBravo")]))
                .collect(),
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
    e
}
fn measured<T>(mut make: impl FnMut() -> T) -> (Duration, Duration) {
    for _ in 0..5 {
        black_box(make());
    }
    let mut times = Vec::new();
    for _ in 0..100 {
        let at = Instant::now();
        let result = black_box(make());
        times.push(at.elapsed());
        drop(result);
    }
    times.sort();
    (times[49], times[94])
}
#[test]
#[ignore = "release diagnostic; PD additions retain a separate 2 ms gate"]
fn snapshot_cost_breakdown_2048_fights_50_players() {
    let e = workload();
    let legacy: Vec<_> = e
        .fights
        .values()
        .map(|f| f.project_uncached(false))
        .collect();
    assert_eq!(legacy.len(), 2048);
    assert!(legacy
        .iter()
        .all(|f| f.players.len() == 50 && f.player_series.is_none()));
    let full: Vec<_> = e
        .fights
        .values()
        .map(|f| f.project_uncached(true))
        .collect();
    for (name, time) in [
        ("legacy FightView clone only", measured(|| legacy.clone())),
        (
            "baseline snapshot: no totals, no series",
            measured(|| {
                e.snapshot_fields(
                    e.fights
                        .values()
                        .map(|f| f.project_uncached(false))
                        .collect(),
                    None,
                )
            }),
        ),
        (
            "snapshot + uncached series, no totals",
            measured(|| {
                e.snapshot_fields(
                    e.fights
                        .values()
                        .map(|f| f.project_uncached(true))
                        .collect(),
                    None,
                )
            }),
        ),
        (
            "run_totals reducer only",
            measured(|| crate::engine::totals::run_totals(&full, None)),
        ),
        (
            "cached run_totals lookup only",
            measured(|| e.snapshot_run_totals()),
        ),
        (
            "cached fight/series clone + snapshot, no totals",
            measured(|| {
                let selected = e.snapshot_series_selection();
                e.snapshot_fields(
                    e.fights
                        .iter()
                        .map(|(&key, f)| f.project_snapshot(selected.contains(key)))
                        .collect(),
                    None,
                )
            }),
        ),
        ("current full snapshot warm", measured(|| e.snapshot())),
    ] {
        println!(
            "{name}: median={:.3} ms p95={:.3} ms",
            time.0.as_secs_f64() * 1000.,
            time.1.as_secs_f64() * 1000.
        );
    }
}

#[test]
#[ignore = "release gate: cached PD projection must stay below 2 ms"]
fn snapshot_pd_additions_under_2ms_p95() {
    let mut e = workload();
    let snapshot = e.snapshot();
    assert_eq!(snapshot.fights.len(), 2048);
    assert!(snapshot.fights.iter().all(|f| f.players.len() == 50));
    assert_eq!(snapshot.run_totals.as_ref().unwrap().players.len(), 50);
    drop(snapshot);
    // Time exactly the projection values copied into an owned warm snapshot: cached
    // totals lookup/copy plus prioritized selection and the bounded series copy.
    // Legacy FightView cloning, sorting and source vectors are excluded, but
    // all projection selection/row/string/point allocations remain inside the timer.
    let pd_projection = || {
        let totals = e.snapshot_run_totals();
        let series = e.snapshot_series_projection();
        (totals, series)
    };
    let (totals, series) = pd_projection();
    assert_eq!(totals.unwrap().players.len(), 50);
    assert_eq!(series.len(), 2048);
    assert_eq!(
        series.iter().flatten().count(),
        crate::SNAPSHOT_SERIES_LIMIT
    );
    assert!(series
        .iter()
        .flatten()
        .all(|rows| (5..=6).contains(&rows.len())
            && rows.iter().any(|p| p.is_local)
            && rows.iter().all(|p| p.points.len() == 1)));
    drop(series);
    let (median, p95) = measured(pd_projection);
    println!("cached PD additions median={median:?}, p95={p95:?}, ceiling=2ms");
    let cached_p95 = p95;
    // Invalidate exactly one fight before each sample. Include both rebuilding
    // its projected series and replacing its aggregate contribution in timing.
    // Include report acceptance as a conservative extra cost: it updates real
    // damage, sources, series and revision, rather than merely bumping a flag.
    let key = *e.fights.keys().next_back().unwrap();
    let before = e.run_totals_cache.lock().unwrap().contribution_builds;
    let lookups = e.run_totals_cache.lock().unwrap().fight_lookups;
    let selection_lookups = e.snapshot_series_cache.lock().unwrap().fight_lookups;
    let (median, p95) = measured(|| {
        e.report(key.0, 1, 1, true, Ok(1), &mut Vec::new());
        let totals = e.snapshot_run_totals();
        let series = e.snapshot_series_projection();
        (totals, series)
    });
    assert_eq!(
        e.run_totals_cache.lock().unwrap().contribution_builds,
        before + 105
    );
    assert_eq!(
        e.run_totals_cache.lock().unwrap().fight_lookups,
        lookups + 105
    );
    assert_eq!(
        e.snapshot_series_cache.lock().unwrap().fight_lookups,
        selection_lookups + 105
    );
    assert_cached_projection_matches_reconstruction(&e);
    println!("dirty-one-fight PD additions median={median:?}, p95={p95:?}, ceiling=2ms");
    assert!(
        cached_p95 < Duration::from_millis(2),
        "cached PD additions p95 {cached_p95:?} must stay below 2 ms"
    );
    assert!(
        p95 < Duration::from_millis(2),
        "dirty-one-fight PD additions p95 {p95:?} must stay below 2 ms"
    );
}

#[test]
#[ignore = "release diagnostic: dirty PD step costs"]
fn snapshot_dirty_step_breakdown() {
    let mut e = workload();
    drop(e.snapshot());
    let key = *e.fights.keys().next_back().unwrap();
    let mut samples: [Vec<Duration>; 9] = Default::default();
    for i in 0..105 {
        let report = Instant::now();
        e.report(key.0, 1, 1, true, Ok(1), &mut Vec::new());
        let report = report.elapsed();
        let total = Instant::now();
        let totals = black_box(e.snapshot_run_totals());
        let series = Instant::now();
        let rows = e.snapshot_series_projection();
        let series = series.elapsed();
        let total = total.elapsed();
        let profile = e.run_totals_cache.lock().unwrap().profile;
        if i >= 5 {
            for (values, elapsed) in samples.iter_mut().zip([
                report,
                profile[0],
                Duration::from_nanos(
                    e.fights[&key]
                        .series_nanos
                        .load(std::sync::atomic::Ordering::Relaxed),
                ),
                profile[1],
                profile[2],
                profile[3],
                profile[4],
                series,
                total,
            ]) {
                values.push(elapsed);
            }
        }
        drop((totals, rows));
    }
    for (name, values) in [
        "hit acceptance/invalidation",
        "dirty discovery/maps",
        "dirty fight series only",
        "dirty fight projection/contribution",
        "subtract/add and rank index updates",
        "rank/share/DPS finalize",
        "totals owned clone",
        "bounded snapshot series selection/owned clone",
        "total PD snapshot projection",
    ]
    .into_iter()
    .zip(&mut samples)
    {
        values.sort();
        println!("{name}: median={:?}, p95={:?}", values[49], values[94]);
    }
    assert_cached_projection_matches_reconstruction(&e);
}

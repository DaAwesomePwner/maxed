use crate::d_support;
use d_support::*;
#[test]
fn series_align_with_buckets_after_compaction() {
    let mut e = started();
    for i in 1..1802 {
        let at = i * 1_000_000;
        apply(&mut e, at, tick(vec![]));
        apply(&mut e, at, damage(1, i as u16, 1));
    }
    let f = e.snapshot().fights.remove(0);
    let s = f.player_series.unwrap();
    assert!(f.buckets.len() < 1800);
    assert_eq!(s[0].points.len(), f.buckets.len());
    for (p, b) in s[0].points.iter().zip(&f.buckets) {
        assert_eq!(*p, b.total);
    }
}
#[test]
fn series_top5_plus_local() {
    let mut e = started();
    for i in 3..10 {
        apply(
            &mut e,
            4500,
            update(vec![player_obj(i, 100, "SynthBravo")], vec![]),
        );
        apply(&mut e, 5000, damage(i, i as u16, (i * 100) as u16));
    }
    let f = e.snapshot().fights.remove(0);
    let s = f.player_series.unwrap();
    assert_eq!(s.len(), 6);
    assert!(s.iter().any(|s| s.is_local));
    for p in f.players.iter().take(5) {
        assert!(s.iter().any(|s| s.player_id == p.player_id));
    }
}
#[test]
fn series_null_when_cap_trips_and_fight_unchanged() {
    let mut e = started();
    assert!(e.snapshot().fights[0].player_series.is_some());
    for id in 3..204 {
        apply(
            &mut e,
            5000,
            update(vec![player_obj(id, 100, "SynthBravo")], vec![]),
        );
        apply(&mut e, 5000, damage(id, id as u16, 1));
    }
    for i in 1..1001 {
        apply(&mut e, i * 1_000_000, tick(vec![]));
        apply(&mut e, i * 1_000_000, damage(1, (i + 1000) as u16, 1));
    }
    let f = e.snapshot().fights.remove(0);
    assert!(f.player_series.is_none());
    assert_eq!(f.total_damage, 1301.);
    assert_eq!(f.players.len(), 202);
    assert!(f.complete_capture);
    assert!(f.issues.is_empty());
    assert!(f.ended.is_none());
}

#[test]
fn local_zero_damage_still_has_aligned_series() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, tick(vec![]));
    apply(
        &mut e,
        4000,
        update(vec![player_obj(3, 100, "SynthBravo")], vec![]),
    );
    apply(&mut e, 5000, damage(3, 1, 100));
    let f = e.snapshot().fights.remove(0);
    let series = f.player_series.unwrap();
    let local = series.iter().find(|s| s.is_local).unwrap();
    assert_eq!(local.points, vec![0.; f.buckets.len()]);
}
#[test]
fn series_estimate_reconciliation_never_doubles_damage() {
    let mut e = started();
    apply(&mut e, 5000, shoot(10));
    apply(&mut e, 6000, hit(10));
    let f = e.snapshot().fights.remove(0);
    assert_eq!(
        f.player_series
            .unwrap()
            .iter()
            .flat_map(|s| &s.points)
            .sum::<f64>(),
        f.total_damage
    );
    apply(&mut e, 7000, damage(1, 10, 80));
    let f = e.snapshot().fights.remove(0);
    assert_eq!(f.total_damage, 180.);
    assert_eq!(
        f.player_series
            .unwrap()
            .iter()
            .flat_map(|s| &s.points)
            .sum::<f64>(),
        180.
    );
}

mod common;
mod d_support;
use d_support::*;
#[test]
fn run_context_frozen_at_run_start() {
    let mut e = engine();
    apply(
        &mut e,
        20_000_000,
        map_ctx(
            "The Void",
            50,
            "NexusPortal.SynthRealm",
            2.5,
            10,
            Some((100, 1000)),
        ),
    );
    let c = e.snapshot().run.unwrap().context.unwrap();
    assert_eq!(c.instance_age_micros, Some(10_000_000));
    assert_eq!(c.instance_cap, Some(50));
    apply(&mut e, 21_000_000, realm_score(200));
    assert_eq!(e.snapshot().run.unwrap().context, Some(c));
}
#[test]
fn snapshot_instance_population_tracks_roster() {
    let mut e = engine();
    setup(&mut e);
    assert_eq!(e.snapshot().instance.unwrap().population, Some(1));
    apply(
        &mut e,
        3000,
        update(vec![player_obj(3, 100, "SynthBravo")], vec![]),
    );
    apply(&mut e, 4000, update(vec![], vec![3]));
    let i = e.snapshot().instance.unwrap();
    assert_eq!(i.population, Some(1));
    assert_eq!(i.population_peak, Some(2));
    assert!(i.population_provisional);
    assert_eq!(e.snapshot().run.unwrap().party_size, Some(2));
}
#[test]
fn population_none_after_midstream_start() {
    let mut e = engine();
    apply(&mut e, 0, tick(vec![status(1, vec![])]));
    assert!(e.snapshot().instance.is_none());
    apply(&mut e, 1, map("Realm"));
    assert_eq!(e.snapshot().instance.unwrap().population, None);
}
#[test]
fn realm_score_only_when_mapinfo_had_scores() {
    let mut e = engine();
    apply(&mut e, 0, map("Realm"));
    apply(&mut e, 1, realm_score(10));
    assert!(e.snapshot().instance.unwrap().realm_score.is_none());
    apply(&mut e, 2, map_ctx("Realm", 85, "", 0., 0, Some((-1, 100))));
    apply(&mut e, 3, realm_score(10));
    assert_eq!(
        e.snapshot().instance.unwrap().realm_score.unwrap().current,
        10
    );
}

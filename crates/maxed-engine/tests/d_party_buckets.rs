use crate::d_support::*;
use maxed_types::FightView;

#[test]
fn others_exclude_local_and_unattributed() {
    let mut e = started();
    apply(
        &mut e,
        4100,
        update(
            vec![
                player_obj(3, 100, "SynthBravo"),
                player_obj(4, 100, "SynthCharlie"),
            ],
            vec![],
        ),
    );
    apply(&mut e, 4200, damage(3, 2, 300));
    apply(&mut e, 4300, damage(4, 3, 100));
    apply(&mut e, 4400, damage(999, 4, 50));

    let live = e.snapshot().fights.remove(0);
    assert_eq!(live.buckets.len(), 1);
    assert_eq!(live.buckets[0].total, 550.);
    assert_eq!(live.buckets[0].local, Some(100.));
    assert_eq!(live.buckets[0].others_damage, Some(400.));
    assert_eq!(live.buckets[0].others_presence, Some(2.));

    kill(&mut e);
    let saved = ended(apply(&mut e, 7000, map("Nexus")));
    assert_eq!(saved.buckets, live.buckets);
    let restored: FightView =
        serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    assert_eq!(restored.buckets, live.buckets);
}

#[test]
fn others_count_players_active_in_each_bucket() {
    let mut e = started();
    apply(
        &mut e,
        4100,
        update(
            vec![
                player_obj(3, 100, "SynthBravo"),
                player_obj(4, 100, "SynthCharlie"),
            ],
            vec![],
        ),
    );
    apply(&mut e, 4200, damage(3, 2, 200));
    apply(&mut e, 4300, damage(3, 3, 100));
    apply(&mut e, 1_000_000, tick(vec![]));
    apply(&mut e, 1_001_000, damage(4, 4, 100));
    let fight = e.snapshot().fights.remove(0);
    assert_eq!(
        fight
            .buckets
            .iter()
            .map(|b| (b.others_damage, b.others_presence))
            .collect::<Vec<_>>(),
        vec![(Some(300.), Some(1.)), (Some(100.), Some(1.))],
    );

    apply(&mut e, 2_000_000, tick(vec![]));
    apply(&mut e, 2_001_000, damage(1, 5, 50));
    let fight = e.snapshot().fights.remove(0);
    assert_eq!(fight.buckets[2].others_damage, Some(0.));
    assert_eq!(fight.buckets[2].others_presence, Some(0.));

    apply(&mut e, 3_000_000, tick(vec![]));
    apply(&mut e, 3_001_000, damage(999, 6, 50));
    let fight = e.snapshot().fights.remove(0);
    assert_eq!(fight.buckets[3].others_damage, Some(0.));
    assert_eq!(fight.buckets[3].others_presence, Some(0.));
}

fn alternating_party(seconds: u64) -> maxed_engine::Engine {
    let mut e = started();
    apply(
        &mut e,
        4100,
        update(
            vec![
                player_obj(3, 100, "SynthBravo"),
                player_obj(4, 100, "SynthCharlie"),
            ],
            vec![],
        ),
    );
    for second in 0..seconds {
        let at = second * 1_000_000 + 5000;
        apply(&mut e, at, tick(vec![]));
        let (owner, amount) = if second % 2 == 0 { (3, 300) } else { (4, 100) };
        apply(
            &mut e,
            at + 1000,
            damage(owner, (second + 2) as u16, amount),
        );
    }
    e
}

fn assert_alternating_party_buckets(fight: &FightView) {
    for bucket in &fight.buckets {
        let seconds = bucket.from.0 / 1_000_000..bucket.until.0 / 1_000_000;
        let damage = seconds
            .map(|second| if second % 2 == 0 { 300. } else { 100. })
            .sum::<f64>();
        assert_eq!(bucket.others_damage, Some(damage));
        assert_eq!(
            bucket.others_presence,
            Some((bucket.until.0 - bucket.from.0) as f64 / 1_000_000.),
        );
    }
    assert_eq!(
        fight
            .buckets
            .iter()
            .map(|b| b.others_damage.unwrap())
            .sum::<f64>(),
        fight
            .players
            .iter()
            .filter(|p| !p.is_local)
            .map(|p| p.damage)
            .sum::<f64>(),
    );
}

#[test]
fn others_follow_bucket_compaction() {
    let mut e = alternating_party(2000);
    let fight = e.snapshot().fights.remove(0);
    assert_eq!(fight.buckets.len(), 1100);
    assert_alternating_party_buckets(&fight);
    let saved = ended(apply(&mut e, 2_001_000_000, map("Nexus")));
    assert_eq!(saved.buckets, fight.buckets);
}

#[test]
fn live_compaction_sums_others() {
    // Cover presentation compaction both before and after domain compaction.
    for seconds in [360, 2000] {
        let e = alternating_party(seconds);
        let live = e.live_snapshot().fights.remove(0);
        assert!(live.buckets.len() <= maxed_engine::LIVE_CHART_BUCKETS);
        assert!(live
            .buckets
            .iter()
            .any(|b| b.until.0 - b.from.0 > 1_000_000));
        assert_alternating_party_buckets(&live);
    }
}

#[test]
fn live_compaction_propagates_unknown_others() {
    let e = alternating_party(360);
    let mut fight = e.snapshot().fights.remove(0);
    // Each field independently becomes unknown if any merged input is unknown.
    fight.buckets[0].others_damage = None;
    fight.buckets[1].others_presence = None;
    fight.player_series = None;
    maxed_engine::compact_live_fight(&mut fight);
    assert_eq!(fight.buckets[0].others_damage, None);
    assert_eq!(fight.buckets[0].others_presence, None);
    assert!(fight.buckets[1..]
        .iter()
        .all(|b| b.others_damage.is_some() && b.others_presence.is_some()));
}

#[test]
fn others_absent_when_series_discarded() {
    let mut e = started();
    assert_eq!(e.snapshot().fights[0].buckets[0].others_damage, Some(0.));
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
    let live = e.snapshot().fights.remove(0);
    assert!(live.player_series.is_none());
    assert_eq!(live.total_damage, 1301.);
    assert!(!live.buckets.is_empty());
    assert!(live
        .buckets
        .iter()
        .all(|b| b.others_damage.is_none() && b.others_presence.is_none()));

    // Later compaction shrinks the series dimensions but must not restore it.
    for i in 1001..1802 {
        apply(&mut e, i * 1_000_000, tick(vec![]));
        apply(&mut e, i * 1_000_000, damage(3, (i + 1000) as u16, 1));
    }
    let live = e.live_snapshot().fights.remove(0);
    assert!(live.player_series.is_none());
    assert!(live
        .buckets
        .iter()
        .all(|b| b.others_damage.is_none() && b.others_presence.is_none()));

    let saved = ended(apply(&mut e, 1_802_000_000, map("Nexus")));
    assert!(!saved.buckets.is_empty());
    assert!(saved
        .buckets
        .iter()
        .all(|b| b.others_damage.is_none() && b.others_presence.is_none()));
}

#[test]
fn others_absent_without_unique_local() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(
        &mut e,
        2000,
        update(vec![player_obj(3, 100, "SynthBravo"), target()], vec![]),
    );
    apply(&mut e, 3000, tick(vec![]));
    apply(&mut e, 4000, damage(3, 1, 100));
    let live = e.snapshot().fights.remove(0);
    assert!(!live.players.iter().any(|p| p.is_local));
    assert_eq!(live.buckets.len(), 1);
    assert_eq!(live.buckets[0].others_damage, None);
    assert_eq!(live.buckets[0].others_presence, None);
    kill(&mut e);
    assert_eq!(
        ended(apply(&mut e, 7000, map("Nexus"))).buckets,
        live.buckets
    );
}

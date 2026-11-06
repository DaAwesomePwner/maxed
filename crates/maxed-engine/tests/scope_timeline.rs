use crate::common::*;
use maxed_engine::Engine;
use maxed_protocol::Packet;
use maxed_types::*;

fn timelines(events: Vec<EngineEvent>) -> Vec<ScopeTimelineUpdate> {
    events
        .into_iter()
        .filter_map(|event| match event {
            EngineEvent::ScopeTimelineUpdated(update) => Some(update),
            _ => None,
        })
        .collect()
}

fn hit_target(
    e: &mut Engine,
    at: u64,
    owner: i32,
    target: i32,
    bullet: u16,
    amount: u16,
) -> Vec<ScopeTimelineUpdate> {
    let mut events = apply(e, at, tick(vec![]));
    let Packet::Damage(mut packet) = damage(owner, bullet, amount) else {
        unreachable!()
    };
    packet.targetId = target;
    events.extend(apply(e, at, Packet::Damage(packet)));
    timelines(events)
}

fn two_players(e: &mut Engine) {
    setup(e);
    apply(
        e,
        3000,
        update(
            vec![
                object(3, 100, vec![string(31, "Synthetic Remote")]),
                object(4, 300, vec![stat(0, 10000)]),
            ],
            vec![],
        ),
    );
}

fn check_totals(update: &ScopeTimelineUpdate) {
    for (i, bucket) in update.buckets.iter().enumerate() {
        assert_eq!(
            bucket.party_total,
            update.players.iter().map(|p| p.points[i]).sum::<f64>()
        );
        assert_eq!(
            bucket.party_active as usize,
            update.players.iter().filter(|p| p.points[i] > 0.).count()
        );
    }
}

#[test]
fn timeline_accumulates_across_fights() {
    let mut e = engine();
    two_players(&mut e);
    hit_target(&mut e, 1_000_000, 1, 2, 1, 100);
    hit_target(&mut e, 1_100_000, 3, 4, 1, 200);
    apply(&mut e, 1_200_000, update(vec![], vec![2, 4]));
    assert!(timelines(e.advance_to(Timestamp(4_999_999))).is_empty());
    let updates = timelines(e.advance_to(Timestamp(5_000_000)));
    assert_eq!(updates.len(), 1);
    let u = &updates[0];
    assert_eq!(u.scope_id, "test:run:1");
    assert_eq!(u.run_id.as_deref(), Some("test:run:1"));
    assert_eq!(u.players.len(), 2);
    assert_eq!(u.buckets.len(), 1);
    assert_eq!(u.buckets[0].from, Timestamp(1_000_000));
    assert_eq!(u.buckets[0].party_total, 300.);
    assert_eq!(u.buckets[0].party_active, 2);
    check_totals(u);
    assert!(timelines(e.advance_to(Timestamp(10_000_000))).is_empty());
}

#[test]
fn unchanged_rows_are_omitted_until_compaction() {
    let mut e = engine();
    two_players(&mut e);
    hit_target(&mut e, 1_000_000, 1, 2, 1, 100);
    hit_target(&mut e, 1_000_000, 3, 4, 1, 200);
    let first = timelines(e.advance_to(Timestamp(5_000_000))).remove(0);
    hit_target(&mut e, 6_000_000, 3, 4, 2, 50);
    let second = timelines(e.advance_to(Timestamp(10_000_000))).remove(0);
    assert!(!second.full);
    assert!(!second.compacted);
    assert_eq!(second.players.len(), 1);
    assert_eq!(second.players[0].object_id, 3);
    assert_eq!(second.players[0].points, [200., 50.]);
    assert_eq!(second.buckets[0].party_total, first.buckets[0].party_total);
    let mut updates = vec![];
    for i in 11..1811 {
        updates.extend(hit_target(&mut e, i * 1_000_000, 3, 4, i as u16, 1));
    }
    updates.extend(timelines(e.advance_to(Timestamp(1_815_000_000))));
    let compacted = updates
        .iter()
        .find(|u| u.compacted)
        .expect("compaction publish");
    assert!(compacted.full);
    assert_eq!(compacted.players.len(), 2);
    check_totals(compacted);
    hit_target(&mut e, 1_816_000_000, 3, 4, 2000, 1);
    let next = timelines(e.advance_to(Timestamp(1_820_000_000))).remove(0);
    assert!(next.compacted);
    assert!(!next.full);
    assert_eq!(next.players.len(), 1);
}

#[test]
fn late_contribution_forces_full_update() {
    let mut e = engine();
    two_players(&mut e);
    apply(&mut e, 1_000_000, tick(vec![]));
    apply(&mut e, 1_000_000, shoot(1));
    apply(&mut e, 1_100_000, hit(1));
    // Snapshots project pending estimates, but must not feed the scope timeline.
    assert_eq!(e.snapshot().fights[0].total_damage, 100.);
    hit_target(&mut e, 3_000_000, 3, 4, 1, 200);
    let first = timelines(e.advance_to(Timestamp(5_000_000))).remove(0);
    assert_eq!(first.players.len(), 1);
    assert_eq!(first.buckets[0].from, Timestamp(3_000_000));
    assert!(timelines(e.advance_to(Timestamp(6_200_000))).is_empty());
    let next = timelines(e.advance_to(Timestamp(10_000_000))).remove(0);
    assert!(next.full);
    assert!(!next.compacted);
    assert_eq!(next.players.len(), 2);
    assert_eq!(next.buckets[0].from, Timestamp(1_000_000));
    check_totals(&next);
}

#[test]
fn overflow_publishes_disabled_once() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 2500, map("Realm"));
    apply(
        &mut e,
        3000,
        update(
            (3..204)
                .map(|id| object(id, 100, vec![string(31, "Synthetic")]))
                .chain([player(), target()])
                .collect(),
            vec![],
        ),
    );
    for id in 3..203 {
        hit_target(&mut e, 1_000_000, id, 2, 1, 1);
    }
    let mut updates = vec![];
    for i in 2..1010 {
        updates.extend(hit_target(&mut e, i * 1_000_000, 1, 2, i as u16, 1));
    }
    updates.extend(timelines(e.advance_to(Timestamp(1_015_000_000))));
    // Existing damaging players remain eligible after Series clears its players.
    apply(
        &mut e,
        1_015_100_000,
        tick(vec![status(3, vec![stat(8, 201)])]),
    );
    // A new player's first contribution must also qualify after the disabled publish.
    hit_target(&mut e, 1_015_100_000, 203, 2, 3000, 1);
    let profiles: Vec<_> = e
        .advance_to(Timestamp(1_015_500_000))
        .into_iter()
        .filter_map(|event| {
            if let EngineEvent::PlayerObserved(profile) = event {
                Some(profile)
            } else {
                None
            }
        })
        .collect();
    assert!(profiles
        .iter()
        .any(|p| p.object_id == 3 && p.equipment[0].item_type == Some(201)));
    assert!(profiles.iter().any(|p| p.object_id == 203));
    updates.extend(timelines(
        e.finish(Timestamp(1_016_000_000), FightEnd::CaptureStopped),
    ));
    let disabled: Vec<_> = updates.iter().filter(|u| u.disabled).collect();
    assert_eq!(disabled.len(), 1);
    assert!(disabled[0].players.is_empty());
    assert!(disabled[0].buckets.is_empty());
    let index = updates.iter().position(|u| u.disabled).unwrap();
    assert_eq!(index + 1, updates.len());
}

#[test]
fn scope_end_flushes_timeline() {
    let mut e = engine();
    setup(&mut e);
    hit_target(&mut e, 1_000_000, 1, 2, 1, 100);
    timelines(e.advance_to(Timestamp(5_000_000))).remove(0);
    hit_target(&mut e, 6_000_000, 1, 2, 2, 50);
    let final_update = timelines(apply(&mut e, 6_100_000, map("Realm"))).remove(0);
    assert_eq!(final_update.scope_id, "test:run:1");
    assert_eq!(final_update.players[0].points, [100., 50.]);
    assert!(timelines(e.advance_to(Timestamp(12_000_000))).is_empty());
    apply(
        &mut e,
        12_100_000,
        update(vec![object(3, 100, vec![]), target()], vec![]),
    );
    hit_target(&mut e, 12_200_000, 3, 2, 1, 20);
    let new = timelines(e.finish(Timestamp(12_300_000), FightEnd::CaptureStopped)).remove(0);
    assert_eq!(new.scope_id, "test:map:2");
    assert_eq!(new.run_id, None);
    assert_eq!(new.players[0].points, [20.]);
}

#[test]
fn timeline_follows_local_identity_merge() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let mut p = player();
    p.status.stats.retain(|s| s.statTypeNum != 38);
    apply(&mut e, 2000, update(vec![p, target()], vec![]));
    hit_target(&mut e, 1_000_000, 1, 2, 1, 100);
    let old = timelines(e.advance_to(Timestamp(5_000_000))).remove(0);
    // This accepted contribution is still queued when identity is merged.
    hit_target(&mut e, 5_100_000, 1, 2, 2, 50);
    let events = apply(
        &mut e,
        5_100_000,
        tick(vec![status(1, vec![string(38, "synthetic-account")])]),
    );
    let verified = events
        .into_iter()
        .find_map(|event| match event {
            EngineEvent::CharacterMerged { verified_id, .. } => Some(verified_id),
            _ => None,
        })
        .unwrap();
    let next = timelines(e.advance_to(Timestamp(10_000_000))).remove(0);
    assert!(next.full);
    assert_eq!(next.players.len(), 1);
    assert_ne!(old.players[0].player_id, next.players[0].player_id);
    assert_eq!(next.players[0].player_id, verified);
    assert!(next.players[0].is_local);
    assert_eq!(next.players[0].points, [100., 50.]);
    check_totals(&next);
}

#[test]
fn retirement_settles_and_drains_pending_damage() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 1_000_000, tick(vec![]));
    apply(&mut e, 1_000_000, shoot(1));
    apply(&mut e, 1_100_000, hit(1));
    let update = timelines(apply(&mut e, 1_200_000, map("Nexus"))).remove(0);
    assert_eq!(update.players[0].points, [100.]);
    assert_eq!(update.buckets[0].from, Timestamp(1_000_000));
    check_totals(&update);
}

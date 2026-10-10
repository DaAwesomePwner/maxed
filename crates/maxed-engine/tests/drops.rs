mod common;
use common::*;
use maxed_types::*;
#[test]
fn immediate_bag_late_contents_and_removed_ids() {
    let mut e = engine();
    setup(&mut e);
    let events = apply(
        &mut e,
        3000,
        update(vec![object(4, 1292, vec![])], vec![99]),
    );
    assert!(events
        .iter()
        .any(|e| matches!(e,EngineEvent::BagDropped(d) if !d.contents_known)));
    let id = e.snapshot().drops[0].id.clone();
    let mut stats: Vec<_> = (8..16).map(|s| stat(s, -1)).collect();
    stats[0] = stat(8, 200);
    stats[1] = stat(9, 200);
    stats.push(string(80, ",,,,,,,"));
    apply(&mut e, 4000, tick(vec![status(4, stats)]));
    let d = &e.snapshot().drops[0];
    assert!(d.contents_known);
    assert_eq!(d.items, vec![200, 200]);
    assert_eq!(d.contents.len(), 2);
    assert_eq!(d.id, id);
    assert!(
        apply(&mut e, 5000, update(vec![object(4, 1292, vec![])], vec![]))
            .iter()
            .all(|v| !matches!(v, EngineEvent::BagDropped(_)))
    );
    assert!(apply(&mut e, 6000, map("Nexus"))
        .iter()
        .any(|v| matches!(v, EngineEvent::DropUpdated(_))));
}
#[test]
fn early_map_transition_flushes_unknown_bag() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(4, 1296, vec![])], vec![]));
    let events = apply(&mut e, 4000, map("Nexus"));
    assert!(events.iter().any(
        |v| matches!(v,EngineEvent::DropUpdated(d) if !d.contents_known && d.source_enemy.is_none())
    ));
}
#[test]
fn per_slot_partial_enchants_preserve_observed_entries() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![object(
                4,
                1292,
                vec![stat(8, 200), stat(9, 201), string(80, "AAIE")],
            )],
            vec![],
        ),
    );
    let d = &e.snapshot().drops[0];
    assert_eq!(d.contents[0].enchant.state, EnchantState::RecordedEmpty);
    assert_eq!(d.contents[1].enchant.state, EnchantState::Missing);
}
#[test]
fn invalid_inventory_value_is_not_known_empty() {
    let mut e = engine();
    setup(&mut e);
    let mut stats: Vec<_> = (8..16).map(|s| stat(s, -1)).collect();
    stats[0] = stat(8, -2);
    apply(&mut e, 3000, update(vec![object(4, 1292, stats)], vec![]));
    assert!(!e.snapshot().drops[0].contents_known);
}
#[test]
fn evicted_bag_is_not_reannounced_on_another_stat_delta() {
    use maxed_engine::Engine;
    use std::sync::Arc;
    let mut c = config();
    c.max_active_bags = 1;
    let mut e = Engine::new(c, Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(4, 1292, vec![])], vec![]));
    let events = apply(&mut e, 4000, update(vec![object(5, 1292, vec![])], vec![]));
    assert!(events.iter().any(|event| matches!(event,
        EngineEvent::DropUpdated(b) if b.issues.contains(&CoverageIssue::ResourceLimit))));
    assert!(e.snapshot().issues.contains(&CoverageIssue::ResourceLimit));
    assert_eq!(e.snapshot().drops.len(), 1);
    assert!(
        apply(&mut e, 5000, tick(vec![status(4, vec![stat(8, 200)])]))
            .iter()
            .all(|v| !matches!(v, EngineEvent::BagDropped(_)))
    );
}
fn text(npc: &str, line: &str) -> maxed_protocol::Packet {
    use maxed_protocol::{defs::Text, Packet};
    Packet::Text(Text {
        name: npc.into(),
        objectId: 99,
        numStars: -1,
        bubbleTime: 0,
        recipient: String::new(),
        text: line.into(),
        cleanText: String::new(),
        isSupporter: false,
        starBackground: 0,
    })
}
#[test]
fn dialogue_attribution_counts_only_ticks_that_process_bags() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        text(
            "#Void Entity",
            "You fools... You can never truly defeat me! I am in all of you! I AM all of you!",
        ),
    );
    // No bag yet: even many empty NEWTICKs must not consume the window.
    for at in 4..=8 {
        apply(&mut e, at * 1000, tick(vec![]));
    }
    for (id, start, expected) in [
        (4, 10_000, Some(45076)),
        (5, 20_000, Some(45076)),
        (6, 30_000, None),
    ] {
        apply(
            &mut e,
            start,
            update(vec![object(id, 1292, vec![])], vec![]),
        );
        apply(&mut e, start + 1000, tick(vec![]));
        apply(&mut e, start + 2000, tick(vec![]));
        let snapshot = e.snapshot();
        let d = snapshot.drops.last().unwrap();
        assert_eq!(d.source_enemy, expected);
        assert_eq!(d.source_estimated, expected.is_some());
    }
}
#[test]
fn bag_spawned_one_tick_after_dialogue_keeps_attribution() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        text(
            "#Void Entity",
            "You fools... You can never truly defeat me! I am in all of you! I AM all of you!",
        ),
    );
    apply(&mut e, 4000, tick(vec![]));
    apply(&mut e, 5000, update(vec![object(4, 1292, vec![])], vec![]));
    apply(&mut e, 6000, tick(vec![]));
    apply(&mut e, 7000, tick(vec![]));
    assert_eq!(e.snapshot().drops[0].source_enemy, Some(45076));
}
#[test]
fn tied_spatial_candidates_remain_unknown_and_do_not_clear_a_run() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![object(3, 301, vec![]), object(4, 1292, vec![])],
            vec![],
        ),
    );
    apply(&mut e, 4000, damage(1, 1, 10));
    let maxed_protocol::Packet::Damage(mut d) = damage(1, 2, 10) else {
        unreachable!()
    };
    d.targetId = 3;
    apply(&mut e, 5000, maxed_protocol::Packet::Damage(d));
    apply(&mut e, 6000, tick(vec![]));
    apply(&mut e, 7000, update(vec![], vec![2, 3]));
    apply(&mut e, 8000, tick(vec![]));
    let d = &e.snapshot().drops[0];
    assert!(d.source_enemy.is_none());
    assert_eq!(
        d.source_reason.as_deref(),
        Some("ambiguous-nearby-removed-targets")
    );
    assert!(e.snapshot().run.unwrap().completed_at.is_none());
}

#[test]
fn same_update_boss_removal_expires_before_bag_attribution_like_java() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, update(vec![object(4, 1292, vec![])], vec![2]));
    let id = e.snapshot().drops[0].id.clone();
    // Java queues in lootTickContainer[lootTickToggle], then adds the hit
    // target to killedEntitys. Tick one selects the other queue and clears
    // killedEntitys; tick two selects this bag with no spatial candidate.
    for at in [5000, 6000, 7000] {
        let events = apply(&mut e, at, tick(vec![]));
        assert!(!events
            .iter()
            .any(|event| matches!(event, EngineEvent::DropUpdated(b)
            if b.source_enemy.is_some() || b.source_estimated)));
        let snapshot = e.snapshot();
        let bag = &snapshot.drops[0];
        assert_eq!(bag.id, id);
        assert_eq!(bag.source_enemy, None);
        assert!(!bag.source_estimated);
        assert_eq!(bag.source_reason.as_deref(), Some("source-not-observed"));
    }
}

#[test]
fn boss_removed_between_bag_ticks_is_attributed_only_as_estimated() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, update(vec![object(4, 1292, vec![])], vec![]));
    apply(&mut e, 5000, tick(vec![]));
    assert!(e.snapshot().drops[0].source_enemy.is_none());
    apply(&mut e, 6000, update(vec![], vec![2]));
    let events = apply(&mut e, 7000, tick(vec![]));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::DropUpdated(b)
        if b.source_enemy == Some(300) && b.source_estimated
        && b.source_reason.as_deref() == Some("nearest-removed-hit-target"))));
    let snapshot = e.snapshot();
    assert_eq!(snapshot.drops[0].source_enemy, Some(300));
    assert!(snapshot.drops[0].source_estimated);
}

#[test]
fn loot_retention_preserves_taken_and_reappearing_items() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![object(4, 1292, vec![stat(8, 200), stat(9, 200)])],
            vec![],
        ),
    );
    apply(&mut e, 4000, tick(vec![status(4, vec![stat(8, -2)])]));
    assert!(e.snapshot().drops[0]
        .contents
        .iter()
        .all(|i| i.taken_observed_at.is_none()));
    apply(&mut e, 5000, tick(vec![status(4, vec![stat(8, -1)])]));
    apply(&mut e, 6000, tick(vec![status(4, vec![stat(8, 201)])]));
    apply(&mut e, 7000, tick(vec![status(4, vec![stat(8, 200)])]));
    let bag = e.snapshot().drops.remove(0);
    assert_eq!(bag.contents.len(), 3);
    assert_eq!(bag.contents[0].taken_observed_at, Some(Timestamp(5000)));
    assert_eq!(bag.contents[1].taken_observed_at, None);
    assert_eq!(bag.contents[2].taken_observed_at, Some(Timestamp(7000)));
    for item in 202u16..212 {
        apply(
            &mut e,
            8000 + u64::from(item),
            tick(vec![status(4, vec![stat(8, item.into())])]),
        );
    }
    let bag = e.snapshot().drops.remove(0);
    assert_eq!(bag.contents.len(), 13);
    assert_eq!(
        bag.contents.iter().filter(|i| i.item_type == 200).count(),
        2
    );
    assert_eq!(bag.contents[0].taken_observed_at, Some(Timestamp(5000)));
}

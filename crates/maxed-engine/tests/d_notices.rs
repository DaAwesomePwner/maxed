mod common;
mod d_support;
use d_support::*;
use maxed_types::*;
#[test]
fn party_death_links_player_by_name_and_increments_row_deaths() {
    let mut e = remote();
    let v = instances(apply(
        &mut e,
        5000,
        player_death("SynthBravo", 20, "Synthetic Boss", 9),
    ))
    .remove(0);
    assert!(v.provisional);
    assert!(matches!(
        v.kind,
        InstanceEventKind::PlayerDeath {
            object_id: Some(3),
            is_local: false,
            ..
        }
    ));
    assert_eq!(
        e.snapshot().fights[0]
            .players
            .iter()
            .find(|p| p.object_id == 3)
            .unwrap()
            .deaths,
        1
    );
}
#[test]
fn local_name_death_notice_sets_is_local_and_does_not_double_count() {
    let mut e = started();
    apply(
        &mut e,
        4500,
        tick(vec![status(1, vec![string(31, "SynthAlpha")])]),
    );
    let v = instances(apply(
        &mut e,
        5000,
        player_death("SynthAlpha", 20, "Synthetic Boss", 9),
    ))
    .remove(0);
    assert!(matches!(
        v.kind,
        InstanceEventKind::PlayerDeath { is_local: true, .. }
    ));
    apply(&mut e, 6000, death());
    assert_eq!(e.snapshot().fights[0].players[0].deaths, 1);
}
#[test]
fn local_name_death_notice_does_not_emit_player_died() {
    let mut e = started();
    apply(
        &mut e,
        4500,
        tick(vec![status(1, vec![string(31, "SynthAlpha")])]),
    );
    let events = apply(
        &mut e,
        5000,
        player_death("SynthAlpha", 20, "Synthetic Boss", 9),
    );
    assert_eq!(instances(events.clone()).len(), 1);
    assert!(!events
        .iter()
        .any(|e| matches!(e, EngineEvent::PlayerDied { .. })));
    assert!(e.snapshot().character.unwrap().died_at.is_none());
}
#[test]
fn removal_after_death_notice_is_not_left() {
    let mut e = remote();
    assert_eq!(
        instances(apply(
            &mut e,
            5000,
            player_death("SynthBravo", 20, "Synthetic Boss", 9)
        ))
        .len(),
        1
    );
    apply(&mut e, 6000, update(vec![], vec![3]));
    assert!(instances(e.advance_to(Timestamp(3_000_000))).is_empty());
    // Cancellation must still suppress the departure when the run ends.
    assert!(instances(apply(&mut e, 4_000_000, map("Nexus"))).is_empty());
}
#[test]
fn removal_then_late_notice_within_2s_cancels_left() {
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![3]));
    let v = instances(apply(
        &mut e,
        1_000_000,
        player_death("SynthBravo", 20, "Synthetic Boss", 9),
    ));
    assert_eq!(v.len(), 1);
    assert!(instances(e.advance_to(Timestamp(3_000_000))).is_empty());
    assert_eq!(
        e.snapshot().fights[0]
            .players
            .iter()
            .find(|p| p.object_id == 3)
            .unwrap()
            .deaths,
        1
    );
    // Cancellation must still suppress the departure when the run ends.
    assert!(instances(apply(&mut e, 4_000_000, map("Nexus"))).is_empty());
}
#[test]
fn portal_opened_resolves_dungeon_from_picture_type() {
    let mut c = catalog();
    c.objects[2].dungeon_name = Some("Synthetic Sanctum".into());
    let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
    setup(&mut e);
    let v = instances(apply(&mut e, 3000, portal_opened("SynthBravo", 301))).remove(0);
    assert!(!v.provisional);
    assert!(
        matches!(v.kind,InstanceEventKind::PortalOpened{dungeon:Some(ref d),..} if d=="Synthetic Sanctum")
    );
}
#[test]
fn oryx_kill_out_of_view_is_event_only() {
    let mut e = engine();
    setup(&mut e);
    let v = instances(apply(
        &mut e,
        3000,
        oryx_killed("Unknown_Boss", "SynthBravo"),
    ))
    .remove(0);
    assert!(v.provisional);
    assert!(matches!(
        v.kind,
        InstanceEventKind::RealmEventKilled {
            object_type: None,
            ..
        }
    ));
    assert!(e.snapshot().fights.is_empty());
}
#[test]
fn instance_event_cap_counts_drops() {
    let mut e = engine();
    setup(&mut e);
    let before = e.snapshot().issues;
    let mut n = 0;
    for _ in 0..4098 {
        n += instances(apply(&mut e, 3000, portal_opened("SynthBravo", 301))).len();
    }
    assert_eq!(n, 4096);
    assert_eq!(e.snapshot().instance.unwrap().events_dropped, 2);
    assert_eq!(e.snapshot().issues, before);
}
#[test]
fn instance_event_ids_are_deterministic() {
    fn run() -> Vec<InstanceEventView> {
        let mut e = engine();
        setup(&mut e);
        let mut v = instances(apply(&mut e, 3000, portal_opened("SynthBravo", 301)));
        v.extend(instances(apply(
            &mut e,
            3000,
            portal_opened("SynthBravo", 301),
        )));
        assert_eq!(v.len(), 2);
        assert_ne!(v[0].id, v[1].id);
        v
    }
    assert_eq!(run(), run());
}
#[test]
fn departure_waits_for_run_end_and_map_change_finalizes_it() {
    let mut e = remote();
    assert!(instances(apply(&mut e, 5000, update(vec![], vec![3]))).is_empty());
    assert!(instances(e.advance_to(Timestamp(2_004_999))).is_empty());
    assert!(instances(e.advance_to(Timestamp(2_005_000))).is_empty());
    // Departures now finalize only at run end, preserving the removal timestamp.
    let v = instances(apply(&mut e, 3_000_000, map("Nexus")));
    assert_eq!(v.len(), 1);
    assert!(matches!(
        v[0].kind,
        InstanceEventKind::PlayerLeft { object_id: 3, .. }
    ));
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![3]));
    assert_eq!(instances(apply(&mut e, 6000, map("Nexus"))).len(), 1);
    assert!(instances(e.advance_to(Timestamp(3_000_000))).is_empty());
}

#[test]
fn duplicate_name_does_not_guess_identity_or_cancel_departures() {
    let mut e = remote();
    apply(
        &mut e,
        4500,
        update(vec![player_obj(4, 100, "SynthBravo")], vec![]),
    );
    let v = instances(apply(
        &mut e,
        5000,
        player_death("SynthBravo", 20, "Synthetic Boss", 9),
    ))
    .remove(0);
    assert!(matches!(
        v.kind,
        InstanceEventKind::PlayerDeath {
            object_id: None,
            is_local: false,
            ..
        }
    ));
    assert_eq!(
        e.snapshot().fights[0]
            .players
            .iter()
            .find(|p| p.object_id == 3)
            .unwrap()
            .deaths,
        0
    );
}
#[test]
fn reappearing_player_cancels_pending_departure() {
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![3]));
    apply(
        &mut e,
        6000,
        update(vec![player_obj(3, 100, "SynthBravo")], vec![]),
    );
    assert!(instances(e.advance_to(Timestamp(3_000_000))).is_empty());
    // Re-entry now has the whole run to cancel a pending departure.
    assert!(instances(apply(&mut e, 4_000_000, map("Nexus"))).is_empty());
}
#[test]
fn mixed_notice_and_departure_events_share_the_cap() {
    let mut e = remote();
    for _ in 0..4095 {
        apply(&mut e, 4500, portal_opened("SynthBravo", 301));
    }
    apply(&mut e, 5000, update(vec![], vec![3]));
    // Local death ends the run and finalizes pending remote departures.
    assert_eq!(instances(apply(&mut e, 3_000_000, death())).len(), 1);
    assert!(instances(apply(&mut e, 3_000_001, portal_opened("SynthBravo", 301))).is_empty());
    assert_eq!(e.snapshot().instance.unwrap().events_dropped, 1);
}

#[test]
fn key_pop_candidate_uses_local_identity_and_catalog_dungeon_only() {
    let mut c = catalog();
    c.objects[2].dungeon_name = Some("The Void".into());
    let mut e = maxed_engine::Engine::new(
        maxed_engine::EngineConfig {
            stream_id: "live-key-pop".into(),
            ..config()
        },
        std::sync::Arc::new(c.clone()),
    )
    .unwrap();
    setup(&mut e);
    apply(
        &mut e,
        2500,
        tick(vec![status(1, vec![string(31, "local-test-marker")])]),
    );
    for (at, opener, expected) in [
        (3000, "remote-test-marker", Some("The Void")),
        (4000, "LOCAL-TEST-MARKER", None),
    ] {
        let notice = instances(apply(&mut e, at, portal_opened(opener, 301))).remove(0);
        assert_eq!(
            e.key_pop_dungeon(&EngineEvent::InstanceEvent(notice))
                .as_deref(),
            expected
        );
        let mut provisional =
            instances(apply(&mut e, at + 1, portal_opened(opener, 301))).remove(0);
        provisional.provisional = true;
        assert!(matches!(provisional.kind, InstanceEventKind::PortalOpened {
            opener_is_local: Some(local), ..
        } if local == expected.is_none()));
        assert_eq!(
            e.key_pop_dungeon(&EngineEvent::InstanceEvent(provisional)),
            None
        );
    }
    let unknown = instances(apply(
        &mut e,
        5000,
        portal_opened("remote-test-marker", 65535),
    ))
    .remove(0);
    assert_eq!(
        e.key_pop_dungeon(&EngineEvent::InstanceEvent(unknown)),
        None
    );
    let mut unknown_identity = maxed_engine::Engine::new(
        maxed_engine::EngineConfig {
            stream_id: "live-unknown".into(),
            ..config()
        },
        std::sync::Arc::new(c),
    )
    .unwrap();
    apply(&mut unknown_identity, 0, map("Nexus"));
    apply(&mut unknown_identity, 1000, create(1, 1));
    apply(
        &mut unknown_identity,
        2000,
        update(vec![object(1, 100, vec![])], vec![]),
    );
    let notice = instances(apply(
        &mut unknown_identity,
        3000,
        portal_opened("remote-test-marker", 301),
    ))
    .remove(0);
    assert_eq!(
        unknown_identity.key_pop_dungeon(&EngineEvent::InstanceEvent(notice)),
        None
    );
}

#[test]
fn portal_opener_flag_tracks_known_local_identity() {
    for (local, opener, expected) in [
        (Some("Vexlin"), "vExLiN", Some(true)),
        (Some("Vexlin"), "Quenby", Some(false)),
        (None, "Quenby", None),
        (Some(""), "Quenby", None),
    ] {
        let mut e = engine();
        apply(&mut e, 0, map("The Void"));
        apply(&mut e, 1000, create(1, 1));
        if let Some(name) = local {
            apply(&mut e, 2000, update(vec![player()], vec![]));
            apply(&mut e, 2500, tick(vec![status(1, vec![string(31, name)])]));
        }
        let row = instances(apply(&mut e, 3000, portal_opened(opener, 301))).remove(0);
        let InstanceEventKind::PortalOpened {
            opener_is_local, ..
        } = row.kind
        else {
            panic!("portal")
        };
        assert_eq!(opener_is_local, expected);
    }
}

use crate::common::*;
use maxed_engine::{parse_enchant, Engine};
use maxed_protocol::defs::ObjectData;
use maxed_types::*;

fn profiles(events: Vec<EngineEvent>) -> Vec<PlayerProfileView> {
    events
        .into_iter()
        .filter_map(|e| match e {
            EngineEvent::PlayerObserved(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn remote() -> ObjectData {
    let mut stats = vec![
        stat(7, 20),
        stat(8, 200),
        stat(9, 201),
        stat(10, -1),
        stat(11, 200),
        stat(25, 123),
        stat(30, 42),
        stat(57, 456),
        string(31, "Remote,x"),
        string(62, "Synthetic Guild"),
        string(80, "AAIEAQA=,,,"),
    ];
    for id in [0, 3, 20, 21, 22, 26, 27, 28] {
        stats.push(stat(id, 75));
    }
    for id in 46..=53 {
        stats.push(stat(id, 0));
    }
    object(3, 100, stats)
}

fn observed_remote(e: &mut Engine) -> PlayerProfileView {
    apply(e, 3000, update(vec![remote()], vec![]));
    let rows = profiles(e.advance_to(Timestamp(250_000)));
    let rows: Vec<_> = rows.into_iter().filter(|p| p.object_id == 3).collect();
    assert_eq!(rows.len(), 1);
    rows.into_iter().next().unwrap()
}

#[test]
fn remote_profile_has_observed_fields() {
    let mut e = engine();
    setup(&mut e);
    let p = observed_remote(&mut e);
    assert_eq!(p.scope_id, "test:run:1");
    assert_eq!(p.run_id.as_deref(), Some("test:run:1"));
    assert_eq!(p.player_id, "test:player:1:3:1");
    assert!(!p.is_local);
    assert_eq!(p.name.as_deref(), Some("Remote"));
    assert_eq!(p.class_id, Some(100));
    assert_eq!(p.guild.as_deref(), Some("Synthetic Guild"));
    assert_eq!(
        (p.level, p.stars, p.fame, p.skin_id),
        (Some(20), Some(42), Some(456), Some(123))
    );
    assert_eq!(
        p.stats.iter().map(|s| s.stat_id).collect::<Vec<_>>(),
        [0, 3, 20, 21, 22, 26, 27, 28, 46, 47, 48, 49, 50, 51, 52, 53]
    );
    assert!(p
        .stats
        .iter()
        .all(|s| s.value == if s.stat_id >= 46 { 0 } else { 75 }));
    assert_eq!(p.equipment.len(), 4);
    assert_eq!(
        p.equipment.iter().map(|s| s.item_type).collect::<Vec<_>>(),
        [Some(200), Some(201), None, Some(200)]
    );
    assert_eq!(p.equipment[0].enchant, parse_enchant(Some("AAIEAQA=")));
    assert_eq!(p.maxed, None); // The synthetic catalog supplies only the ATT cap.
    assert_eq!(p.maxed_stats, [20]);
    assert_eq!(
        (p.first_seen, p.last_seen),
        (Timestamp(3000), Timestamp(3000))
    );
}

#[test]
fn map_profiles_wait_for_damage_and_publish_current_fields() {
    for name in ["Nexus", "Realm"] {
        let mut e = engine();
        apply(&mut e, 0, map(name));
        apply(&mut e, 3000, update(vec![remote(), target()], vec![]));
        assert!(profiles(e.advance_to(Timestamp(250_000))).is_empty());
        apply(
            &mut e,
            300_000,
            tick(vec![status(3, vec![stat(8, 201), stat(57, 789)])]),
        );
        assert!(profiles(e.advance_to(Timestamp(500_000))).is_empty());
        apply(&mut e, 510_000, damage(3, 1, 100));
        let rows = profiles(e.advance_to(Timestamp(750_000)));
        assert_eq!(rows.len(), 1);
        let p = &rows[0];
        assert_eq!(p.scope_id, "test:map:1");
        assert_eq!(p.run_id, None);
        assert_eq!(p.equipment[0].item_type, Some(201));
        assert_eq!(p.fame, Some(789));
        assert_eq!(p.guild.as_deref(), Some("Synthetic Guild"));
    }
}

#[test]
fn map_bystanders_are_not_published_at_scope_end() {
    let mut e = engine();
    apply(&mut e, 0, map("Nexus"));
    apply(&mut e, 3000, update(vec![remote()], vec![]));
    assert!(profiles(e.advance_to(Timestamp(250_000))).is_empty());
    assert!(profiles(apply(&mut e, 300_000, map("Realm"))).is_empty());
}

#[test]
fn nexus_roster_profiles_are_changed_only_and_never_published() {
    let mut e = engine();
    let mut events = apply(&mut e, 0, map("Nexus"));
    events.extend(apply(
        &mut e,
        1000,
        update(
            vec![object(
                3,
                100,
                vec![string(31, "SynthAlpha"), stat(7, 20), stat(8, 200)],
            )],
            vec![],
        ),
    ));
    events.extend(e.advance_to(Timestamp(250_000)));
    let rows = e.take_roster_profiles();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name.as_deref(), Some("SynthAlpha"));
    assert_eq!(rows[0].equipment[0].item_type, Some(200));
    assert_eq!(rows[0].run_id, None);
    assert!(e.take_roster_profiles().is_empty());

    events.extend(apply(&mut e, 300_000, tick(vec![status(3, vec![])])));
    events.extend(e.advance_to(Timestamp(500_000)));
    assert!(e.take_roster_profiles().is_empty());
    events.extend(apply(
        &mut e,
        600_000,
        tick(vec![status(3, vec![stat(8, 201)])]),
    ));
    events.extend(e.advance_to(Timestamp(750_000)));
    assert_eq!(
        e.take_roster_profiles()[0].equipment[0].item_type,
        Some(201)
    );
    events.extend(apply(&mut e, 800_000, map("Realm")));
    assert!(e.take_roster_profiles().is_empty());
    assert!(profiles(events).is_empty());
}

#[test]
fn maxed_uses_the_same_observed_values_and_boosts_as_character() {
    let mut catalog = catalog();
    catalog.classes[0].stat_caps = [0, 3, 20, 21, 22, 26, 27, 28]
        .into_iter()
        .map(|stat_id| StatView { stat_id, value: 75 })
        .collect();
    let mut e = Engine::new(config(), std::sync::Arc::new(catalog)).unwrap();
    setup(&mut e);
    let mut local = remote();
    local.status.objectId = 1;
    local.status.stats.push(stat(48, 1)); // ATT's unboosted value is below its cap.

    // Local cap claims in this fixture rely on explicitly observed zero exalt.
    local.status.stats.extend((105..=112).map(|id| stat(id, 0)));
    apply(&mut e, 3000, update(vec![remote(), local], vec![]));
    let rows = profiles(e.advance_to(Timestamp(250_000)));
    assert_eq!(
        rows.iter().find(|p| p.object_id == 3).unwrap().maxed,
        Some(8)
    );
    assert_eq!(
        rows.iter().find(|p| p.object_id == 1).unwrap().maxed,
        Some(7)
    );
    assert_eq!(e.snapshot().character.unwrap().maxed, Some(7));
    assert_eq!(
        rows.iter().find(|p| p.object_id == 3).unwrap().maxed_stats,
        [0, 3, 20, 21, 22, 28, 26, 27]
    );
    assert_eq!(
        rows.iter().find(|p| p.object_id == 1).unwrap().maxed_stats,
        [0, 3, 21, 22, 28, 26, 27]
    );
}

#[test]
fn local_profile_maxed_requires_shared_exalt_evidence_but_remote_does_not() {
    for level in [None, Some(0), Some(2)] {
        let mut catalog = catalog();
        catalog.classes[0].stat_caps = [0, 3, 20, 21, 22, 26, 27, 28]
            .into_iter()
            .map(|stat_id| StatView { stat_id, value: 75 })
            .collect();
        let mut e = Engine::new(config(), std::sync::Arc::new(catalog)).unwrap();
        setup(&mut e);
        let mut local = remote();
        local.status.objectId = 1;
        // Leave enough boost for nonzero exalt, including HP/MP's multiplier.
        for id in [0, 3, 20, 21, 22, 26, 27, 28] {
            local.status.stats.push(stat(id, 85));
        }
        local.status.stats.extend((46..=53).map(|id| stat(id, 10)));
        if let Some(level) = level {
            local
                .status
                .stats
                .extend((105..=112).map(|id| stat(id, level)));
        }
        apply(&mut e, 3000, update(vec![remote(), local], vec![]));
        let rows = profiles(e.advance_to(Timestamp(250_000)));
        let local = rows.iter().find(|p| p.is_local).unwrap();
        let remote = rows.iter().find(|p| !p.is_local).unwrap();
        let expected = (level == Some(0)).then_some(8);
        assert!(local.maxed_gated);
        assert!(!remote.maxed_gated);
        assert_eq!(local.maxed, expected, "level {level:?}");
        assert_eq!(local.maxed, e.snapshot().character.unwrap().maxed);
        assert_eq!(
            local.maxed_stats.len(),
            if expected.is_some() { 8 } else { 0 }
        );
        assert_eq!(remote.maxed, Some(8));
        assert_eq!(remote.maxed_stats, [0, 3, 20, 21, 22, 28, 26, 27]);
    }
}

#[test]
fn maxed_stats_require_observed_caps_values_and_boosts() {
    for unknown in ["caps", "value", "boost"] {
        let mut catalog = catalog();
        catalog.classes[0].stat_caps = if unknown == "caps" {
            vec![]
        } else {
            vec![StatView {
                stat_id: 20,
                value: 75,
            }]
        };
        let mut e = Engine::new(config(), std::sync::Arc::new(catalog)).unwrap();
        setup(&mut e);
        let mut row = remote();
        let missing = match unknown {
            "value" => 20,
            "boost" => 48,
            _ => 255,
        };
        row.status.stats.retain(|s| s.statTypeNum != missing);
        apply(&mut e, 3000, update(vec![row], vec![]));
        let p = profiles(e.advance_to(Timestamp(250_000)))
            .into_iter()
            .find(|p| p.object_id == 3)
            .unwrap();
        assert!(p.maxed_stats.is_empty(), "{unknown}");
    }
}

#[test]
fn buffered_local_spawn_is_profiled_in_the_new_scope() {
    let mut e = engine();
    apply(&mut e, 0, create(1, 1));
    apply(&mut e, 1000, update(vec![player()], vec![]));
    apply(&mut e, 2000, map("The Void"));
    let rows = profiles(e.advance_to(Timestamp(250_000)));
    assert_eq!(rows.len(), 1);
    assert!(rows[0].is_local);
    assert_eq!(rows[0].scope_id, "test:run:1");
    assert_eq!(rows[0].name.as_deref(), Some("Synthetic"));
}

#[test]
fn departed_profile_keeps_its_final_observation() {
    let mut e = engine();
    setup(&mut e);
    let old = observed_remote(&mut e);
    apply(&mut e, 300_000, update(vec![], vec![3]));
    assert!(profiles(e.advance_to(Timestamp(500_000))).is_empty());
    let final_rows = profiles(apply(&mut e, 510_000, map("Nexus")));
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0].player_id, old.player_id);
    assert_eq!(final_rows[0].last_seen, Timestamp(300_000));
}

#[test]
fn absent_stats_stay_absent() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(
        &mut e,
        1000,
        update(vec![object(3, 100, vec![string(31, "Remote")])], vec![]),
    );
    let p = profiles(e.advance_to(Timestamp(250_000))).remove(0);
    assert!(p.stats.is_empty());
    assert_eq!(
        (p.level, p.stars, p.fame, p.skin_id, p.guild, p.maxed),
        (None, None, None, None, None, None)
    );
    assert!(p
        .equipment
        .iter()
        .all(|s| s.item_type.is_none() && s.enchant.state == EnchantState::Missing));
}

#[test]
fn unchanged_profile_is_not_republished() {
    let mut e = engine();
    setup(&mut e);
    observed_remote(&mut e);
    assert!(profiles(e.advance_to(Timestamp(500_000))).is_empty());
    apply(
        &mut e,
        510_000,
        tick(vec![status(3, vec![stat(8, 200), string(31, "Remote,x")])]),
    );
    assert!(profiles(e.advance_to(Timestamp(750_000))).is_empty());
}

#[test]
fn last_seen_alone_does_not_republish_but_gear_change_does() {
    let mut e = engine();
    setup(&mut e);
    observed_remote(&mut e);
    apply(&mut e, 300_000, tick(vec![status(3, vec![])]));
    assert!(profiles(e.advance_to(Timestamp(500_000))).is_empty());
    assert!(profiles(apply(
        &mut e,
        510_000,
        tick(vec![status(3, vec![stat(8, 201)])])
    ))
    .is_empty());
    assert!(profiles(e.advance_to(Timestamp(749_999))).is_empty());
    let p = profiles(e.advance_to(Timestamp(750_000))).remove(0);
    assert_eq!(p.equipment[0].item_type, Some(201));
    assert_eq!(p.equipment[0].enchant.state, EnchantState::Missing);
    assert_eq!(p.last_seen, Timestamp(510_000));
    assert_eq!(p.first_seen, Timestamp(3000));
}

#[test]
fn guild_is_kept_only_for_players_and_capped() {
    let mut e = engine();
    setup(&mut e);
    let guild = "é".repeat(50);
    apply(
        &mut e,
        3000,
        update(
            vec![
                object(3, 100, vec![string(62, &guild)]),
                object(4, 301, vec![string(62, &guild)]),
            ],
            vec![],
        ),
    );
    let rows = profiles(e.advance_to(Timestamp(250_000)));
    assert!(rows.iter().all(|p| p.object_id != 4));
    let p = rows.iter().find(|p| p.object_id == 3).unwrap();
    assert_eq!(p.guild.as_deref(), Some("é".repeat(32).as_str()));
}

#[test]
fn empty_guild_stays_absent_and_clears_a_previous_guild() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(vec![object(3, 100, vec![string(62, "")])], vec![]),
    );
    let rows = profiles(e.advance_to(Timestamp(250_000)));
    assert_eq!(rows.iter().find(|p| p.object_id == 3).unwrap().guild, None);
    apply(
        &mut e,
        300_000,
        tick(vec![status(3, vec![string(62, "Synthetic Guild")])]),
    );
    let row = profiles(e.advance_to(Timestamp(500_000))).remove(0);
    assert_eq!(row.guild.as_deref(), Some("Synthetic Guild"));
    apply(&mut e, 510_000, tick(vec![status(3, vec![string(62, "")])]));
    assert_eq!(
        profiles(e.advance_to(Timestamp(750_000))).remove(0).guild,
        None
    );
}

#[test]
fn profile_cap_marks_resource_limit() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(
        &mut e,
        1000,
        update(
            (1..=513)
                .map(|id| object(id, 100, vec![string(31, "Synthetic")]))
                .collect(),
            vec![],
        ),
    );
    let events = e.advance_to(Timestamp(250_000));
    assert_eq!(profiles(events).len(), 512);
    assert!(e.snapshot().issues.contains(&CoverageIssue::ResourceLimit));
    assert!(profiles(e.advance_to(Timestamp(500_000))).is_empty());
}

#[test]
fn scope_end_publishes_last_seen() {
    let mut e = engine();
    setup(&mut e);
    let old = observed_remote(&mut e);
    apply(&mut e, 300_000, tick(vec![status(3, vec![])]));
    assert!(profiles(e.advance_to(Timestamp(500_000))).is_empty());
    let final_rows = profiles(apply(&mut e, 510_000, map("Realm")));
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0].player_id, old.player_id);
    assert_eq!(final_rows[0].last_seen, Timestamp(300_000));
    apply(&mut e, 520_000, update(vec![remote(), target()], vec![]));
    apply(&mut e, 530_000, damage(3, 1, 100));
    let new = profiles(e.advance_to(Timestamp(750_000))).remove(0);
    assert_eq!(new.scope_id, "test:map:2");
    assert_eq!(new.run_id, None);
    assert_eq!(new.first_seen, Timestamp(520_000));
    assert_ne!(new.player_id, old.player_id);
}

#[test]
fn scope_end_flushes_pending_changes_and_finish_clears_scope() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![remote()], vec![]));
    let rows = profiles(apply(&mut e, 4000, map("The Shatters")));
    assert_eq!(rows.iter().filter(|p| p.object_id == 3).count(), 1);
    apply(&mut e, 5000, update(vec![remote()], vec![]));
    assert_eq!(
        profiles(e.finish(Timestamp(6000), FightEnd::CaptureStopped)).len(),
        1
    );
    assert!(profiles(e.finish(Timestamp(7000), FightEnd::CaptureStopped)).is_empty());
}

#[test]
fn fights_carry_scope_id() {
    for (name, scope) in [("The Void", "test:run:1"), ("Realm", "test:map:1")] {
        let mut e = engine();
        apply(&mut e, 0, map(name));
        apply(&mut e, 1000, update(vec![remote(), target()], vec![]));
        let events = apply(&mut e, 2000, damage(3, 1, 100));
        let f = events
            .into_iter()
            .find_map(|e| match e {
                EngineEvent::FightStarted(f) => Some(f),
                _ => None,
            })
            .unwrap();
        assert_eq!(f.scope_id.as_deref(), Some(scope));
        assert_eq!(e.snapshot().fights[0].scope_id.as_deref(), Some(scope));
    }
}

#[test]
fn run_ending_early_keeps_scope_until_map_exit() {
    let mut e = engine();
    setup(&mut e);
    observed_remote(&mut e);
    apply(&mut e, 300_000, death());
    apply(&mut e, 310_000, tick(vec![status(3, vec![stat(30, 43)])]));
    let p = profiles(e.advance_to(Timestamp(500_000))).remove(0);
    assert_eq!(p.scope_id, "test:run:1");
    assert_eq!(p.run_id.as_deref(), Some("test:run:1"));
}

#[test]
fn local_identity_merge_rekeys_profile() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    apply(
        &mut e,
        2000,
        update(vec![object(1, 100, vec![string(31, "Synthetic")])], vec![]),
    );
    let old = profiles(e.advance_to(Timestamp(250_000))).remove(0);
    assert!(old.is_local);
    let events = apply(
        &mut e,
        300_000,
        tick(vec![status(1, vec![string(38, "synthetic-account")])]),
    );
    let verified = events
        .into_iter()
        .find_map(|e| match e {
            EngineEvent::CharacterMerged {
                provisional_id,
                verified_id,
            } => {
                assert_eq!(provisional_id, old.player_id);
                Some(verified_id)
            }
            _ => None,
        })
        .unwrap();
    let next = profiles(e.advance_to(Timestamp(500_000)));
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].player_id, verified);
    assert_eq!(next[0].first_seen, old.first_seen);
    apply(&mut e, 510_000, tick(vec![status(1, vec![])]));
    let final_rows = profiles(apply(&mut e, 520_000, map("Nexus")));
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0].player_id, verified);
}

#[test]
fn death_notice_sets_player_id_on_unique_match() {
    for duplicate in [false, true] {
        let mut e = engine();
        setup(&mut e);
        let p = observed_remote(&mut e);
        if duplicate {
            apply(
                &mut e,
                260_000,
                update(
                    vec![object(4, 100, vec![string(31, "Remote,duplicate")])],
                    vec![],
                ),
            );
        }
        let rows = crate::d_support::instances(apply(
            &mut e,
            270_000,
            crate::d_support::player_death("Remote,fe14", 20, "Synthetic Boss", 9),
        ));
        let InstanceEventKind::PlayerDeath { player_id, .. } = &rows[0].kind else {
            panic!("death expected")
        };
        assert_eq!(player_id.as_ref(), (!duplicate).then_some(&p.player_id));
    }
}

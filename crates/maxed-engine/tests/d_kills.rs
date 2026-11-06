use crate::d_support;
use d_support::*;
use maxed_types::*;
#[test]
fn late_resolved_quest_ends_at_pending_victory() {
    for clear in [false, true] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 2500, quest(42, vec![]));
        apply(&mut e, 3000, tick(vec![]));
        let maxed_protocol::Packet::Damage(mut p) = damage(1, 1, 100) else {
            unreachable!()
        };
        p.targetId = 42;
        apply(&mut e, 4000, maxed_protocol::Packet::Damage(p));
        apply(&mut e, 6000, victory());
        if clear {
            apply(&mut e, 6500, quest(-1, vec![]));
        }
        apply(
            &mut e,
            10_000,
            update(vec![object(42, 300, vec![])], vec![]),
        );
        let f = e.snapshot().fights.remove(0);
        assert_eq!(f.kill.unwrap().at, Timestamp(6000));
        assert_eq!(f.ended, Some(Timestamp(6000)));
    }
}

#[test]
fn pending_quest_victory_excludes_gap_damage_and_keeps_estimate_reconciliation() {
    for resolve_first in [false, true] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 2500, quest(42, vec![]));
        apply(&mut e, 3000, tick(vec![]));
        // Admit a placeholder without resolving the quest's generation.
        let maxed_protocol::Packet::Damage(mut p) = damage(1, 0, 0) else {
            unreachable!()
        };
        p.targetId = 42;
        apply(&mut e, 3500, maxed_protocol::Packet::Damage(p));
        apply(&mut e, 4000, shoot(1));
        let maxed_protocol::Packet::EnemyHit(mut hit) = hit(1) else {
            unreachable!()
        };
        hit.targetId = 42;
        apply(&mut e, 4500, maxed_protocol::Packet::EnemyHit(hit));
        assert!(e.snapshot().fights[0].total_damage > 0.);
        assert!(e.snapshot().fights[0].players[0].sources[0].estimated);
        apply(&mut e, 6000, victory());
        if resolve_first {
            apply(&mut e, 6500, update(vec![object(42, 300, vec![])], vec![]));
        }
        let maxed_protocol::Packet::Damage(mut p) = damage(1, 1, 123) else {
            unreachable!()
        };
        p.targetId = 42;
        apply(&mut e, 7000, maxed_protocol::Packet::Damage(p));
        apply(&mut e, 8000, tick(vec![]));
        let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 500) else {
            unreachable!()
        };
        p.targetId = 42;
        apply(&mut e, 9000, maxed_protocol::Packet::Damage(p));
        if !resolve_first {
            apply(
                &mut e,
                10_000,
                update(vec![object(42, 300, vec![])], vec![]),
            );
        }
        let f = e.snapshot().fights.remove(0);
        assert_eq!(f.total_damage, 123.);
        assert!(!f.players[0].sources[0].estimated);
        assert_eq!(f.last_hit, Some(Timestamp(3000)));
        assert_eq!(f.ended, Some(Timestamp(6000)));
    }
}

#[test]
fn removal_boundary_excludes_gap_damage_but_reconciles_pre_removal_estimates() {
    for confirm_first in [false, true] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 3000, tick(vec![]));
        apply(&mut e, 4000, shoot(1));
        apply(&mut e, 4500, hit(1));
        apply(&mut e, 6000, update(vec![], vec![2]));
        if confirm_first {
            apply(&mut e, 6500, death_announcement("Synthetic_300"));
        }
        apply(&mut e, 7000, damage(1, 1, 123));
        assert_eq!(e.snapshot().fights[0].total_damage, 123.);
        assert!(!e.snapshot().fights[0].players[0].sources[0].estimated);
        apply(&mut e, 8000, tick(vec![]));
        apply(&mut e, 9000, damage(1, 2, 500));
        if !confirm_first {
            apply(&mut e, 10_000, death_announcement("Synthetic_300"));
        }
        let f = e.snapshot().fights.remove(0);
        assert_eq!(f.total_damage, 123.);
        assert_eq!(f.last_hit, Some(Timestamp(3000)));
        assert_eq!(f.ended, Some(Timestamp(6000)));
        assert_eq!(f.kill.unwrap().at, Timestamp(6000));
    }
}

#[test]
fn removal_without_confirmation_keeps_reports_from_the_pre_removal_combat_tick() {
    let mut e = started();
    apply(&mut e, 5000, tick(vec![]));
    apply(&mut e, 6000, update(vec![], vec![2]));
    apply(&mut e, 7000, damage(1, 2, 50));
    apply(&mut e, 8000, death_announcement("Synthetic_300"));
    let f = e.snapshot().fights.remove(0);
    assert_eq!(f.total_damage, 150.);
    assert_eq!(f.last_hit, Some(Timestamp(5000)));
    assert_eq!(f.ended, Some(Timestamp(6000)));
}

#[test]
fn later_diagnostics_mark_open_fights_only() {
    for (diagnostic, issue) in [
        (
            maxed_protocol::Diagnostic::Recovering,
            CoverageIssue::StreamGap,
        ),
        (
            maxed_protocol::Diagnostic::Packet {
                id: 10,
                problem: maxed_protocol::PacketDiagnostic::BadFields,
                alignment_verified: true,
            },
            CoverageIssue::DecodeFailure,
        ),
    ] {
        let mut e = started();
        kill(&mut e);
        apply(&mut e, 7000, update(vec![object(4, 300, vec![])], vec![]));
        let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 10) else {
            unreachable!()
        };
        p.targetId = 4;
        apply(&mut e, 8000, maxed_protocol::Packet::Damage(p));
        e.diagnostic(
            ConnId(1),
            Direction::ServerToClient,
            Timestamp(9000),
            diagnostic,
        );
        let events = apply(&mut e, 10_000, map("Nexus"));
        let fights: Vec<_> = events
            .iter()
            .filter_map(|event| {
                if let EngineEvent::FightEnded(f) = event {
                    Some(f)
                } else {
                    None
                }
            })
            .collect();
        let killed = fights.iter().find(|f| f.target_id == 2).unwrap();
        let open = fights.iter().find(|f| f.target_id == 4).unwrap();
        assert!(killed.complete_capture);
        assert!(!killed.issues.contains(&issue));
        assert!(!open.complete_capture);
        assert!(open.issues.contains(&issue));
    }
}
#[test]
fn killed_label_replaces_map_changed_only() {
    let mut e = started();
    kill(&mut e);
    assert_eq!(
        ended(apply(&mut e, 7000, map("Nexus"))).end_reason,
        Some(FightEnd::Killed)
    );
}
#[test]
fn killed_retirement_keeps_complete_capture_rules() {
    for gap in [false, true] {
        let mut e = started();
        if gap {
            e.diagnostic(
                ConnId(1),
                Direction::ServerToClient,
                Timestamp(4500),
                maxed_protocol::Diagnostic::Recovering,
            );
        }
        kill(&mut e);
        let f = ended(apply(&mut e, 7000, map("Nexus")));
        assert_eq!(f.end_reason, Some(FightEnd::Killed));
        assert_eq!(f.complete_capture, !gap);
    }
}
#[test]
fn capture_stopped_after_kill_preserves_kill_end_and_coverage() {
    for reason in [FightEnd::CaptureStopped, FightEnd::AppClosed] {
        let mut e = started();
        kill(&mut e);
        let f = ended(e.finish(Timestamp(7000), reason));
        assert_eq!(f.end_reason, Some(FightEnd::Killed));
        assert_eq!(f.ended, Some(Timestamp(6000)));
        assert!(f.kill.is_some());
        assert!(f.complete_capture);
    }
}

#[test]
fn resource_retirement_after_kill_records_resource_issue() {
    let mut e = started();
    kill(&mut e);
    let f = ended(e.finish(Timestamp(7000), FightEnd::ResourceLimit));
    assert_eq!(f.end_reason, Some(FightEnd::Killed));
    assert_eq!(f.ended, Some(Timestamp(6000)));
    assert!(f.issues.contains(&CoverageIssue::ResourceLimit));
    assert!(!f.complete_capture);
}

#[test]
fn server_report_after_kill_reconciles_only_pending_pre_kill_combat() {
    for later_tick in [false, true] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 3000, tick(vec![]));
        apply(&mut e, 4000, shoot(1));
        apply(&mut e, 4500, hit(1));
        apply(&mut e, 5000, quest(2, vec![]));
        apply(&mut e, 6000, victory());
        let before = e.snapshot().fights.remove(0);
        assert!(!before.players[0].sources[0].estimated);
        if later_tick {
            apply(&mut e, 7000, tick(vec![]));
        }
        apply(&mut e, 8000, damage(1, 1, 123));
        // A new pair after kill must never add damage, even on the old tick.
        apply(&mut e, 8500, damage(1, 2, 500));
        let after = e.snapshot().fights.remove(0);
        assert_eq!(after.total_damage, 123.);
        assert!(!after.players[0].sources[0].estimated);
        assert_eq!(after.ended, Some(Timestamp(6000)));
        assert_eq!(after.last_hit, before.last_hit);
        assert_eq!(after.end_reason, Some(FightEnd::Killed));
    }
}

#[test]
fn confirmed_target_still_counts_in_victory_and_announcement_ambiguity() {
    for announcement in [false, true] {
        let mut e = started();
        kill(&mut e);
        if announcement {
            apply(
                &mut e,
                7000,
                oryx_killed("Synthetic_300", "SyntheticKiller"),
            );
        }
        apply(&mut e, 7100, update(vec![object(4, 300, vec![])], vec![]));
        let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 10) else {
            unreachable!()
        };
        p.targetId = 4;
        apply(&mut e, 7200, maxed_protocol::Packet::Damage(p));
        apply(
            &mut e,
            7300,
            if announcement {
                oryx_killed("Synthetic_300", "SyntheticKiller")
            } else {
                victory()
            },
        );
        let snapshot = e.snapshot();
        assert!(snapshot
            .fights
            .iter()
            .find(|f| f.target_id == 4)
            .unwrap()
            .kill
            .is_none());
        assert_eq!(
            snapshot
                .fights
                .iter()
                .find(|f| f.target_id == 2)
                .unwrap()
                .kill
                .as_ref()
                .unwrap()
                .evidence,
            KillEvidence::HpZeroRemoved
        );
    }
}
#[test]
fn kill_view_published_at_confirmation() {
    let mut e = started();
    let events = kill(&mut e);
    assert!(events.iter().any(
        |e| matches!(e,EngineEvent::FightUpdated(f) if f.kill.is_some() && f.ended == Some(Timestamp(6000)))
    ));
    let f = e.snapshot().fights.remove(0);
    assert_eq!(f.kill.unwrap().evidence, KillEvidence::HpZeroRemoved);
    assert_eq!(f.ended, Some(Timestamp(6000)));
}
#[test]
fn target_hp_tracks_server_hp() {
    let mut e = started();
    apply(&mut e, 4500, tick(vec![status(2, vec![stat(1, 123)])]));
    assert_eq!(e.snapshot().fights[0].target_hp, Some(123));
}
#[test]
fn target_kind_precedence() {
    for (boss, event, mini, quested, want) in [
        (true, true, true, true, TargetKind::Boss),
        (false, true, true, true, TargetKind::EventBoss),
        (false, false, true, true, TargetKind::Miniboss),
        (false, false, false, true, TargetKind::Quest),
        (false, false, false, false, TargetKind::Ordinary),
    ] {
        let mut c = catalog();
        let o = c.objects.iter_mut().find(|o| o.object_type == 300).unwrap();
        o.is_boss = boss;
        o.event_boss = Some(event);
        o.is_miniboss = mini;
        let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
        setup(&mut e);
        if quested {
            apply(&mut e, 2500, quest(2, vec![]));
        }
        apply(&mut e, 3000, damage(1, 1, 100));
        assert_eq!(e.snapshot().fights[0].target_kind, Some(want));
    }
}
#[test]
fn drop_without_hp_zero_is_not_a_kill() {
    let mut e = started();
    apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, 100)])]));
    apply(&mut e, 6000, update(vec![], vec![2]));
    assert!(e.snapshot().fights[0].kill.is_none());
}
#[test]
fn victory_with_two_boss_fights_and_no_quest_is_unattributed() {
    let mut e = started();
    apply(&mut e, 4100, update(vec![object(4, 300, vec![])], vec![]));
    let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 10) else {
        unreachable!()
    };
    p.targetId = 4;
    apply(&mut e, 4200, maxed_protocol::Packet::Damage(p));
    apply(&mut e, 5000, victory());
    assert!(e.snapshot().fights.iter().all(|f| f.kill.is_none()));
}

#[test]
fn quest_kind_survives_clear_move_and_retirement_without_generation_leak() {
    for next in [-1, 4] {
        let mut c = catalog();
        c.objects
            .iter_mut()
            .find(|o| o.object_type == 300)
            .unwrap()
            .is_boss = false;
        let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
        setup(&mut e);
        apply(&mut e, 3000, damage(1, 1, 100));
        apply(&mut e, 4000, quest(2, vec![]));
        apply(&mut e, 5000, quest(next, vec![]));
        assert_eq!(e.snapshot().fights[0].target_kind, Some(TargetKind::Quest));
        let f = ended(apply(&mut e, 6000, map("Nexus")));
        assert_eq!(f.target_kind, Some(TargetKind::Quest));
    }
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, quest(2, vec![]));
    // Changed type proves a new generation while the old quest ID stays set.
    apply(&mut e, 4000, update(vec![object(2, 301, vec![])], vec![]));
    apply(&mut e, 5000, damage(1, 1, 100));
    assert_eq!(
        e.snapshot().fights[0].target_kind,
        Some(TargetKind::Ordinary)
    );
}

#[test]
fn live_quest_arrow_moves_between_historical_quest_fights() {
    let mut c = catalog();
    c.objects
        .iter_mut()
        .find(|o| o.object_type == 300)
        .unwrap()
        .is_boss = false;
    let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, quest(2, vec![]));
    assert!(e.snapshot().fights[0].is_current_quest);
    apply(&mut e, 5000, update(vec![object(4, 300, vec![])], vec![]));
    let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 50) else {
        unreachable!()
    };
    p.targetId = 4;
    apply(&mut e, 6000, maxed_protocol::Packet::Damage(p));
    apply(&mut e, 7000, quest(4, vec![]));
    for f in e.snapshot().fights {
        assert_eq!(f.target_kind, Some(TargetKind::Quest));
        assert_eq!(f.is_current_quest, f.target_id == 4);
    }
    apply(&mut e, 8000, quest(-1, vec![]));
    for f in e.snapshot().fights {
        assert_eq!(f.target_kind, Some(TargetKind::Quest));
        assert!(!f.is_current_quest);
    }
    // Repointing the arrow dirties both fights even though neither kind changes.
    apply(&mut e, 300_000, quest(2, vec![]));
    apply(&mut e, 600_000, quest(4, vec![]));
    let events = e.advance_to(Timestamp(900_000));
    let updates: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::FightUpdated(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(updates.len(), 2);
    assert!(updates
        .iter()
        .all(|f| f.is_current_quest == (f.target_id == 4)));
    let f = ended(apply(&mut e, 1_200_000, map("Nexus")));
    assert!(!f.is_current_quest);
}

#[test]
fn local_hit_recency_does_not_follow_other_players() {
    let mut e = started();
    assert_eq!(e.snapshot().fights[0].local_last_hit, Some(Timestamp(3000)));
    apply(
        &mut e,
        5000,
        update(vec![player_obj(3, 100, "SyntheticRemote")], vec![]),
    );
    apply(&mut e, 6000, tick(vec![]));
    apply(&mut e, 6100, damage(3, 2, 50));
    let f = e.snapshot().fights.remove(0);
    assert_eq!(f.local_last_hit, Some(Timestamp(3000)));
    assert_eq!(f.last_hit, Some(Timestamp(6000)));
}

#[test]
fn confirmed_victory_ends_at_kill_before_later_hits_and_exit() {
    let mut e = started();
    apply(&mut e, 4500, quest(2, vec![]));
    apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, 123)])]));
    apply(&mut e, 6000, victory());
    let f = e.snapshot().fights.remove(0);
    assert_eq!(f.ended, Some(Timestamp(6000)));
    assert_eq!(f.end_reason, Some(FightEnd::Killed));
    apply(&mut e, 7000, tick(vec![]));
    apply(&mut e, 8000, damage(1, 2, 50));
    let final_fight = ended(apply(&mut e, 9000, map("Nexus")));
    assert_eq!(final_fight.ended, f.ended);
    assert_eq!(final_fight.last_hit, f.last_hit);
    assert_eq!(final_fight.total_damage, f.total_damage);
}
#[test]
fn provisional_announcement_does_not_end_fight() {
    let mut e = started();
    apply(
        &mut e,
        5000,
        oryx_killed("Synthetic_300", "SyntheticKiller"),
    );
    let f = e.snapshot().fights.remove(0);
    assert!(f.kill.as_ref().unwrap().provisional);
    assert_eq!(f.ended, None);
    assert_eq!(
        ended(apply(&mut e, 9000, map("Nexus"))).end_reason,
        Some(FightEnd::MapChanged)
    );
}

fn death_announcement(key: &str) -> maxed_protocol::Packet {
    let maxed_protocol::Packet::Text(mut p) = oryx_killed(key, "SyntheticKiller") else {
        unreachable!()
    };
    p.text = format!(r#"{{"k":"stringlist.{key}.death.0"}}{{"k":"stringlist.{key}.death.0"}}"#);
    maxed_protocol::Packet::Text(p)
}
#[test]
fn announcement_and_removal_confirm_death_in_either_order() {
    for hp in [1, 428] {
        for delay in [-500_000i64, -100_000, 0, 100_000, 500_000] {
            let mut e = started();
            apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, hp)])]));
            let removal = 1_000_000;
            let announcement = (removal as i64 + delay) as u64;
            if delay < 0 {
                apply(
                    &mut e,
                    announcement,
                    oryx_killed("Synthetic_300", "SyntheticKiller"),
                );
                apply(&mut e, removal, update(vec![], vec![2]));
            } else {
                apply(&mut e, removal, update(vec![], vec![2]));
                apply(&mut e, announcement, death_announcement("Synthetic_300"));
            }
            let f = e.snapshot().fights.remove(0);
            let kill = f.kill.unwrap();
            assert!(!kill.provisional, "HP {hp}, delay {delay}");
            assert_eq!(
                serde_json::to_value(kill.evidence).unwrap(),
                "announced_death_removed"
            );
            assert_eq!(kill.at, Timestamp(removal));
            assert_eq!(f.ended, Some(kill.at));
            assert_eq!(f.end_reason, Some(FightEnd::Killed));
        }
    }
}
#[test]
fn removal_without_announcement_and_stale_pairs_are_not_confirmed() {
    let mut e = started();
    apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, 1)])]));
    apply(&mut e, 1_000_000, update(vec![], vec![2]));
    assert!(e.snapshot().fights[0].kill.is_none());
    apply(&mut e, 1_500_001, death_announcement("Synthetic_300"));
    assert!(e.snapshot().fights[0].kill.as_ref().unwrap().provisional);
    assert!(e.snapshot().fights[0].ended.is_none());
    let mut e = started();
    apply(&mut e, 5000, death_announcement("Synthetic_300"));
    assert!(e.snapshot().fights[0].kill.as_ref().unwrap().provisional);
    apply(&mut e, 505_001, update(vec![], vec![2]));
    assert!(e.snapshot().fights[0].kill.as_ref().unwrap().provisional);
    assert!(e.snapshot().fights[0].ended.is_none());
}

#[test]
fn leading_the_catalog_name_matches_a_death_key() {
    let mut c = catalog();
    c.objects
        .iter_mut()
        .find(|o| o.object_type == 300)
        .unwrap()
        .name = "The Plague Doctor".into();
    let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, update(vec![], vec![2]));
    apply(&mut e, 4000, death_announcement("The_Plague_Doctor"));
    let f = e.snapshot().fights.remove(0);
    assert_eq!(
        f.kill.unwrap().evidence,
        KillEvidence::AnnouncedDeathRemoved
    );
    assert_eq!(f.ended, Some(Timestamp(4000)));
}

#[test]
fn late_reconciliation_preserves_time_bounds_before_kill_confirmation() {
    let mut e = engine();
    setup(&mut e);
    for (tick_at, hit_at, bullet) in [(1_000_000, 1_100_000, 1), (4_000_000, 4_200_000, 2)] {
        apply(&mut e, tick_at, tick(vec![]));
        apply(&mut e, tick_at + 10_000, shoot(bullet));
        apply(&mut e, hit_at, hit(bullet));
    }
    apply(&mut e, 4_210_000, damage(1, 2, 100));
    apply(&mut e, 4_500_000, update(vec![], vec![2]));
    let before = e.snapshot().fights.remove(0);
    assert!(before.ended.is_none());
    assert_eq!(before.first_hit, Some(Timestamp(1_000_000)));
    assert_eq!(before.last_hit, Some(Timestamp(4_000_000)));
    assert_eq!(before.local_last_hit, Some(Timestamp(4_200_000)));
    assert_eq!(before.players[0].dps, Some(200. / 3.1));

    apply(&mut e, 4_600_000, tick(vec![]));
    apply(&mut e, 4_700_000, damage(1, 1, 100));
    let snapshot = e.snapshot();
    let after = &snapshot.fights[0];
    assert!(after.ended.is_none());
    assert_eq!(after.last_hit, before.last_hit);
    assert_eq!(after.first_hit, before.first_hit);
    assert_eq!(after.started, before.started);
    assert_eq!(after.local_last_hit, before.local_last_hit);
    assert_eq!(after.players[0].dps, before.players[0].dps);
    assert_eq!(after.total_damage, before.total_damage);
    assert_eq!(after.buckets, before.buckets);

    let events = apply(&mut e, 5_000_000, map("Nexus"));
    let fight = ended(events);
    let candidate =
        maxed_engine::alerts::personal_best_candidate(&fight, snapshot.character.as_ref().unwrap())
            .unwrap();
    assert_eq!(candidate.at, Timestamp(4_000_000));
    assert_eq!(candidate.dps, 200. / 3.1);
}

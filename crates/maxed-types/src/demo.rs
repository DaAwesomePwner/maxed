//! Synthetic contract fixtures for UI development, not runtime replay output.
//! All names, IDs, types and times are invented. No client assets are required.

use crate::*;

fn missing_enchant() -> EnchantView {
    EnchantView {
        state: EnchantState::Missing,
        slots: None,
        applied: None,
        ids: vec![],
    }
}

/// An observed completion with a still-open target window and partial identity.
pub fn demo_snapshot() -> LiveSnapshot {
    let character = CharacterView {
        account_id: None,
        calculation_revision: None,
        stat_panel: None,
        id: "demo-character".into(),
        name: Some("Demo Knight".into()),
        class_id: Some(100),
        level: Some(20),
        maxed: None,
        fame: Some(1250),
        stats: vec![StatView {
            stat_id: 20,
            value: 50,
        }],
        equipment: vec![EquipmentView {
            observed_empty: None,
            slot: 0,
            item_type: Some(200),
            enchant: missing_enchant(),
        }],
        observed_at: Timestamp(1_000_000),
        identity_verified: false,
        died_at: None,
        issues: vec![CoverageIssue::MissingIdentity, CoverageIssue::MissingStats],
    };
    let run = RunView {
        context: None,
        id: "demo-run".into(),
        session_id: "demo-session".into(),
        character_id: Some(character.id.clone()),
        dungeon: "Synthetic Sanctum".into(),
        area_kind: AreaKind::Dungeon,
        started: Timestamp(1_000_000),
        ended: None,
        completed_at: Some(Timestamp(5_000_000)),
        died_at: None,
        outcome: Some(Outcome::Cleared),
        death: None,
        evidence: vec![EvidenceAt {
            at: Timestamp(5_000_000),
            evidence: Evidence::Victory,
        }],
        party_size: Some(1),
        party_size_estimated: true,
        complete_capture: false,
        issues: character.issues.clone(),
    };
    let fight = FightView {
        owner_character_id: None,
        scope_id: None,
        dps_basis: DpsBasis::PlayerActiveWindowV1,
        pending_local_reconciliation: false,
        is_current_quest: false,
        local_last_hit: None,
        local_active_micros: None,
        kill: None,
        target_hp: None,
        target_kind: None,
        player_series: None,
        id: "demo-fight".into(),
        run_id: Some(run.id.clone()),
        session_id: run.session_id.clone(),
        target_id: 2,
        target_generation: 0,
        boss_type: Some(300),
        target_type: Some(300),
        max_hp: None,
        started: Timestamp(2_000_000),
        ended: None,
        first_hit: Some(Timestamp(2_000_000)),
        last_hit: Some(Timestamp(4_000_000)),
        end_reason: None,
        players: vec![PlayerDamage {
            object_id: 1,
            player_id: character.id.clone(),
            name: character.name.clone(),
            class_id: character.class_id,
            is_local: true,
            damage: 300.0,
            hits: 2,
            biggest_hit: Some(200.0),
            deaths: 0,
            dps: Some(150.0),
            share: Some(1.0),
            rank: Some(1),
            sources: vec![SourceDamage {
                item_type: Some(200),
                ability: false,
                kind: SourceKind::Weapon,
                enchant: missing_enchant(),
                damage: 300.0,
                hits: 2,
                biggest_hit: Some(200.0),
                estimated: false,
                reasons: vec![],
            }],
        }],
        total_damage: 300.0,
        unattributed_damage: 0.0,
        buckets: vec![
            DamageBucket {
                from: Timestamp(2_000_000),
                until: Timestamp(3_000_000),
                total: 100.0,
                local: Some(100.0),
                others_damage: None,
                others_presence: None,
            },
            DamageBucket {
                from: Timestamp(3_000_000),
                until: Timestamp(4_000_001),
                total: 200.0,
                local: Some(200.0),
                others_damage: None,
                others_presence: None,
            },
        ],
        complete_capture: false,
        issues: character.issues.clone(),
    };
    let drop = DropView {
        owner_character_id: None,
        capture_scope: None,
        id: "demo-drop".into(),
        session_id: run.session_id.clone(),
        run_id: Some(run.id.clone()),
        at: Timestamp(5_000_000),
        bag_type: 400,
        items: vec![200],
        enchant: None,
        contents: vec![DropItem {
            taken_observed_at: None,
            slot: 0,
            item_type: 200,
            quantity: 1,
            enchant: missing_enchant(),
        }],
        source_enemy: None,
        source_estimated: false,
        source_reason: None,
        contents_known: true,
        bag_rank: None,
        issues: vec![],
    };
    LiveSnapshot {
        instance: None,
        run_totals: None,
        at: Timestamp(5_000_000),
        stream_id: "demo-stream".into(),
        sequence: Sequence(Timestamp(12)),
        capture: CaptureStatus {
            message: "Synthetic contract preview; capture stopped".into(),
            ..CaptureStatus::default()
        },
        session: Some(PlaySessionView {
            issues: vec![],
            id: run.session_id.clone(),
            started: run.started,
            last_traffic: Timestamp(5_000_000),
            ended: None,
        }),
        character: Some(character),
        map: Some(run.dungeon.clone()),
        area_kind: AreaKind::Dungeon,
        issues: run.issues.clone(),
        run: Some(run),
        fights: vec![fight],
        drops: vec![drop],
    }
}

/// A detached historical projection of the synthetic run after an observed exit.
pub fn demo_run_story() -> RunStory {
    let snapshot = demo_snapshot();
    let mut run = snapshot.run.expect("the synthetic fixture includes a run");
    run.ended = Some(Timestamp(6_000_000));
    run.evidence.push(EvidenceAt {
        at: Timestamp(6_000_000),
        evidence: Evidence::ObservedExit,
    });
    let mut fights = snapshot.fights;
    for fight in &mut fights {
        fight.ended = run.ended;
        fight.end_reason = Some(FightEnd::MapChanged);
    }
    let timeline = [
        (
            "entry",
            1_000_000,
            TimelineKind::Entered,
            Some("demo-run"),
            "Entered Synthetic Sanctum",
        ),
        (
            "fight",
            2_000_000,
            TimelineKind::Fight,
            Some("demo-fight"),
            "Observed target damage",
        ),
        (
            "drop",
            5_000_000,
            TimelineKind::Drop,
            Some("demo-drop"),
            "Observed a bag; pickup is unknown",
        ),
        (
            "completion",
            5_000_000,
            TimelineKind::Completion,
            Some("demo-run"),
            "Observed Victory",
        ),
        (
            "exit",
            6_000_000,
            TimelineKind::Exit,
            Some("demo-run"),
            "Observed area exit",
        ),
    ]
    .into_iter()
    .map(|(id, at, kind, related, explanation)| TimelineEntry {
        id: format!("demo-timeline-{id}"),
        at: Timestamp(at),
        kind,
        related_id: related.map(str::to_owned),
        explanation: explanation.into(),
    })
    .collect();
    RunStory {
        totals: None,
        boss_fights: None,
        boss_fights_limited: None,
        events: None,
        events_limited: None,
        older_run_id: None,
        newer_run_id: None,
        run,
        character: snapshot.character,
        fights: fights
            .into_iter()
            .enumerate()
            .map(|(index, fight)| RunFightSummary {
                id: fight.id,
                target_type: fight.target_type,
                target_name: None,
                target_kind: fight.target_kind.clone(),
                boss_type: fight.boss_type,
                ordinal: index as u32 + 1,
                started: fight.started,
                ended: fight.ended,
                first_hit: fight.first_hit,
                last_hit: fight.last_hit,
                total_damage: fight.total_damage,
                local_damage: None,
                local_dps: None,
                local_rank: None,
                party: None,
                player_count: fight.players.len() as u32,
                partial: true,
                estimated: false,
            })
            .collect(),
        drops: snapshot.drops,
        timeline,
        comparison: None,
    }
}

pub fn demo_run_page() -> RunPage {
    RunPage {
        details: None,
        summary: None,
        rows: vec![demo_run_story().run],
        next: None,
    }
}

pub fn demo_home_summary() -> HomeSummary {
    let story = demo_run_story();
    HomeSummary {
        dungeon_stats: None,
        runs: 1,
        clears: 1,
        deaths: 0,
        fame_delta: None,
        fame: vec![FamePoint {
            at: Timestamp(1_000_000),
            character_id: "demo-character".into(),
            fame: 1250,
        }],
        fame_characters: vec![CharacterFame {
            character_id: "demo-character".into(),
            first_fame: 1250,
            last_fame: 1250,
            last_at: Timestamp(1_000_000),
        }],
        best_drop: None, // No bag-rank evidence is available in this fixture.
        top_fight: demo_snapshot().fights.into_iter().next().map(|mut fight| {
            fight.ended = story.run.ended;
            fight.end_reason = Some(FightEnd::MapChanged);
            fight
        }),
        issues: story.run.issues.clone(),
        recent_details: vec![HomeRunDetail {
            run_id: story.run.id.clone(),
            local_share: None, // The synthetic run carries missing-identity evidence.
            share_partial: true,
            observed_drop_count: 1,
            loot: story.drops,
            loot_preview_limited: false,
            issues: story.run.issues.clone(),
        }],
        recent_runs: vec![story.run],
        fame_missing_baselines: 1,
        fame_sampled: false,
        unranked_drop_count: 1,
    }
}

/// Minimal invented definitions for displaying the synthetic IDs without sprites.
pub fn demo_catalog() -> Catalog {
    Catalog {
        format_version: 1,
        client_fingerprint: "synthetic-contract-v1".into(),
        items: vec![ItemDef {
            weapon_firing: None,
            potion_stat: None,
            soulbound: None,
            stat_bonuses: None,
            object_type: 200,
            name: "Synthetic Sword".into(),
            tier: None,
            slot_type: Some(1),
            projectiles: vec![ProjectileDef {
                index: 0,
                min_damage: 100,
                max_damage: 200,
                armor_piercing: false,
                lifetime_millis: None,
            }],
            scaling: None,
            sprite_key: None,
        }],
        objects: vec![
            ObjectDef {
                class_name: None,
                dungeon_name: None,
                event_boss: None,
                realm_event_key: None,
                object_type: 100,
                name: "Demo Knight".into(),
                is_player: true,
                is_boss: false,
                is_miniboss: false,
                sprite_key: None,
            },
            ObjectDef {
                class_name: None,
                dungeon_name: None,
                event_boss: None,
                realm_event_key: None,
                object_type: 300,
                name: "Synthetic Guardian".into(),
                is_player: false,
                is_boss: true,
                is_miniboss: false,
                sprite_key: None,
            },
        ],
        classes: vec![ClassDef {
            object_type: 100,
            stat_caps: vec![StatView {
                stat_id: 20,
                value: 75,
            }],
        }],
        enchants: vec![],
        dungeons: vec![DungeonDef {
            name: "Synthetic Sanctum".into(),
            aliases: vec![],
            kind: AreaKind::Dungeon,
            completion_index: None,
            portal_type: None,
            difficulty: None,
            exalt: false,
            exalt_gateway: false,
            category: None,
        }],
        bags: vec![BagDef {
            object_type: 400,
            name: "Synthetic Bag".into(),
            base_type: 400,
            is_white: false,
            rank: None,
            capture: true,
        }],
        sprites: vec![],
        tiles: vec![],
        crucible_bonuses: vec![],
    }
}

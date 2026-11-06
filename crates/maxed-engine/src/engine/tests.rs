use crate::{Engine, EngineConfig};
use maxed_protocol::{defs::*, DecodedPacket, Packet};
use maxed_types::*;
use std::sync::Arc;

fn notice(effect: &'static str, message: &str) -> Packet {
    Packet::Notification(Notification {
        data: NotificationData {
            effect: Some(effect),
            message: Some(message.into()),
            pictureType: 300,
            ..Default::default()
        },
    })
}
fn text(name: &str, recipient: &str) -> Packet {
    Packet::Text(Text {
        name: name.into(),
        objectId: -1,
        numStars: 0,
        starBackground: 0,
        bubbleTime: 0,
        recipient: recipient.into(),
        text: r#"{"k":"stringlist.Synthetic_300.killed.0","t":{"KILLER":"SynthAlpha"}}"#.into(),
        cleanText: String::new(),
        isSupporter: false,
    })
}
#[test]
fn notification_publication_preserves_legacy_projection() {
    let mut tracked = engine();
    let mut control = engine();
    setup(&mut tracked);
    setup(&mut control);
    for (at, effect, message) in [
        (
            3000,
            "PlayerDeath",
            r#"{"k":"s.death","t":{"player":"SynthAlpha","level":"20","enemy":"Synthetic Boss",}}"#,
        ),
        (
            4000,
            "PortalOpened",
            r#"{"k":"s.dungeon_opened_by","t":{"player":"SynthBravo",}}"#,
        ),
    ] {
        assert_eq!(
            legacy_events(apply(&mut tracked, at, notice(effect, message))),
            legacy_events(apply(&mut control, at, notice("Global", "ignored")))
        );
        assert_eq!(legacy_snapshot(&tracked), legacy_snapshot(&control));
    }
    assert_eq!(tracked.notices.entries.len(), 2);
    assert!(control.notices.entries.is_empty());
    for at in [5000, 6000] {
        assert_eq!(
            legacy_events(apply(&mut tracked, at, tick(vec![]))),
            legacy_events(apply(&mut control, at, tick(vec![])))
        );
    }
    assert_eq!(
        legacy_events(apply(&mut tracked, 7000, map("Nexus"))),
        legacy_events(apply(&mut control, 7000, map("Nexus")))
    );
    assert!(tracked.notices.entries.is_empty());
}
#[test]
fn oryx_detection_requires_system_sender_and_public_text() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, text("SynthBravo", ""));
    apply(&mut e, 5000, text("#Oryx the Mad God", "SynthBravo"));
    assert!(e.notices.entries.is_empty());
    assert!(e.observed_kills.is_empty());
    let mut control = engine();
    setup(&mut control);
    apply(&mut control, 3000, damage(1, 1, 100));
    apply(&mut control, 4000, text("SynthBravo", ""));
    apply(&mut control, 5000, text("#Oryx the Mad God", "SynthBravo"));
    assert_eq!(
        legacy_events(apply(&mut e, 6000, text("#Oryx the Mad God", ""))),
        legacy_events(apply(&mut control, 6000, text("SynthBravo", "")))
    );
    assert_eq!(legacy_snapshot(&e), legacy_snapshot(&control));
    assert_eq!(e.notices.entries.len(), 1);
    assert_eq!(e.observed_kills.len(), 1);
}
#[test]
fn hp_zero_removal_ends_fight_before_retirement() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, tick(vec![status(2, vec![stat(1, 0)])]));
    let events = apply(&mut e, 5000, update(vec![], vec![2]));
    assert!(!events
        .iter()
        .any(|e| matches!(e, EngineEvent::FightEnded(_))));
    assert_eq!(e.observed_kills.len(), 1);
    assert_eq!(e.snapshot().fights[0].end_reason, Some(FightEnd::Killed));
    apply(&mut e, 6000, victory());
    assert_eq!(e.snapshot().fights[0].end_reason, Some(FightEnd::Killed));
    let events = apply(&mut e, 7000, map("Nexus"));
    assert!(events
        .iter()
        .any(|e| matches!(e,EngineEvent::FightEnded(f) if f.end_reason==Some(FightEnd::Killed))));
    assert!(e.observed_kills.is_empty());
}
#[test]
fn private_roster_is_update_driven_and_resets_on_map() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, tick(vec![status(42, vec![])]));
    assert_eq!(e.instance.population(), None);
    apply(
        &mut e,
        2000,
        update(
            vec![object(10, 100, vec![]), object(20, 100, vec![])],
            vec![],
        ),
    );
    assert_eq!(e.instance.population(), Some(2));
    apply(&mut e, 3000, update(vec![], vec![10]));
    assert_eq!(e.instance.population(), Some(1));
    apply(&mut e, 4000, map("Nexus"));
    assert_eq!(e.instance.population(), None);
}
#[test]
fn instance_event_cap_counts_drops_without_marking_coverage() {
    let mut e = engine();
    setup(&mut e);
    let issues = e.snapshot().issues;
    for _ in 0..4098 {
        apply(
            &mut e,
            3000,
            notice(
                "PortalOpened",
                r#"{"k":"s.dungeon_opened_by","t":{"player":"SynthBravo"}}"#,
            ),
        );
    }
    assert_eq!(e.notices.entries.len(), 4096);
    assert_eq!(e.notices.dropped, 2);
    assert_eq!(e.snapshot().issues, issues);
}
#[test]
fn hp_recovery_in_same_packet_cancels_zero_evidence() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(
        &mut e,
        4000,
        tick(vec![status(2, vec![stat(1, 0), stat(1, 10)])]),
    );
    apply(&mut e, 5000, update(vec![], vec![2]));
    assert!(e.observed_kills.is_empty());
}

fn quest(id: i32) -> Packet {
    Packet::QuestObjId(QuestObjId {
        objectId: id,
        list: vec![],
    })
}
fn score(value: i32) -> Packet {
    Packet::RealmScoreUpdate(RealmScoreUpdate { score: value })
}
#[test]
fn quest_clear_before_victory_same_tick_through_dispatch() {
    let mut e = engine();
    let mut control = engine();
    setup(&mut e);
    setup(&mut control);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut control, 3000, damage(1, 1, 100));
    for (at, id) in [(4000, 2), (5000, -1)] {
        assert_eq!(
            legacy_events(apply(&mut e, at, quest(id))),
            legacy_events(apply(&mut control, at, notice("Global", "ignored")))
        );
        assert_eq!(legacy_snapshot(&e), legacy_snapshot(&control));
    }
    assert!(e.instance.quest.is_none());
    assert_eq!(e.instance.previous_quest.as_ref().unwrap().0.object_id, 2);
    assert_eq!(
        legacy_events(apply(&mut e, 5000, victory())),
        legacy_events(apply(&mut control, 5000, victory()))
    );
    let key = (2, e.entities[&2].generation);
    let kill = &e.observed_kills[&key];
    assert_eq!(
        kill.evidence,
        super::kills::KillEvidence::VictoryQuestTarget
    );
    assert!(!kill.provisional);
    assert_eq!(e.snapshot().fights[0].ended, Some(Timestamp(5000)));
    let mut killed = legacy_snapshot(&e);
    killed.fights[0].ended = None;
    killed.fights[0].end_reason = None;
    assert_eq!(killed, legacy_snapshot(&control));
    // Retirement preserves the earlier confirmed end; the unconfirmed control ends here.
    let killed = apply(&mut e, 6000, map("Nexus"));
    let control = apply(&mut control, 6000, map("Nexus"));
    assert!(killed.iter().any(
        |event| matches!(event, EngineEvent::FightEnded(f) if f.ended == Some(Timestamp(5000)))
    ));
    assert!(control.iter().any(
        |event| matches!(event, EngineEvent::FightEnded(f) if f.ended == Some(Timestamp(6000)))
    ));
    assert!(e.instance.quest.is_none());
    assert!(e.instance.previous_quest.is_none());
}
#[test]
fn quest_dispatch_buffers_unknown_object_and_resolves_generation() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, quest(2));
    let q = e.instance.quest.as_ref().unwrap();
    assert_eq!(
        (q.object_id, q.object_type, q.since),
        (2, None, Timestamp(1000))
    );
    apply(&mut e, 2000, update(vec![target()], vec![]));
    apply(&mut e, 3000, damage(1, 1, 100));
    assert_eq!(e.instance.quest.as_ref().unwrap().object_type, Some(300));
    apply(
        &mut e,
        4000,
        Packet::QuestObjId(QuestObjId {
            objectId: 2,
            list: vec![2],
        }),
    );
    assert_eq!(e.instance.quest.as_ref().unwrap().since, Timestamp(1000));
    apply(&mut e, 5000, quest(-1));
    apply(&mut e, 5000, victory());
    let key = (2, e.entities[&2].generation);
    assert_eq!(
        e.observed_kills[&key].evidence,
        super::kills::KillEvidence::VictoryQuestTarget
    );
}
#[test]
fn unresolved_quest_dispatch_does_not_reuse_old_target() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, quest(2));
    apply(&mut e, 5000, quest(42));
    apply(&mut e, 6000, victory());
    assert!(e.observed_kills.is_empty());
    assert_eq!(e.instance.quest.as_ref().unwrap().object_id, 42);
}
#[test]
fn quest_clear_after_unattributed_victory_through_dispatch() {
    let mut e = engine();
    setup(&mut e);
    // An ordinary target prevents the sole-boss fallback at Victory.
    apply(&mut e, 2500, update(vec![object(2, 301, vec![])], vec![]));
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, victory());
    assert!(e.observed_kills.is_empty());
    apply(&mut e, 4500, quest(2));
    apply(&mut e, 5000, quest(-1));
    let key = (2, e.entities[&2].generation);
    assert_eq!(
        e.observed_kills[&key].evidence,
        super::kills::KillEvidence::VictoryQuestTarget
    );
}
#[test]
fn realm_score_dispatch_replaces_absolute_value_preserving_legacy_projection() {
    let mut e = engine();
    let mut control = engine();
    for engine in [&mut e, &mut control] {
        let Packet::MapInfo(mut p) = map("Realm") else {
            unreachable!()
        };
        p.maxRealmScore = 300000;
        p.currentRealmScore = 100;
        p.has_scores = true;
        apply(engine, 0, Packet::MapInfo(p));
    }
    for (at, value) in [(1000, 120), (2000, 150), (3000, -1), (4000, 180)] {
        assert_eq!(
            legacy_events(apply(&mut e, at, score(value))),
            legacy_events(apply(&mut control, at, notice("Global", "ignored")))
        );
        assert_eq!(legacy_snapshot(&e), legacy_snapshot(&control));
        assert_eq!(
            e.instance.realm_score.as_ref().map(|s| (s.current, s.max)),
            (value >= 0).then_some((value, 300000))
        );
    }
    apply(&mut e, 5000, map("Nexus"));
    apply(&mut e, 6000, score(200));
    assert!(e.instance.realm_score.is_none());
}
#[test]
fn unverified_quest_and_score_packets_do_not_feed_trackers() {
    let mut e = engine();
    setup(&mut e);
    for packet in [quest(2), score(100)] {
        e.apply(DecodedPacket {
            conn: ConnId(1),
            dir: Direction::ServerToClient,
            at: Timestamp(3000),
            id: 0,
            len: 0,
            alignment_verified: false,
            packet,
        });
    }
    assert!(e.instance.quest.is_none());
    assert!(e.instance.realm_score.is_none());
    assert!(e.observed_kills.is_empty());
}

fn catalog() -> Catalog {
    Catalog {
        format_version: 1,
        client_fingerprint: "synthetic".into(),
        items: vec![
            ItemDef {
                weapon_firing: None,
                potion_stat: None,
                soulbound: None,
                stat_bonuses: None,
                object_type: 200,
                name: "Synthetic sword".into(),
                tier: None,
                slot_type: Some(1),
                projectiles: vec![ProjectileDef {
                    index: 0,
                    min_damage: 100,
                    max_damage: 100,
                    armor_piercing: false,
                    lifetime_millis: None,
                }],
                scaling: None,
                sprite_key: None,
            },
            ItemDef {
                weapon_firing: None,
                potion_stat: None,
                soulbound: None,
                stat_bonuses: None,
                object_type: 201,
                name: "Synthetic ability".into(),
                tier: None,
                slot_type: Some(4),
                projectiles: vec![ProjectileDef {
                    index: 0,
                    min_damage: 50,
                    max_damage: 50,
                    armor_piercing: true,
                    lifetime_millis: None,
                }],
                scaling: Some(ScalingDef {
                    stat_id: 27,
                    minimum: 50,
                    damage_per_stat: 2.,
                    shots: 3,
                    origin_item: None,
                }),
                sprite_key: None,
            },
        ],
        objects: vec![
            object_def(100, true, false),
            object_def(300, false, true),
            object_def(301, false, false),
        ],
        classes: vec![ClassDef {
            object_type: 100,
            stat_caps: vec![StatView {
                stat_id: 20,
                value: 75,
            }],
        }],
        enchants: vec![],
        dungeons: vec![
            dungeon("Nexus", AreaKind::Nexus),
            dungeon("Realm", AreaKind::Realm),
            dungeon("The Void", AreaKind::Dungeon),
            dungeon("The Shatters", AreaKind::Dungeon),
            dungeon("Moonlight Village", AreaKind::Dungeon),
        ],
        bags: vec![BagDef {
            object_type: 1292,
            name: "White".into(),
            base_type: 1292,
            is_white: true,
            rank: Some(255),
            capture: true,
        }],
        sprites: vec![],
        tiles: vec![],
        crucible_bonuses: vec![],
    }
}

fn object_def(id: u16, player: bool, boss: bool) -> ObjectDef {
    ObjectDef {
        class_name: None,
        dungeon_name: None,
        event_boss: None,
        realm_event_key: None,
        object_type: id,
        name: format!("Synthetic {id}"),
        is_player: player,
        is_boss: boss,
        is_miniboss: false,
        sprite_key: None,
    }
}

fn dungeon(name: &str, kind: AreaKind) -> DungeonDef {
    DungeonDef {
        name: name.into(),
        aliases: vec![],
        kind,
        completion_index: Some(65),
        portal_type: None,
    }
}

fn config() -> EngineConfig {
    EngineConfig {
        stream_id: "test".into(),
        identity_key: [7; 32],
        ..Default::default()
    }
}

fn engine() -> Engine {
    Engine::new(config(), Arc::new(catalog())).unwrap()
}

#[test]
fn guild_retention_excludes_enemy_and_unknown_string_stats() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    let guild = format!("{}é", "x".repeat(63));
    apply(
        &mut e,
        1000,
        update(
            vec![
                object(
                    3,
                    100,
                    vec![string(62, &guild), string(63, "Synthetic rank")],
                ),
                object(4, 301, vec![string(62, "Synthetic Guild")]),
            ],
            vec![],
        ),
    );
    assert_eq!(
        e.entities[&3].guild.as_deref(),
        Some("x".repeat(63).as_str())
    );
    assert!(e.entities[&4].guild.is_none());
    assert!(!e.entities[&3].stats.contains_key(&63));
}

fn pos() -> WorldPosData {
    WorldPosData { x: 0., y: 0. }
}

fn stat(id: u8, n: i32) -> StatData {
    StatData {
        statTypeNum: id,
        statType: None,
        statValue: n,
        stringStatValue: None,
        statValueTwo: 0,
    }
}

fn string(id: u8, v: &str) -> StatData {
    StatData {
        stringStatValue: Some(v.into()),
        ..stat(id, 0)
    }
}

fn status(id: i32, stats: Vec<StatData>) -> ObjectStatusData {
    ObjectStatusData {
        objectId: id,
        pos: pos(),
        stats,
    }
}

fn object(id: i32, ty: u16, stats: Vec<StatData>) -> ObjectData {
    ObjectData {
        objectType: ty,
        status: status(id, stats),
    }
}

fn update(objects: Vec<ObjectData>, drops: Vec<i32>) -> Packet {
    Packet::Update(Update {
        pos: pos(),
        levelType: 0,
        tiles: vec![],
        newObjects: objects,
        drops,
    })
}

fn tick(stats: Vec<ObjectStatusData>) -> Packet {
    Packet::NewTick(NewTick {
        tickId: 0,
        tickTime: 200,
        serverRealTimeMS: 0,
        serverLastTimeRTTMS: 0,
        status: stats,
    })
}

fn map(name: &str) -> Packet {
    Packet::MapInfo(MapInfo {
        width: 1,
        height: 1,
        name: name.into(),
        displayName: name.into(),
        realmName: String::new(),
        fp: 1,
        seed: 1,
        background: 0,
        difficulty: 0.,
        allowPlayerTeleport: false,
        noSave: false,
        showDisplays: false,
        maxPlayerCount: 85,
        gameOpenedTime: 0,
        versionNumber: String::new(),
        unknown1: 0,
        viewDistance: 0,
        unknown2: false,
        unknownInt: 0,
        modifiers: Default::default(),
        dungeonModifiers: None,
        dungeonModifiers2: None,
        dungeonModifiers3: None,
        dungeonModifiers4: None,
        dungeonGrade: None,
        bgColor: 0,
        has_scores: false,
        maxRealmScore: 0,
        currentRealmScore: 0,
    })
}

fn create(id: i32, char_id: i32) -> Packet {
    Packet::CreateSuccess(CreateSuccess {
        objectId: id,
        charId: char_id,
        str: "AAAAAA==".into(),
    })
}

fn apply(e: &mut Engine, at: u64, p: Packet) -> Vec<EngineEvent> {
    connection(e, 1, at, p)
}

fn connection(e: &mut Engine, conn: u64, at: u64, p: Packet) -> Vec<EngineEvent> {
    e.apply(DecodedPacket {
        conn: ConnId(conn),
        dir: Direction::ServerToClient,
        at: Timestamp(at),
        id: 0,
        len: 0,
        alignment_verified: true,
        packet: p,
    })
}

fn player() -> ObjectData {
    object(
        1,
        100,
        vec![
            string(31, "SynthAlpha"),
            string(38, "synthetic-account"),
            stat(7, 20),
            stat(20, 25),
            stat(29, 0),
            stat(96, 0),
            stat(113, 1000),
            string(128, ""),
            string(155, ""),
            string(80, ""),
            stat(8, 200),
            stat(9, 201),
            stat(27, 60),
            stat(57, 100),
        ],
    )
}

fn target() -> ObjectData {
    object(
        2,
        300,
        vec![stat(0, 10000), stat(21, 0), stat(29, 0), stat(96, 0)],
    )
}

fn setup(e: &mut Engine) {
    apply(e, 0, map("The Void"));
    apply(e, 1000, create(1, 1));
    apply(e, 2000, update(vec![player(), target()], vec![]));
}

fn damage(owner: i32, bullet: u16, n: u16) -> Packet {
    Packet::Damage(Damage {
        targetId: 2,
        effects: vec![],
        damageAmount: n,
        damageProperties: false,
        bulletId: bullet,
        objectId: owner,
    })
}

fn victory() -> Packet {
    Packet::Notification(Notification {
        data: NotificationData {
            effect: Some("Victory"),
            ..Default::default()
        },
    })
}

// These comparisons omit additive facts to guard the existing domain values
// and timing.
fn legacy_fight(f: &mut FightView) {
    f.kill = None;
    f.target_hp = None;
    f.target_kind = None;
    f.is_current_quest = false;
    f.local_last_hit = None;
    f.player_series = None;
    if f.end_reason == Some(FightEnd::Killed) {
        f.end_reason = Some(FightEnd::MapChanged);
    }
}
fn legacy_snapshot(e: &Engine) -> LiveSnapshot {
    let mut s = e.snapshot();
    s.instance = None;
    s.run_totals = None;
    for f in &mut s.fights {
        legacy_fight(f)
    }
    s
}
fn legacy_events(events: Vec<EngineEvent>) -> Vec<EngineEvent> {
    events
        .into_iter()
        .filter_map(|mut e| {
            if matches!(
                &e,
                EngineEvent::InstanceEvent(_)
                    | EngineEvent::Chat(_)
                    | EngineEvent::FightUpdated(FightView { kill: Some(_), .. })
            ) {
                return None;
            }
            match &mut e {
                EngineEvent::FightStarted(f)
                | EngineEvent::FightUpdated(f)
                | EngineEvent::FightEnded(f) => legacy_fight(f),
                _ => {}
            }
            Some(e)
        })
        .collect()
}

mod profiling_tests {
    include!("profiling_tests.rs");
}

fn assert_cached_projection_matches_reconstruction(e: &Engine) {
    let snapshot = e.snapshot();
    let selected = e.snapshot_series_selection();
    let expected: Vec<_> = e
        .fights
        .iter()
        .map(|(&key, f)| {
            let mut view = f.project_uncached(true);
            if !selected.contains(key) {
                view.player_series = None;
            }
            view
        })
        .collect();
    assert_eq!(snapshot.fights, expected);
    assert_eq!(
        snapshot.run_totals,
        e.run
            .as_ref()
            .map(|r| crate::engine::totals::run_totals_iter(
                expected
                    .iter()
                    .filter(|f| f.run_id.as_ref() == Some(&r.view.id)),
                None
            ))
    );
}
#[test]
fn projection_caches_reuse_unchanged_fights_and_totals() {
    use std::sync::atomic::Ordering;
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, tick(vec![]));
    apply(&mut e, 4000, damage(1, 1, 100));
    apply(&mut e, 4100, update(vec![object(4, 300, vec![])], vec![]));
    let Packet::Damage(mut packet) = damage(1, 2, 100) else {
        unreachable!()
    };
    packet.targetId = 4;
    apply(&mut e, 4200, Packet::Damage(packet));
    assert_cached_projection_matches_reconstruction(&e);
    let builds: Vec<_> = e
        .fights
        .values()
        .map(|f| f.projection_builds.load(Ordering::Relaxed))
        .collect();
    let totals_builds = e.run_totals_cache.lock().unwrap().rebuilds;
    let contributions = e.run_totals_cache.lock().unwrap().contribution_builds;
    let lookups = e.run_totals_cache.lock().unwrap().fight_lookups;
    for _ in 0..5 {
        assert_cached_projection_matches_reconstruction(&e);
    }
    e.set_watermark(Sequence(Timestamp(77)));
    assert_cached_projection_matches_reconstruction(&e);
    assert_eq!(
        builds,
        e.fights
            .values()
            .map(|f| f.projection_builds.load(Ordering::Relaxed))
            .collect::<Vec<_>>()
    );
    assert_eq!(totals_builds, e.run_totals_cache.lock().unwrap().rebuilds);
    assert_eq!(lookups, e.run_totals_cache.lock().unwrap().fight_lookups);
    let detached = e.snapshot();
    apply(&mut e, 5000, damage(1, 3, 80));
    assert_eq!(e.run_totals_cache.lock().unwrap().pending_count(), 1);
    assert_cached_projection_matches_reconstruction(&e);
    let after: Vec<_> = e
        .fights
        .values()
        .map(|f| f.projection_builds.load(Ordering::Relaxed))
        .collect();
    assert_eq!(after, vec![builds[0] + 1, builds[1]]);
    assert_eq!(
        e.run_totals_cache.lock().unwrap().rebuilds,
        totals_builds + 1
    );
    assert_eq!(
        e.run_totals_cache.lock().unwrap().contribution_builds,
        contributions + 1
    );
    assert_eq!(
        e.run_totals_cache.lock().unwrap().fight_lookups,
        lookups + 1
    );
    assert_eq!(detached.fights[0].total_damage, 100.);
    assert_eq!(e.snapshot().fights[0].total_damage, 180.);
}
#[test]
fn projection_caches_follow_metadata_kills_deaths_and_pair_settlement() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, tick(vec![]));
    apply(&mut e, 4000, damage(1, 1, 100));
    assert_cached_projection_matches_reconstruction(&e);
    for (at, packet) in [
        (4100, tick(vec![status(2, vec![stat(1, 125)])])),
        (
            4200,
            update(vec![object(3, 100, vec![string(31, "SynthBravo")])], vec![]),
        ),
        (4300, damage(3, 2, 40)),
        (
            4400,
            notice(
                "PlayerDeath",
                r#"{"k":"s.death","t":{"player":"SynthBravo","level":"20","enemy":"Synthetic Boss"}}"#,
            ),
        ),
        (4500, quest(2)),
        (4600, tick(vec![status(2, vec![stat(1, 0)])])),
        (4700, update(vec![], vec![2])),
    ] {
        apply(&mut e, at, packet);
        assert_cached_projection_matches_reconstruction(&e);
    }
    assert_eq!(e.snapshot().run_totals.unwrap().kills, 1);
    assert_eq!(
        e.snapshot().fights[0]
            .players
            .iter()
            .find(|p| p.object_id == 3)
            .unwrap()
            .deaths,
        1
    );
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(5000),
        maxed_protocol::Diagnostic::Recovering,
    );
    assert_cached_projection_matches_reconstruction(&e);
    e.advance_to(Timestamp(6_000_000));
    assert_cached_projection_matches_reconstruction(&e);
    apply(&mut e, 7_000_000, map("Realm"));
    assert_cached_projection_matches_reconstruction(&e);
    apply(&mut e, 8_000_000, map("The Void"));
    assert_cached_projection_matches_reconstruction(&e);
    assert_eq!(e.snapshot().run_totals.unwrap().fights, 0);
}

#[test]
fn remote_death_does_not_inject_row_for_unsettled_estimate() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(vec![object(3, 100, vec![string(31, "SynthBravo")])], vec![]),
    );
    e.report(2, 3, 10, false, Ok(50), &mut Vec::new());
    assert!(e.snapshot().fights[0]
        .players
        .iter()
        .any(|p| p.object_id == 3));
    apply(
        &mut e,
        4000,
        notice(
            "PlayerDeath",
            r#"{"k":"s.death","t":{"player":"SynthBravo","level":"20","enemy":"Synthetic Boss"}}"#,
        ),
    );
    assert_eq!(
        e.snapshot().fights[0]
            .players
            .iter()
            .find(|p| p.object_id == 3)
            .unwrap()
            .deaths,
        0
    );
    // A duplicate estimate makes this pair ambiguous, then expiry discards it.
    e.report(2, 3, 10, false, Ok(50), &mut Vec::new());
    e.advance_to(Timestamp(6_000_000));
    assert!(e.fights.values().all(|f| f.pending.is_empty()));
    assert!(!e.snapshot().fights[0]
        .players
        .iter()
        .any(|p| p.object_id == 3));
    assert!(!e
        .snapshot()
        .run_totals
        .unwrap()
        .players
        .iter()
        .any(|p| p.name.as_deref() == Some("SynthBravo")));
}
#[test]
fn totals_incremental_replacement_removal_and_metadata_match_reducer() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 3100, update(vec![object(4, 300, vec![])], vec![]));
    e.report(4, 1, 2, true, Ok(50), &mut Vec::new());
    assert_cached_projection_matches_reconstruction(&e);
    let keys: Vec<_> = e.fights.keys().copied().collect();
    for (i, key) in keys.iter().enumerate() {
        let f = e.fights.get_mut(key).unwrap();
        f.base.players[0].name = Some(format!("Synthetic {i}"));
        f.base.players[0].class_id = Some(i as u16);
        f.base.players[0].hits = u32::MAX;
        f.base.players[0].deaths = u32::MAX;
        f.changed();
    }
    assert_cached_projection_matches_reconstruction(&e);
    let first = e.fights.get_mut(&keys[0]).unwrap();
    first.base.players[0].name = Some("Updated first".into());
    first.base.players[0].is_local = false;
    first.changed();
    assert_cached_projection_matches_reconstruction(&e);
    e.fights.remove(&keys[1]);
    assert_cached_projection_matches_reconstruction(&e);
    assert_eq!(
        e.snapshot().run_totals.unwrap().players[0].name.as_deref(),
        Some("Updated first")
    );
    e.fights.remove(&keys[0]);
    assert_cached_projection_matches_reconstruction(&e);
}

#[test]
fn totals_dirty_queue_is_bounded_when_uncached_fights_retire() {
    let mut e = engine();
    setup(&mut e);
    for id in 100..200 {
        apply(&mut e, 3000, update(vec![object(id, 300, vec![])], vec![]));
        e.report(id, 1, 1, true, Ok(1), &mut Vec::new());
        let key = (id, e.entities[&id].generation);
        assert_eq!(e.run_totals_cache.lock().unwrap().pending_count(), 1);
        e.retire_fight(key, FightEnd::ResourceLimit, &mut Vec::new());
        assert_eq!(e.run_totals_cache.lock().unwrap().pending_count(), 0);
    }
    assert_cached_projection_matches_reconstruction(&e);
}

#[test]
fn incremental_rank_index_keeps_full_set_ties_and_local_after_reordering() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            (3..78)
                .map(|id| object(id, 100, vec![string(31, "Synthetic")]))
                .collect(),
            vec![],
        ),
    );
    for id in 3..78 {
        e.report(2, id, id as u16, true, Ok(id), &mut Vec::new());
    }
    assert_cached_projection_matches_reconstruction(&e);
    assert_eq!(e.snapshot().run_totals.unwrap().players.len(), 64);
    e.report(2, 3, 99, true, Ok(74), &mut Vec::new()); // tie with the prior leader
    assert_cached_projection_matches_reconstruction(&e);
    e.report(2, 1, 100, true, Ok(100), &mut Vec::new()); // local moves to first
    assert_cached_projection_matches_reconstruction(&e);
    let totals = e.snapshot().run_totals.unwrap();
    assert_eq!(totals.local.unwrap().rank, Some(1));
    assert_eq!(
        totals.players.iter().filter(|p| p.rank == Some(2)).count(),
        2
    );
}

fn bounded_series_fixture(reverse: bool) -> Engine {
    let mut cfg = config();
    cfg.max_active_fights = 2048;
    let mut e = Engine::new(cfg, Arc::new(catalog())).unwrap();
    setup(&mut e);
    let mut ids: Vec<_> = (100..2148).collect();
    if reverse {
        ids.reverse();
    }
    let mut objects: Vec<_> = ids
        .iter()
        .map(|&id| object(id, if id == 100 { 300 } else { 301 }, vec![]))
        .collect();
    objects.push(object(3, 100, vec![string(31, "Synthetic remote")]));
    apply(&mut e, 3000, update(objects, vec![]));
    apply(&mut e, 3500, tick(vec![]));
    for &id in &ids {
        e.report(id, 3, 1, true, Ok(1), &mut Vec::new());
    }
    apply(&mut e, 4000, tick(vec![]));
    e.report(2147, 1, 1, true, Ok(1), &mut Vec::new());
    apply(&mut e, 5000, tick(vec![]));
    for &id in ids
        .iter()
        .filter(|&&id| id != 100 && id != 101 && id != 2147)
    {
        e.report(id, 3, 2, true, Ok(1), &mut Vec::new());
    }
    apply(&mut e, 6000, quest(101));
    e
}
#[test]
fn live_series_prioritizes_all_boss_tiers_before_newer_visible_ordinary_hits() {
    let mut e = bounded_series_fixture(false);
    apply(&mut e, 7000, tick(vec![]));
    for id in 103..123 {
        e.report(id, 1, 3, true, Ok(1), &mut Vec::new());
    }
    // The most-recent ordinary local hit remains eligible after boss priorities.
    apply(&mut e, 8000, tick(vec![]));
    e.report(122, 1, 4, true, Ok(1), &mut Vec::new());
    for fight in e.fights.values_mut() {
        match fight.base.target_id {
            100 => fight.base.target_kind = Some(TargetKind::EventBoss),
            101 => {
                fight.base.target_kind = Some(TargetKind::Miniboss);
                fight.base.is_current_quest = false;
            }
            102 => fight.base.is_current_quest = true,
            _ => {}
        }
        fight.changed();
    }
    let domain = e.snapshot();
    let live = e.live_snapshot();
    for id in [100, 101, 102, 122] {
        let fight = live
            .fights
            .iter()
            .find(|fight| fight.target_id == id)
            .unwrap_or_else(|| panic!("missing priority target {id}"));
        assert!(
            fight.player_series.is_some(),
            "priority target {id} must retain series"
        );
    }
    assert_eq!(
        live.fights
            .iter()
            .filter(|fight| fight.player_series.is_some())
            .count(),
        crate::SNAPSHOT_SERIES_LIMIT
    );
    assert_eq!(e.snapshot(), domain, "domain selection must stay unchanged");
}

#[test]
fn live_series_keeps_local_boss_focus_ahead_of_newer_ordinary_and_remote_hits() {
    let mut e = bounded_series_fixture(false);
    apply(&mut e, 7000, tick(vec![]));
    e.report(100, 1, 3, true, Ok(1), &mut Vec::new());
    apply(&mut e, 8000, tick(vec![]));
    for id in 101..121 {
        e.report(id, 3, 3, true, Ok(1), &mut Vec::new());
    }
    apply(&mut e, 9000, tick(vec![]));
    e.report(130, 1, 4, true, Ok(1), &mut Vec::new());
    for fight in e
        .fights
        .values_mut()
        .filter(|fight| (100..121).contains(&fight.base.target_id))
    {
        fight.base.target_kind = Some(TargetKind::EventBoss);
        fight.changed();
    }
    let domain = e.snapshot();
    let live = e.live_snapshot();
    assert!(
        live.fights
            .iter()
            .find(|fight| fight.target_id == 100)
            .unwrap()
            .player_series
            .is_some(),
        "the last boss hit locally must retain focus series"
    );
    assert!(
        live.fights
            .iter()
            .find(|fight| fight.target_id == 130)
            .unwrap()
            .player_series
            .is_none(),
        "ordinary hits must not displace a visible boss tier"
    );
    assert_eq!(
        live.fights
            .iter()
            .filter(|fight| fight.player_series.is_some())
            .count(),
        crate::SNAPSHOT_SERIES_LIMIT
    );
    assert_eq!(e.snapshot(), domain);
}

#[test]
fn live_series_slots_ignore_hidden_fights_and_leave_domain_selection_unchanged() {
    let mut e = bounded_series_fixture(false);
    for fight in e.fights.values_mut() {
        if fight.base.target_id == 100 {
            fight.base.target_kind = Some(maxed_types::TargetKind::EventBoss);
        } else if fight.base.target_id == 101 {
            fight.base.target_kind = Some(maxed_types::TargetKind::Miniboss);
        }
    }
    // The locally hit ordinary row has expired from live visibility. Newer
    // remote hits on thousands of hidden ordinary rows must not consume slots.
    e.now = Timestamp(e.now.0 + 10_000_000);
    let domain = e.snapshot();
    let live = e.live_snapshot();
    assert_eq!(live.fights.len(), 2);
    assert!(live
        .fights
        .iter()
        .all(|fight| fight.player_series.is_some()));
    assert_eq!(
        e.snapshot(),
        domain,
        "live selection cannot change the domain cache"
    );
}

#[test]
fn live_payload_is_bounded_without_losing_focus_or_dungeon_totals() {
    for count in [100, 2048] {
        let mut e = bounded_series_fixture(false);
        let omitted: Vec<_> = e
            .fights
            .keys()
            .copied()
            .filter(|key| ![100, 101, 2147].contains(&key.0))
            .skip(count - 3)
            .collect();
        for key in omitted {
            e.fights.remove(&key);
        }
        let full = e.snapshot();
        let live = e.live_snapshot();
        println!(
            "full={} live={} bytes_full={} bytes_live={}",
            full.fights.len(),
            live.fights.len(),
            serde_json::to_vec(&full).unwrap().len(),
            serde_json::to_vec(&live).unwrap().len()
        );
        assert_eq!(
            live.fights.len(),
            3,
            "ordinary open fights must not flood presentation"
        );
        for target in [100, 101, 2147] {
            assert!(live.fights.iter().any(|f| f.target_id == target));
        }
        assert_eq!(live.run_totals, full.run_totals);
        assert_eq!(e.fights.len(), count, "storage retains ordinary fights");
        e.now = Timestamp(e.now.0 + 10_000_000);
        assert_eq!(
            e.live_snapshot().fights.len(),
            2,
            "idle ordinary local hit leaves the live payload"
        );
    }
}

#[test]
fn live_payload_omits_nonparticipating_local_row_but_keeps_known_zero_buckets() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![object(3, 100, vec![string(31, "Synthetic remote")])],
            vec![],
        ),
    );
    e.report(2, 3, 1, true, Ok(100), &mut Vec::new());
    let fight = e.live_snapshot().fights.remove(0);
    assert!(!fight.players.iter().any(|p| p.is_local));
    assert!(fight.buckets.iter().all(|b| b.local == Some(0.)));
    e.report(2, 1, 2, true, Ok(50), &mut Vec::new());
    let fight = e.live_snapshot().fights.remove(0);
    assert_eq!(
        fight.players.iter().find(|p| p.is_local).unwrap().damage,
        50.
    );
}

#[test]
fn live_payload_bounds_chart_resolution_without_changing_damage_or_dps() {
    let mut e = engine();
    setup(&mut e);
    for second in 1..=360 {
        apply(&mut e, second * 1_000_000, tick(vec![]));
        e.report(2, 1, second as u16, true, Ok(100), &mut Vec::new());
    }
    let domain = e.snapshot().fights.remove(0);
    let live = e.live_snapshot().fights.remove(0);
    assert!(domain.buckets.len() > 120);
    assert!(live.buckets.len() <= 120);
    assert_eq!(live.players, domain.players);
    assert_eq!(
        live.buckets.iter().map(|b| b.total).sum::<f64>(),
        domain.total_damage
    );
    for series in live.player_series.unwrap() {
        assert_eq!(series.points.len(), live.buckets.len());
        let original = domain
            .player_series
            .as_ref()
            .unwrap()
            .iter()
            .find(|p| p.player_id == series.player_id)
            .unwrap();
        assert_eq!(
            series.points.iter().sum::<f64>(),
            original.points.iter().sum::<f64>()
        );
    }
}

fn intermittent_live_chart(lengths: &[usize]) -> FightView {
    let mut e = engine();
    setup(&mut e);
    e.report(2, 1, 1, true, Ok(100), &mut Vec::new());
    let mut fight = e.snapshot().fights.remove(0);
    fight.buckets.clear();
    let mut at = 0;
    for (run, &length) in lengths.iter().enumerate() {
        let rate = (run + 1) as f64 * 100.;
        for i in 0..length {
            let seconds = (i % 2 + 1) as u64;
            let until = at + seconds * 1_000_000;
            fight.buckets.push(DamageBucket {
                from: Timestamp(at),
                until: Timestamp(until),
                total: rate * seconds as f64,
                local: Some(rate * seconds as f64 / 2.),
                others_damage: None,
                others_presence: None,
            });
            at = until;
        }
        at += 20_000_000;
    }
    fight.player_series = Some(vec![maxed_types::PlayerSeries {
        player_id: "synthetic-series".into(),
        object_id: 3,
        is_local: false,
        points: fight
            .buckets
            .iter()
            .map(|bucket| bucket.total / 4.)
            .collect(),
    }]);
    fight
}

fn assert_live_chart_runs_preserved(lengths: &[usize]) {
    let original = intermittent_live_chart(lengths);
    let mut live = original.clone();
    crate::compact_live_fight(&mut live);
    assert!(live.buckets.len() <= 120.max(lengths.len()));
    let points = &live.player_series.as_ref().unwrap()[0].points;
    assert_eq!(points.len(), live.buckets.len());
    assert_eq!(
        live.buckets.iter().map(|bucket| bucket.total).sum::<f64>(),
        original
            .buckets
            .iter()
            .map(|bucket| bucket.total)
            .sum::<f64>()
    );
    let mut offset = 0;
    let mut accounted = 0;
    for (run, &length) in lengths.iter().enumerate() {
        let source = &original.buckets[offset..offset + length];
        let from = source[0].from;
        let until = source.last().unwrap().until;
        let rows: Vec<_> = live
            .buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| bucket.from >= from && bucket.until <= until)
            .collect();
        assert!(!rows.is_empty());
        assert_eq!(rows[0].1.from, from);
        assert_eq!(
            rows.last().unwrap().1.until,
            until,
            "run endpoint cannot cross an observation gap"
        );
        assert_eq!(
            rows.iter().map(|(_, bucket)| bucket.total).sum::<f64>(),
            source.iter().map(|bucket| bucket.total).sum::<f64>()
        );
        let rate = (run + 1) as f64 * 100.;
        for (index, bucket) in &rows {
            let seconds = (bucket.until.0 - bucket.from.0) as f64 / 1_000_000.;
            assert_eq!(bucket.total / seconds, rate);
            assert_eq!(bucket.local.unwrap() / seconds, rate / 2.);
            assert_eq!(points[*index] / seconds, rate / 4.);
        }
        accounted += rows.len();
        offset += length;
    }
    assert_eq!(
        accounted,
        live.buckets.len(),
        "every output bucket belongs to exactly one observed run"
    );
}

#[test]
fn live_chart_compaction_preserves_intermittent_rates_sums_and_endpoints() {
    assert_live_chart_runs_preserved(&[61, 61, 61]);
}

#[test]
fn live_chart_compaction_has_a_gap_preserving_bound_for_many_runs() {
    assert_live_chart_runs_preserved(&vec![3; 600]);
    assert_live_chart_runs_preserved(&vec![1; 1800]);
    assert_live_chart_runs_preserved(&[1200, 1, 599]);
}

#[test]
fn snapshot_series_cap_prioritizes_and_keeps_full_event_series() {
    use crate::SNAPSHOT_SERIES_LIMIT;
    let mut e = bounded_series_fixture(false);
    let snapshot = e.snapshot();
    assert_eq!(snapshot.fights.len(), 2048);
    assert!(snapshot.fights.iter().all(|f| f.ended.is_none()));
    let selected: std::collections::BTreeSet<_> = snapshot
        .fights
        .iter()
        .filter(|f| f.player_series.is_some())
        .map(|f| f.id.clone())
        .collect();
    assert_eq!(selected.len(), SNAPSHOT_SERIES_LIMIT);
    for target in [100, 101, 2147] {
        assert!(snapshot
            .fights
            .iter()
            .find(|f| f.target_id == target)
            .unwrap()
            .player_series
            .is_some());
    }
    assert_eq!(
        snapshot
            .fights
            .iter()
            .find(|f| f.target_id == 100)
            .unwrap()
            .target_kind,
        Some(TargetKind::Boss)
    );
    assert_eq!(
        snapshot
            .fights
            .iter()
            .find(|f| f.target_id == 101)
            .unwrap()
            .target_kind,
        Some(TargetKind::Quest)
    );
    let mut expected: Vec<_> = snapshot.fights.iter().collect();
    expected.sort_by(|a, b| {
        let tier = |f: &FightView| {
            if matches!(f.target_kind, Some(TargetKind::Boss | TargetKind::Quest)) {
                0
            } else if f.target_id == 2147 {
                1
            } else {
                2
            }
        };
        tier(a)
            .cmp(&tier(b))
            .then_with(|| b.last_hit.cmp(&a.last_hit))
            .then_with(|| a.id.cmp(&b.id))
    });
    assert_eq!(
        selected,
        expected
            .into_iter()
            .take(SNAPSHOT_SERIES_LIMIT)
            .map(|f| f.id.clone())
            .collect()
    );
    let reversed = bounded_series_fixture(true).snapshot();
    assert_eq!(
        selected,
        reversed
            .fights
            .iter()
            .filter(|f| f.player_series.is_some())
            .map(|f| f.id.clone())
            .collect()
    );
    let lookups = e.snapshot_series_cache.lock().unwrap().fight_lookups;
    assert_eq!(e.snapshot(), snapshot);
    assert_eq!(
        e.snapshot_series_cache.lock().unwrap().fight_lookups,
        lookups
    );
    let hidden = snapshot
        .fights
        .iter()
        .find(|f| f.player_series.is_none())
        .unwrap();
    let key = (hidden.target_id, hidden.target_generation);
    let full = e.fights[&key].project().player_series;
    assert!(full.is_some());
    apply(&mut e, 7000, tick(vec![status(key.0, vec![stat(1, 123)])]));
    assert!(e
        .snapshot()
        .fights
        .iter()
        .find(|f| f.id == hidden.id)
        .unwrap()
        .player_series
        .is_none());
    assert_eq!(
        e.snapshot_series_cache.lock().unwrap().fight_lookups,
        lookups + 1
    );
    let events = e.advance_to(Timestamp(300_000));
    let updated = events
        .iter()
        .find_map(|event| match event {
            EngineEvent::FightUpdated(f) if f.id == hidden.id => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(updated.player_series, full);
    let events = e.finish(Timestamp(400_000), FightEnd::CaptureStopped);
    let ended: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::FightEnded(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(ended.len(), 2048);
    assert!(ended
        .iter()
        .all(|f| f.player_series.is_some() && f.ended.is_some()));
    assert_eq!(
        ended
            .iter()
            .find(|f| f.id == hidden.id)
            .unwrap()
            .player_series,
        full
    );
}

fn small_series_priority_fixture(boss: bool) -> Engine {
    let mut e = engine();
    setup(&mut e);
    let mut objects: Vec<_> = (100..118)
        .map(|id| object(id, if boss { 300 } else { 301 }, vec![]))
        .collect();
    objects.push(object(3, 100, vec![string(31, "Synthetic remote")]));
    apply(&mut e, 3000, update(objects, vec![]));
    for id in 100..118 {
        e.report(id, 3, 1, true, Ok(1), &mut Vec::new());
    }
    e
}
#[test]
fn snapshot_series_priority_overflow_and_ended_exclusion() {
    let mut e = small_series_priority_fixture(true);
    e.report(117, 1, 1, true, Ok(1), &mut Vec::new());
    let s = e.snapshot();
    assert_eq!(
        s.fights
            .iter()
            .filter(|f| f.player_series.is_some())
            .count(),
        16
    );
    assert!(s
        .fights
        .iter()
        .find(|f| f.target_id == 117)
        .unwrap()
        .player_series
        .is_none());
    // Future retention of ended views must not make them series candidates.
    let key = (100, e.entities[&100].generation);
    let f = e.fights.get_mut(&key).unwrap();
    f.base.ended = Some(Timestamp(4000));
    f.changed();
    let key = (101, e.entities[&101].generation);
    let f = e.fights.get_mut(&key).unwrap();
    f.base.target_kind = Some(TargetKind::EventBoss);
    f.changed();
    let s = e.snapshot();
    assert_eq!(
        s.fights
            .iter()
            .filter(|f| f.player_series.is_some())
            .count(),
        16
    );
    for id in [100, 101] {
        assert!(s
            .fights
            .iter()
            .find(|f| f.target_id == id)
            .unwrap()
            .player_series
            .is_none());
    }
    let f = e.fights.get_mut(&key).unwrap();
    f.base.target_kind = Some(TargetKind::Quest);
    f.changed();
    let s = e.snapshot();
    assert!(s
        .fights
        .iter()
        .find(|f| f.target_id == 101)
        .unwrap()
        .player_series
        .is_some());
    assert!(s
        .fights
        .iter()
        .find(|f| f.target_id == 117)
        .unwrap()
        .player_series
        .is_none());
}
#[test]
fn snapshot_series_local_hit_ties_and_retirement_update_selection() {
    let mut e = small_series_priority_fixture(false);
    apply(&mut e, 4000, tick(vec![]));
    for id in [116, 115] {
        e.report(id, 1, 1, true, Ok(1), &mut Vec::new());
    }
    apply(&mut e, 5000, tick(vec![]));
    for id in (100..118).filter(|id| *id != 115 && *id != 116) {
        e.report(id, 3, 2, true, Ok(1), &mut Vec::new());
    }
    let s = e.snapshot();
    // Ascending fight ID wins the equal local-hit timestamps, even though
    // every remote-only candidate was hit more recently.
    assert!(s
        .fights
        .iter()
        .find(|f| f.target_id == 115)
        .unwrap()
        .player_series
        .is_some());
    assert!(s
        .fights
        .iter()
        .find(|f| f.target_id == 116)
        .unwrap()
        .player_series
        .is_none());
    let key = (115, e.entities[&115].generation);
    e.retire_fight(key, FightEnd::MapChanged, &mut Vec::new());
    let s = e.snapshot();
    assert_eq!(
        s.fights
            .iter()
            .filter(|f| f.player_series.is_some())
            .count(),
        16
    );
    assert!(s
        .fights
        .iter()
        .find(|f| f.target_id == 116)
        .unwrap()
        .player_series
        .is_some());
    let key = (117, e.entities[&117].generation);
    apply(&mut e, 6000, tick(vec![]));
    e.report(117, 1, 9, false, Ok(1), &mut Vec::new());
    assert_eq!(e.fights[&key].latest_local_hit(), Some(Timestamp(6000)));
    e.snapshot(); // cache the projected unpaired estimate's priority
    e.report(117, 1, 9, false, Ok(1), &mut Vec::new());
    assert_eq!(e.fights[&key].latest_local_hit(), None);
    assert!(e
        .snapshot()
        .fights
        .iter()
        .find(|f| f.target_id == 116)
        .unwrap()
        .player_series
        .is_some());
}

#[test]
fn record_packet_ids_match_consumed_reducer_variants() {
    use std::collections::{BTreeMap, BTreeSet};

    // Read the wire registry and actual reducer arms, so a new consumed variant
    // cannot silently drift from coverage classification. No packet bodies needed.
    let registry = include_str!("../../../maxed-protocol/src/defs.rs")
        .split_once("define_packets! {")
        .unwrap()
        .1
        .split_once("\n}")
        .unwrap()
        .0;
    let ids: BTreeMap<_, _> = registry
        .lines()
        .filter_map(|line| {
            let (id, rest) = line.trim().split_once(" => ")?;
            Some((rest.split_once(',')?.0, id.parse::<u8>().ok()?))
        })
        .collect();
    let world = include_str!("../engine.rs")
        .split_once("match packet.packet {")
        .unwrap()
        .1
        .split_once("self.publish_notices")
        .unwrap()
        .0;
    let combat = include_str!("../combat.rs")
        .split_once("pub(crate) fn apply_combat(")
        .unwrap()
        .1
        .split_once("fn weapon_multiplier")
        .unwrap()
        .0;
    let consumed: BTreeSet<_> = [world, combat]
        .into_iter()
        .flat_map(|source| {
            source
                .split("Packet::")
                .skip(1)
                .map(|arm| {
                    arm.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .next()
                        .unwrap()
                })
                .filter(|name| *name != "Unknown")
        })
        .map(|variant| {
            *ids.get(variant)
                .expect("consumed packet must have a wire ID")
        })
        .collect();
    let classified: BTreeSet<_> = (0..=u8::MAX)
        .filter(|id| super::record_packet(*id))
        .collect();
    assert_eq!(classified, consumed);
}

fn chat_line(name: &str, recipient: &str, line: &str) -> Packet {
    Packet::Text(Text {
        name: name.into(),
        objectId: -1,
        numStars: 70,
        starBackground: 0,
        bubbleTime: 0,
        recipient: recipient.into(),
        text: line.into(),
        cleanText: String::new(),
        isSupporter: true,
    })
}

fn chats(events: &[EngineEvent]) -> Vec<&ChatMessageView> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Chat(m) => Some(m),
            _ => None,
        })
        .collect()
}

#[test]
fn every_text_packet_emits_one_chat_event() {
    let mut e = engine();
    setup(&mut e);
    let events = apply(&mut e, 3000, chat_line("SynthAlpha,1", "*Guild*", "hello"));
    let chat = chats(&events);
    assert_eq!(chat.len(), 1);
    let m = chat[0];
    assert!(m.id.ends_with(":chat:000000000001"));
    assert_eq!(m.channel, ChatChannel::Guild);
    assert_eq!(m.sender, "SynthAlpha");
    assert_eq!(m.text, "hello");
    assert_eq!(m.stars, Some(70));
    assert!(m.supporter);
    assert_eq!(m.map.as_deref(), Some("The Void"));
    let events = apply(&mut e, 3100, chat_line("#Oryx the Mad God", "", "line"));
    assert!(chats(&events)[0].id.ends_with(":chat:000000000002"));
}

#[test]
fn negative_stars_are_absent() {
    let mut e = engine();
    setup(&mut e);
    let mut packet = chat_line("SynthAlpha", "", "x");
    if let Packet::Text(t) = &mut packet {
        t.numStars = -1;
    }
    assert_eq!(chats(&apply(&mut e, 3000, packet))[0].stars, None);
}

#[test]
fn existing_text_rules_still_run_alongside_chat() {
    // The Oryx realm-kill notice still publishes its instance event.
    let mut e = engine();
    setup(&mut e);
    let events = apply(&mut e, 3000, text("#Oryx the Mad God", ""));
    assert_eq!(chats(&events).len(), 1);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, EngineEvent::InstanceEvent(_))));
}

#[test]
fn chat_alert_kind_rules() {
    use crate::alerts::chat_alert_kind;
    let mut e = engine();
    setup(&mut e);
    let m = chats(&apply(
        &mut e,
        3000,
        chat_line("SynthBravo", "*Party*", "x"),
    ))[0]
        .clone();
    assert_eq!(chat_alert_kind(&m), Some(AlertKind::PartyMessage));
    let npc = ChatMessageView {
        sender_kind: ChatSenderKind::Npc,
        ..m.clone()
    };
    assert_eq!(chat_alert_kind(&npc), None);
    let mine = ChatMessageView {
        is_local: true,
        ..m.clone()
    };
    assert_eq!(chat_alert_kind(&mine), None);
    let whisper_unknown = ChatMessageView {
        channel: ChatChannel::Whisper,
        direction: Some(WhisperDirection::Unknown),
        ..m.clone()
    };
    assert_eq!(chat_alert_kind(&whisper_unknown), None);
    let whisper_in = ChatMessageView {
        direction: Some(WhisperDirection::Incoming),
        ..whisper_unknown
    };
    assert_eq!(chat_alert_kind(&whisper_in), Some(AlertKind::Whisper));
    assert_eq!(
        crate::alerts::chat_excerpt(&"a".repeat(130))
            .chars()
            .count(),
        120
    );
    assert!(crate::alerts::chat_excerpt(&"a".repeat(130)).ends_with('…'));
}

#[test]
fn chat_alertable_requires_live_stream_and_known_local_name() {
    let mut replay = engine();
    setup(&mut replay);
    assert!(!replay.chat_alertable());
    let mut e = Engine::new(
        EngineConfig {
            stream_id: "live-synthetic".into(),
            ..config()
        },
        Arc::new(catalog()),
    )
    .unwrap();
    assert!(!e.chat_alertable());
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    assert!(!e.chat_alertable());
    apply(&mut e, 2000, update(vec![player()], vec![]));
    assert!(e.chat_alertable());
    let events = apply(
        &mut e,
        3000,
        chat_line("SynthBravo", "synthalpha,1", "hello"),
    );
    assert_eq!(
        chats(&events)[0].direction,
        Some(WhisperDirection::Incoming)
    );
    let events = apply(
        &mut e,
        4000,
        chat_line("SynthAlpha,2", "SynthBravo", "hello"),
    );
    assert_eq!(
        chats(&events)[0].direction,
        Some(WhisperDirection::Outgoing)
    );
    assert!(chats(&events)[0].is_local);
    apply(&mut e, 5000, tick(vec![status(1, vec![string(31, "")])]));
    assert!(!e.chat_alertable());
}

#[test]
fn chat_ids_sort_in_emission_order_at_the_same_timestamp() {
    let mut e = engine();
    setup(&mut e);
    let mut ids = Vec::new();
    for _ in 0..12 {
        let events = apply(&mut e, 3000, chat_line("SynthAlpha", "", "hello"));
        ids.push(chats(&events)[0].id.clone());
    }
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn chat_packet_names_and_map_are_bounded_at_utf8_boundaries() {
    let mut e = engine();
    setup(&mut e);
    let long = format!("SynthAlphaXY{}", "界".repeat(96));
    assert_eq!(long.len(), 300);
    e.map = Some(long.clone());
    let events = apply(&mut e, 3000, chat_line(&long, &long, "hello"));
    let m = chats(&events)[0];
    for field in [
        Some(m.sender.as_str()),
        m.counterpart.as_deref(),
        m.map.as_deref(),
    ] {
        let value = field.unwrap();
        assert!(value.len() <= 256);
        assert!(value.len() > 253);
        assert!(long.starts_with(value));
    }
    assert_eq!(m.text, "hello");
    assert!(!m.truncated, "truncated describes the text only");
}

#![allow(dead_code)]
use maxed_engine::{Engine, EngineConfig};
use maxed_protocol::{defs::*, DecodedPacket};
use maxed_types::*;
use std::sync::Arc;
pub fn catalog() -> Catalog {
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
        difficulty: None,
        exalt: false,
        exalt_gateway: false,
        category: None,
    }
}
pub fn config() -> EngineConfig {
    EngineConfig {
        stream_id: "test".into(),
        identity_key: [7; 32],
        ..Default::default()
    }
}
pub fn engine() -> Engine {
    Engine::new(config(), Arc::new(catalog())).unwrap()
}
pub fn pos() -> WorldPosData {
    WorldPosData { x: 0., y: 0. }
}
pub fn stat(id: u8, n: i32) -> StatData {
    StatData {
        statTypeNum: id,
        statType: None,
        statValue: n,
        stringStatValue: None,
        statValueTwo: 0,
    }
}
pub fn string(id: u8, v: &str) -> StatData {
    StatData {
        stringStatValue: Some(v.into()),
        ..stat(id, 0)
    }
}
pub fn status(id: i32, stats: Vec<StatData>) -> ObjectStatusData {
    ObjectStatusData {
        objectId: id,
        pos: pos(),
        stats,
    }
}
pub fn object(id: i32, ty: u16, stats: Vec<StatData>) -> ObjectData {
    ObjectData {
        objectType: ty,
        status: status(id, stats),
    }
}
pub fn update(objects: Vec<ObjectData>, drops: Vec<i32>) -> Packet {
    Packet::Update(Update {
        pos: pos(),
        levelType: 0,
        tiles: vec![],
        newObjects: objects,
        drops,
    })
}
pub fn tick(stats: Vec<ObjectStatusData>) -> Packet {
    Packet::NewTick(NewTick {
        tickId: 0,
        tickTime: 200,
        serverRealTimeMS: 0,
        serverLastTimeRTTMS: 0,
        status: stats,
    })
}
pub fn map(name: &str) -> Packet {
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
pub fn create(id: i32, char_id: i32) -> Packet {
    Packet::CreateSuccess(CreateSuccess {
        objectId: id,
        charId: char_id,
        str: "AAAAAA==".into(),
    })
}
pub fn apply(e: &mut Engine, at: u64, p: Packet) -> Vec<EngineEvent> {
    connection(e, 1, at, p)
}
pub fn connection(e: &mut Engine, conn: u64, at: u64, p: Packet) -> Vec<EngineEvent> {
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
pub fn player() -> ObjectData {
    object(
        1,
        100,
        vec![
            string(31, "Synthetic"),
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
pub fn target() -> ObjectData {
    object(
        2,
        300,
        vec![stat(0, 10000), stat(21, 0), stat(29, 0), stat(96, 0)],
    )
}
pub fn setup(e: &mut Engine) {
    apply(e, 0, map("The Void"));
    apply(e, 1000, create(1, 1));
    apply(e, 2000, update(vec![player(), target()], vec![]));
}
pub fn damage(owner: i32, bullet: u16, n: u16) -> Packet {
    Packet::Damage(Damage {
        targetId: 2,
        effects: vec![],
        damageAmount: n,
        damageProperties: false,
        bulletId: bullet,
        objectId: owner,
    })
}
pub fn shoot(bullet: i16) -> Packet {
    Packet::PlayerShoot(PlayerShoot {
        time: 0,
        bulletId: bullet,
        weaponId: 200,
        projectileId: -1,
        startingPos: pos(),
        angle: 0.,
        isBurst: false,
        patternIdx: 0,
        attackType: 0,
        playerPosition: pos(),
    })
}
pub fn hit(bullet: i16) -> Packet {
    Packet::EnemyHit(EnemyHit {
        time: 0,
        bulletId: bullet,
        shooterID: 1,
        targetId: 2,
        kill: false,
        mainID: 1,
    })
}
pub fn victory() -> Packet {
    Packet::Notification(Notification {
        data: NotificationData {
            effect: Some("Victory"),
            ..Default::default()
        },
    })
}
pub fn death() -> Packet {
    Packet::Death(Death {
        accountId: "synthetic-account".into(),
        charId: 1,
        killedBy: "Synthetic".into(),
        gravestoneType: 0,
        totalFame: 999,
        fameData: vec![],
        stats: String::new(),
    })
}

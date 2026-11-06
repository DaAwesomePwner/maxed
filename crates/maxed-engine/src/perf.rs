//! Invented packet workload, compiled only for tests or explicit perf builds.
use maxed_protocol::{defs::*, Packet};
use maxed_types::*;

pub const FIGHTS: usize = 1536;
pub const PLAYERS: usize = 24;
pub const HITS_PER_SECOND: usize = 24;

pub fn seed() -> Vec<Packet> {
    let mut objects: Vec<_> = (1..=PLAYERS as i32)
        .map(|id| {
            let mut row = player();
            row.status.objectId = id;
            row.status
                .stats
                .retain(|s| s.statTypeNum != 31 && s.statTypeNum != 38);
            row.status
                .stats
                .push(string(31, &format!("Synthetic {id}")));
            if id == 1 {
                row.status.stats.push(string(38, "synthetic-account"));
            }
            row
        })
        .collect();
    objects.extend((100..100 + FIGHTS as i32).map(|id| {
        object(
            id,
            if id < 104 { 300 } else { 301 },
            vec![stat(0, 10000000), stat(1, 10000000)],
        )
    }));
    let mut packets = vec![
        map("Realm"),
        create(1, 1),
        update(objects, vec![]),
        Packet::QuestObjId(QuestObjId {
            objectId: 104,
            list: vec![],
        }),
    ];
    packets.extend((10000..10320).step_by(16).map(|first| {
        update(
            (first..first + 16)
                .map(|id| object(id, 1292, vec![stat(8, -1)]))
                .collect(),
            vec![],
        )
    }));
    packets.extend((100..100 + FIGHTS as i32).map(|target| {
        Packet::Damage(Damage {
            targetId: target,
            effects: vec![],
            damageAmount: 100,
            damageProperties: false,
            bulletId: 0,
            objectId: 2 + target % 23,
        })
    }));
    packets
}
pub fn hits(step: u64) -> Vec<Packet> {
    let mut packets = vec![tick(vec![])];
    for i in 0..4 {
        let target = 100 + ((step * 4 + i) % FIGHTS as u64) as i32;
        packets.push(Packet::Damage(Damage {
            targetId: target,
            effects: vec![],
            damageAmount: 100,
            damageProperties: false,
            bulletId: step as u16,
            objectId: 2 + ((step * 4 + i) % 23) as i32,
        }));
    }
    // Local engagement on one boss, bystander hits on another.
    for (target, owner) in [(100, 1), (101, 2)] {
        packets.push(Packet::Damage(Damage {
            targetId: target,
            effects: vec![],
            damageAmount: 100,
            damageProperties: false,
            bulletId: step as u16,
            objectId: owner,
        }));
    }
    packets
}
pub fn leave() -> Packet {
    map("Nexus")
}
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
    }
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

fn player() -> ObjectData {
    object(
        1,
        100,
        vec![
            string(31, "Synthetic local"),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_packet_batches_leave_room_for_subscription_resync() {
        let mut engine =
            crate::Engine::new(Default::default(), std::sync::Arc::new(catalog())).unwrap();
        let largest = seed()
            .into_iter()
            .map(|packet| {
                engine
                    .apply(maxed_protocol::DecodedPacket {
                        conn: ConnId(1),
                        dir: Direction::ServerToClient,
                        at: Timestamp(1_000_000),
                        id: 0,
                        len: 0,
                        alignment_verified: true,
                        packet,
                    })
                    .len()
            })
            .max()
            .unwrap();
        assert!(largest <= 128, "one seed packet emitted {largest} events");
        assert_eq!(engine.snapshot().fights.len(), FIGHTS);
        assert_eq!(engine.snapshot().drops.len(), 320);
    }
}

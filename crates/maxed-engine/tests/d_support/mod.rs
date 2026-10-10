#![allow(dead_code)]
pub use crate::common::*;
use maxed_engine::Engine;
use maxed_protocol::defs::*;
use maxed_types::*;
pub fn quest(id: i32, list: Vec<i32>) -> Packet {
    Packet::QuestObjId(QuestObjId { objectId: id, list })
}
pub fn realm_score(score: i32) -> Packet {
    Packet::RealmScoreUpdate(RealmScoreUpdate { score })
}
pub fn map_ctx(
    name: &str,
    cap: i16,
    realm: &str,
    difficulty: f32,
    opened: i32,
    scores: Option<(i32, i32)>,
) -> Packet {
    let Packet::MapInfo(mut m) = map(name) else {
        unreachable!()
    };
    m.maxPlayerCount = cap;
    m.realmName = realm.into();
    m.difficulty = difficulty;
    m.gameOpenedTime = opened;
    m.has_scores = scores.is_some();
    (m.currentRealmScore, m.maxRealmScore) = scores.unwrap_or((-1, -1));
    Packet::MapInfo(m)
}
fn notice(effect: &'static str, message: String, picture: i32) -> Packet {
    Packet::Notification(Notification {
        data: NotificationData {
            effect: Some(effect),
            message: Some(message),
            pictureType: picture,
            ..Default::default()
        },
    })
}
pub fn player_death(name: &str, level: u16, enemy: &str, grave: i32) -> Packet {
    notice("PlayerDeath",serde_json::json!({"k":"s.death","t":{"player":name,"level":level.to_string(),"enemy":enemy}}).to_string(),grave)
}
pub fn portal_opened(opener: &str, portal_type: i32) -> Packet {
    notice(
        "PortalOpened",
        serde_json::json!({"k":"s.dungeon_opened_by","t":{"player":opener}}).to_string(),
        portal_type,
    )
}
pub fn oryx_killed(key: &str, killer: &str) -> Packet {
    Packet::Text(Text {
        name: "#Oryx the Mad God".into(),
        objectId: -1,
        numStars: 0,
        starBackground: 0,
        bubbleTime: 0,
        recipient: String::new(),
        text: serde_json::json!({"k":format!("stringlist.{key}.killed.0"),"t":{"KILLER":killer}})
            .to_string(),
        cleanText: String::new(),
        isSupporter: false,
    })
}
pub fn player_obj(id: i32, class: u16, name: &str) -> ObjectData {
    object(id, class, vec![string(31, name)])
}
pub fn started() -> Engine {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, tick(vec![]));
    apply(&mut e, 4000, damage(1, 1, 100));
    e
}
pub fn kill(e: &mut Engine) -> Vec<EngineEvent> {
    apply(e, 5000, tick(vec![status(2, vec![stat(1, 0)])]));
    apply(e, 6000, update(vec![], vec![2]))
}
pub fn ended(events: Vec<EngineEvent>) -> FightView {
    events
        .into_iter()
        .find_map(|e| {
            if let EngineEvent::FightEnded(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .unwrap()
}
pub fn instances(events: Vec<EngineEvent>) -> Vec<InstanceEventView> {
    events
        .into_iter()
        .filter_map(|e| {
            if let EngineEvent::InstanceEvent(v) = e {
                Some(v)
            } else {
                None
            }
        })
        .collect()
}
pub fn remote() -> Engine {
    let mut e = started();
    apply(
        &mut e,
        4100,
        update(vec![player_obj(3, 100, "SynthBravo,suffix")], vec![]),
    );
    apply(&mut e, 4200, damage(3, 2, 50));
    e
}

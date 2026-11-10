use maxed_protocol::{decode_packet, Packet};
use maxed_types::Direction::{self, ClientToServer as C, ServerToClient as S};
use serde_json::{json, Value};

#[test]
fn escape() {
    let (p, left) = decode_packet(105, C, &[]).unwrap();
    assert_eq!(p.name(), "ESCAPE");
    assert_eq!(p.fields().unwrap(), json!({}));
    assert_eq!(left, 0);
    assert!(matches!(
        decode_packet(250, S, &[1, 2, 3]).unwrap().0,
        Packet::Unknown { id: 250, len: 8 }
    ));
}

fn check(id: u8, dir: Direction, body: &[u8], fields: Value) {
    let (p, left) = decode_packet(id, dir, body).unwrap();
    assert_eq!(p.fields().unwrap(), fields);
    assert_eq!(left, 0);
    for end in 0..body.len() {
        assert!(
            decode_packet(id, dir, &body[..end]).is_err(),
            "id {id}, prefix {end}"
        );
    }
    let wrong = if dir == C { S } else { C };
    assert!(matches!(
        decode_packet(id, wrong, body).unwrap().0,
        Packet::Unknown { .. }
    ));
    let mut extended = body.to_vec();
    extended.push(0);
    assert_eq!(decode_packet(id, dir, &extended).unwrap().1, 1);
}

#[derive(Default)]
struct Wire(Vec<u8>);
impl Wire {
    fn raw(mut self, b: &[u8]) -> Self {
        self.0.extend_from_slice(b);
        self
    }
    fn byte(self, n: u8) -> Self {
        self.raw(&[n])
    }
    fn short(self, n: i16) -> Self {
        self.raw(&n.to_be_bytes())
    }
    fn int(self, n: i32) -> Self {
        self.raw(&n.to_be_bytes())
    }
    fn float(self, n: f32) -> Self {
        self.raw(&n.to_be_bytes())
    }
    fn pos(self) -> Self {
        self.float(1.25).float(-2.5)
    }
    fn string(self, s: &str) -> Self {
        self.short(s.len() as i16).raw(s.as_bytes())
    }
}
fn w() -> Wire {
    Wire::default()
}
fn pos() -> Value {
    json!({"x":1.25,"y":-2.5})
}
#[test]
fn player_hit() {
    check(
        90,
        C,
        &w().short(-1).int(-1).0,
        json!({"bulletId":-1,"objectId":-1}),
    );
}
#[test]
fn failure() {
    check(
        0,
        S,
        &w().int(4).string("").0,
        json!({"errorId":"IncorrectVersion","errorDescription":""}),
    );
    assert_eq!(
        decode_packet(0, S, &w().int(-1).string("").0)
            .unwrap()
            .0
            .fields()
            .unwrap()["errorId"],
        Value::Null
    );
}
#[test]
fn enemy_hit() {
    check(
        25,
        C,
        &w().int(-1).short(-1).int(-1).int(-1).byte(255).int(-1).0,
        json!({"time":-1,"bulletId":-1,"shooterID":-1,"targetId":-1,"kill":true,"mainID":-1}),
    );
}
#[test]
fn hello() {
    check(
        74,
        C,
        &w().int(-1)
            .string("")
            .string("synthetic")
            .int(-1)
            .short(2)
            .raw(&[0, 255])
            .string("")
            .string("")
            .string("")
            .string("")
            .string("")
            .0,
        json!({"gameId":-1,"buildVersion":"","accessToken":"synthetic","keyTime":-1,"key":"00ff","userPlatform":"","playPlatform":"","platformToken":"","clientToken":"","userToken":""}),
    );
}
#[test]
fn create_success() {
    check(
        101,
        S,
        &w().int(-1).int(-1).string("").0,
        json!({"objectId":-1,"charId":-1,"str":""}),
    );
}
#[test]
fn text() {
    check(
        44,
        S,
        &w().string("")
            .int(-1)
            .short(-1)
            .byte(255)
            .string("")
            .string("synthetic")
            .string("")
            .byte(255)
            .int(-1)
            .0,
        json!({"name":"","objectId":-1,"numStars":-1,"bubbleTime":255,"recipient":"","text":"synthetic","cleanText":"","isSupporter":true,"starBackground":-1}),
    );
}
#[test]
fn player_shoot() {
    check(
        30,
        C,
        &w().int(-1)
            .short(-1)
            .short(-1)
            .byte(255)
            .pos()
            .float(1.25)
            .byte(255)
            .byte(255)
            .byte(255)
            .pos()
            .0,
        json!({"time":-1,"bulletId":-1,"weaponId":65535,"projectileId":-1,"startingPos":pos(),"angle":1.25,"isBurst":true,"patternIdx":-1,"attackType":-1,"playerPosition":pos()}),
    );
}
#[test]
fn ally_shoot() {
    check(
        49,
        S,
        &w().short(-1).int(-1).int(-1).float(1.25).byte(255).0,
        json!({"bulletId":65535,"ownerId":-1,"containerType":-1,"angle":1.25,"inspiredBuff":true}),
    );
}
#[test]
fn damage() {
    check(
        75,
        S,
        &w().int(-1)
            .byte(2)
            .raw(&[0, 255])
            .short(-1)
            .byte(255)
            .short(-1)
            .int(-1)
            .0,
        json!({"targetId":-1,"effects":[0,255],"damageAmount":65535,"damageProperties":true,"bulletId":65535,"objectId":-1}),
    );
}
#[test]
fn aoe() {
    check(
        64,
        S,
        &w().pos()
            .float(1.25)
            .short(-1)
            .byte(255)
            .float(-2.5)
            .short(-1)
            .int(-1)
            .byte(255)
            .0,
        json!({"pos":pos(),"radius":1.25,"damage":65535,"effect":255,"duration":-2.5,"origType":65535,"color":-1,"armorPiercing":true}),
    );
}
#[test]
fn ground_damage() {
    check(
        103,
        C,
        &w().int(-1).pos().0,
        json!({"time":-1,"position":pos()}),
    );
}
#[test]
fn reconnect() {
    check(
        45,
        S,
        &w().string("")
            .string("synthetic.invalid")
            .short(-1)
            .int(-1)
            .int(-1)
            .short(2)
            .raw(&[128, 255])
            .0,
        json!({"name":"","host":"synthetic.invalid","port":65535,"gameId":-1,"keyTime":-1,"key":"80ff"}),
    );
}
#[test]
fn owner_object_id_stat_is_distinct_from_exaltation_packet() {
    let body = w()
        .int(1)
        .int(200)
        .int(200)
        .short(0)
        .short(1)
        .byte(1) // synthetic carrier object ID
        .pos()
        .byte(1) // stat count
        .byte(114)
        .byte(2) // synthetic owner object ID
        .byte(0)
        .0;
    check(
        10,
        S,
        &body,
        json!({"tickId":1,"tickTime":200,"serverRealTimeMS":200,"serverLastTimeRTTMS":0,
            "status":[{"objectId":1,"pos":pos(),"stats":[{
                "statTypeNum":114,"statType":"OWNER_OBJECT_ID","statValue":2,
                "stringStatValue":null,"statValueTwo":0}]}]}),
    );
    assert_eq!(
        decode_packet(114, S, &w().short(-1).raw(&[0x41; 8]).0)
            .unwrap()
            .0
            .name(),
        "EXALTATION_BONUS_CHANGED"
    );
}

#[test]
fn exaltation() {
    check(
        114,
        S,
        &w().short(-1).raw(&[0x41; 8]).0,
        json!({"objType":-1,"dexterityProgress":-1,"speedProgress":-1,"vitalityProgress":-1,"wisdomProgress":-1,"defenseProgress":-1,"attackProgress":-1,"manaProgress":-1,"healthProgress":-1}),
    );
}
#[test]
fn death() {
    check(
        46,
        S,
        &w().string("")
            .byte(0x41)
            .string("")
            .int(-1)
            .byte(0x41)
            .byte(1)
            .string("synthetic")
            .byte(0x41)
            .byte(2)
            .string("")
            .0,
        json!({"accountId":"","charId":-1,"killedBy":"","gravestoneType":-1,"totalFame":-1,"fameData":[{"achievement":"synthetic","fameLevel":-1,"fameAdded":2}],"stats":""}),
    );
}
fn status() -> Wire {
    w().byte(0x41)
        .pos()
        .byte(2)
        .byte(6)
        .string("synthetic")
        .byte(0x41)
        .byte(255)
        .byte(0x41)
        .byte(2)
}
fn status_json() -> Value {
    json!({"objectId":-1,"pos":pos(),"stats":[{"statTypeNum":6,"statType":"EXP_STAT","statValue":0,"stringStatValue":"synthetic","statValueTwo":-1},{"statTypeNum":255,"statType":null,"statValue":-1,"stringStatValue":null,"statValueTwo":2}]})
}
#[test]
fn update() {
    check(
        42,
        S,
        &w().pos()
            .byte(255)
            .byte(1)
            .short(-1)
            .short(-1)
            .short(-1)
            .byte(1)
            .short(-1)
            .raw(&status().0)
            .byte(1)
            .byte(0x41)
            .0,
        json!({"pos":pos(),"levelType":-1,"tiles":[{"x":-1,"y":-1,"type":65535}],"newObjects":[{"objectType":65535,"status":status_json()}],"drops":[-1]}),
    );
}
#[test]
fn new_tick() {
    check(
        10,
        S,
        &w().int(-1)
            .int(-1)
            .int(-1)
            .short(-1)
            .short(1)
            .raw(&status().0)
            .0,
        json!({"tickId":-1,"tickTime":-1,"serverRealTimeMS":4294967295u64,"serverLastTimeRTTMS":65535,"status":[status_json()]}),
    );
    assert!(decode_packet(10, S, &w().int(0).int(0).int(0).short(0).short(-1).0).is_err());
}
#[test]
fn server_player_shoot() {
    let base = w()
        .short(-1)
        .int(-1)
        .int(-1)
        .pos()
        .float(1.25)
        .short(-1)
        .int(-1)
        .0;
    for n in [0, 1, 2, 6] {
        let mut body = base.clone();
        body.extend_from_slice(&[255, 255, 0x3f, 0xa0, 0, 0][..n]);
        let (p, left) = decode_packet(12, S, &body).unwrap();
        assert_eq!(left, 0);
        assert_eq!(
            p.fields().unwrap(),
            json!({"bulletId":-1,"ownerId":-1,"containerType":-1,"startingPos":pos(),"angle":1.25,"damage":-1,"summonerId":-1,"bulletType":if n>0 {-1}else{0},"bulletCount":if n>1 {-1}else{0},"anglesBetweenBullets":if n==6 {1.25}else{0.0}})
        );
    }
    for end in 0..base.len() {
        assert!(decode_packet(12, S, &base[..end]).is_err());
    }
    for n in 3..6 {
        let mut b = base.clone();
        b.extend(vec![0; n]);
        assert!(decode_packet(12, S, &b).is_err());
    }
}
#[test]
fn enemy_shoot() {
    let base = w()
        .short(-1)
        .int(-1)
        .byte(255)
        .pos()
        .float(1.25)
        .short(-1)
        .0;
    for tail in [vec![], w().byte(3).float(-2.5).0] {
        let mut b = base.clone();
        b.extend(&tail);
        assert_eq!(
            decode_packet(35, S, &b).unwrap().0.fields().unwrap(),
            json!({"bulletId":-1,"ownerId":-1,"bulletType":255,"startingPos":pos(),"angle":1.25,"damage":-1,"numShots":if tail.is_empty(){255}else{3},"angleInc":if tail.is_empty(){0.0}else{-2.5}})
        );
    }
    for end in 0..base.len() {
        assert!(decode_packet(35, S, &base[..end]).is_err());
    }
    for n in 1..5 {
        let mut b = base.clone();
        b.extend(vec![0; n]);
        assert!(decode_packet(35, S, &b).is_err());
    }
}
fn map(mods: &str) -> Vec<u8> {
    w().int(-1)
        .int(-1)
        .string("")
        .string("")
        .string("")
        .int(-1)
        .int(-1)
        .float(1.25)
        .byte(255)
        .byte(0)
        .byte(1)
        .short(-1)
        .int(-1)
        .string("")
        .short(-1)
        .short(-1)
        .byte(255)
        .int(-1)
        .string(mods)
        .short(-1)
        .0
}
#[test]
fn map_info() {
    let b = map("a;;c;d;|S");
    let f = decode_packet(92, S, &b).unwrap().0.fields().unwrap();
    assert_eq!(
        f,
        json!({"width":-1,"height":-1,"name":"","displayName":"","realmName":"","fp":-1,"seed":-1,"background":-1,"difficulty":1.25,"allowPlayerTeleport":true,"noSave":false,"showDisplays":true,"maxPlayerCount":-1,"gameOpenedTime":-1,"versionNumber":"","unknown1":-1,"viewDistance":-1,"unknown2":true,"unknownInt":-1,"dungeonModifiers":"a","dungeonModifiers2":"","dungeonModifiers3":"c","dungeonModifiers4":"d","dungeonGrade":"S","bgColor":-1,"maxRealmScore":-1,"currentRealmScore":-1})
    );
    for end in 0..b.len() {
        assert!(decode_packet(92, S, &b[..end]).is_err());
    }
    for tail in 1..8 {
        let mut b = b.clone();
        b.extend(vec![0; tail]);
        let (p, left) = decode_packet(92, S, &b).unwrap();
        assert_eq!(left, tail);
        assert_eq!(p.fields().unwrap(), f);
    }
    let mut scores = b.clone();
    scores.extend(w().int(100).int(20).0);
    let f = decode_packet(92, S, &scores).unwrap().0.fields().unwrap();
    assert_eq!(f["maxRealmScore"], 100);
    assert_eq!(f["currentRealmScore"], 20);
    for (mods, key, left, grade) in [
        ("a|A", "dungeonModifiers", "a", "A"),
        ("a;b|B", "dungeonModifiers2", "b", "B"),
        ("a;b;c|C", "dungeonModifiers3", "c", "C"),
        ("a;b;c;d|D", "dungeonModifiers4", "d", "D"),
    ] {
        let f = decode_packet(92, S, &map(mods))
            .unwrap()
            .0
            .fields()
            .unwrap();
        assert_eq!(f[key], left);
        assert_eq!(f["dungeonGrade"], grade);
    }
    let f = decode_packet(92, S, &map("")).unwrap().0.fields().unwrap();
    assert_eq!(f["dungeonModifiers"], "");
    assert!(f["dungeonModifiers2"].is_null());
    assert!(f["dungeonGrade"].is_null());
}
#[test]
fn notification() {
    let names = [
        (0, "StatIncrease"),
        (1, "ServerMessage"),
        (2, "ErrorMessage"),
        (3, "StickyMessage"),
        (4, "Global"),
        (5, "Queue"),
        (6, "ObjectText"),
        (7, "PlayerDeath"),
        (8, "PortalOpened"),
        (9, "TeleportationError"),
        (10, "PlayerCallout"),
        (11, "ProgressBar"),
        (12, "Behavior"),
        (13, "Emote"),
        (14, "Victory"),
        (15, "MissionRefresh"),
        (16, "MissionsProgressOnlyRefresh"),
        (20, "BlueprintUnlock"),
        (21, "WithIcon"),
        (22, "FameBonus"),
        (23, "ForgeFire"),
    ];
    for (code, name) in names {
        for extra in [0, 1, 4] {
            let mut b = w().byte(code).byte(extra);
            let mut f = json!({"effect":name,"extra":extra,"objectId":0,"message":null,"uiExtra":0,"realmQueueMessageType":0,"queuePos":0,"color":0,"pictureType":0,"senderObjectId":0,"numberOfStars":0,"progressMax":0,"progressValue":0,"emoteType":0});
            if matches!(code,0..=4|6..=10|12) || (code == 11 && extra & 3 != 0) {
                b = b.string("synthetic");
                f["message"] = json!("synthetic");
            }
            match code {
                4 => {
                    b = b.short(-1);
                    f["uiExtra"] = json!(-1);
                }
                5 => {
                    b = b.int(-1).short(-1);
                    f["realmQueueMessageType"] = json!(-1);
                    f["queuePos"] = json!(-1);
                }
                6 => {
                    b = b.int(-1).int(-1);
                    f["objectId"] = json!(-1);
                    f["color"] = json!(-1);
                }
                7 | 8 => {
                    b = b.int(-1);
                    f["pictureType"] = json!(-1);
                }
                10 => {
                    b = b.int(-1).short(-1);
                    f["senderObjectId"] = json!(-1);
                    f["numberOfStars"] = json!(-1);
                }
                11 if extra != 0 => {
                    b = b.int(-1).short(-1);
                    f["progressMax"] = json!(-1);
                    f["progressValue"] = json!(-1);
                }
                12 => {
                    b = b.int(-1).int(-1);
                    f["pictureType"] = json!(-1);
                    f["color"] = json!(-1);
                }
                13 => {
                    b = b.int(-1).int(-1);
                    f["objectId"] = json!(-1);
                    f["emoteType"] = json!(-1);
                }
                _ => {}
            }
            check(67, S, &b.0, f);
        }
    }
    assert!(decode_packet(67, S, &[255, 0]).is_err());
}
#[test]
fn every_typed_id_rejects_wrong_direction_and_arbitrary_bytes_never_panic() {
    for (id, dir) in [
        (74, C),
        (0, S),
        (101, S),
        (92, S),
        (42, S),
        (10, S),
        (44, S),
        (67, S),
        (46, S),
        (30, C),
        (12, S),
        (49, S),
        (25, C),
        (75, S),
        (90, C),
        (35, S),
        (64, S),
        (103, C),
        (105, C),
        (45, S),
        (114, S),
        (183, S),
    ] {
        let wrong = if dir == C { S } else { C };
        assert!(matches!(
            decode_packet(id, wrong, &[]).unwrap().0,
            Packet::Unknown { .. }
        ));
        for n in 0..64 {
            let b = vec![255; n];
            let _ = decode_packet(id, dir, &b);
        }
    }
}

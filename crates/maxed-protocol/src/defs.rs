//! Single wire-schema registry ported from RealmShark; see NOTICE.md.
#![allow(non_snake_case)]
use crate::privacy::SecretKind::{Credential, Identity, PlayerText};
use crate::{ProtocolError, Reader};
use maxed_types::Direction;
use serde::{Serialize, Serializer};

pub struct HexBytes(pub Vec<u8>);
impl Serialize for HexBytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::canonical::lower_hex(&self.0))
    }
}

macro_rules! define_packets {
    ($( $id:literal => $variant:ident, $name:literal, $dir:ident, |$r:ident| {
        $( $(#[$attr:meta])* $field:ident : $ty:ty = $read:expr ),* $(,)?
    });* $(;)?) => {
        $(#[derive(Serialize)] pub struct $variant { $($(#[$attr])* pub $field: $ty),* })*
        pub enum Packet { $($variant($variant),)* Unknown { id: u8, len: usize } }
        pub fn decode_packet(id: u8, dir: Direction, body: &[u8]) -> Result<(Packet, usize), ProtocolError> {
            #[allow(unused_mut, unused_variables)]
            match (id, dir) {
                $(($id, Direction::$dir) => {
                    let mut $r = Reader::new(body);
                    $( $r.field_name(stringify!($field)); let $field: $ty = $read;)*
                    Ok((Packet::$variant($variant { $($field),* }), $r.remaining()))
                },)*
                _ => Ok((Packet::Unknown { id, len: body.len().checked_add(5).ok_or(ProtocolError::Length)? }, 0)),
            }
        }
        pub(crate) fn privacy_inventory(id: u8, dir: Direction, body: &[u8], context: Option<&crate::privacy::ObjectContext>) -> Result<crate::privacy::Inventory, ProtocolError> {
            #[allow(unused_mut, unused_variables)]
            match (id, dir) {
                $(($id, Direction::$dir) => {
                    let mut $r = Reader::for_privacy(body, context);
                    $( $r.field_name(stringify!($field)); let $field: $ty = $read;)*
                    // Exercise the same serializer checks as canonical export.
                    let packet = Packet::$variant($variant { $($field),* });
                    packet.fields()?;
                    $r.finish_privacy()
                },)*
                _ => Err(ProtocolError::Definition),
            }
        }
        impl Packet {
            pub fn name(&self) -> &'static str {
                match self { $(Self::$variant(_) => $name,)* Self::Unknown { id, .. } => known_name(*id) }
            }
            pub fn fields(&self) -> Result<serde_json::Value, ProtocolError> {
                match self {
                    $(Self::$variant(v) => serde_json::to_value(v).map_err(|_| ProtocolError::NonFinite),)*
                    Self::Unknown { len, .. } => Ok(serde_json::json!({"len":len})),
                }
            }
        }
        /// Return only schema-authored field names for metadata-only diff paths.
        pub fn schema_field(name: &str) -> Option<&'static str> {
            $( $(if name == stringify!($field) { return Some(stringify!($field)); })* )*
            match name {
                "x" => Some("x"), "y" => Some("y"), "type" => Some("type"),
                "objectType" => Some("objectType"), "statTypeNum" => Some("statTypeNum"),
                "statType" => Some("statType"), "statValue" => Some("statValue"),
                "stringStatValue" => Some("stringStatValue"), "statValueTwo" => Some("statValueTwo"),
                "achievement" => Some("achievement"), "fameLevel" => Some("fameLevel"), "fameAdded" => Some("fameAdded"),
                "effect" => Some("effect"), "extra" => Some("extra"), "message" => Some("message"),
                "uiExtra" => Some("uiExtra"), "realmQueueMessageType" => Some("realmQueueMessageType"),
                "queuePos" => Some("queuePos"), "pictureType" => Some("pictureType"),
                "senderObjectId" => Some("senderObjectId"), "numberOfStars" => Some("numberOfStars"),
                "progressMax" => Some("progressMax"), "progressValue" => Some("progressValue"),
                "emoteType" => Some("emoteType"), "len" => Some("len"), _ => None,
            }
        }
    }
}

define_packets! {
    105 => Escape, "ESCAPE", ClientToServer, |r| {};
    90 => PlayerHit, "PLAYERHIT", ClientToServer, |r| {
        bulletId:i16=r.i16()?,
        objectId:i32=r.i32()?,
    };
    0 => Failure, "FAILURE", ServerToClient, |r| {
        errorId:Option<&'static str>=failure_name(r.i32()?),
        errorDescription:String=r.public_literal(crate::privacy::FAILURE_MESSAGES)?,
    };
    25 => EnemyHit, "ENEMYHIT", ClientToServer, |r| {
        time:i32=r.i32()?,
        bulletId:i16=r.i16()?,
        shooterID:i32=r.i32()?,
        targetId:i32=r.i32()?,
        kill:bool=r.bool()?,
        mainID:i32=r.i32()?,
    };
    74 => Hello, "HELLO", ClientToServer, |r| {
        gameId:i32=r.i32()?,
        buildVersion:String=r.public_if(crate::privacy::version, true)?,
        accessToken:String=r.secret_string(Credential)?,
        keyTime:i32=r.i32()?,
        key:HexBytes=HexBytes(r.secret_bytes()?),
        // RS outgoing/HelloPacket.java: only known platform words are public.
        userPlatform:String=r.public_literal(&["Unity", "unity", "Windows"])?,
        playPlatform:String=r.public_literal(&["Steam", "steam", "Deca", "Standalone"])?,
        platformToken:String=r.secret_string(Credential)?,
        clientToken:String=r.secret_string(Credential)?,
        userToken:String=r.secret_string(Credential)?,
    };
    101 => CreateSuccess, "CREATE_SUCCESS", ServerToClient, |r| {
        objectId:i32=r.i32()?,
        charId:i32=r.i32()?,
        // RS TomatoData.setUserId -> RealmCharacterStats.decode: candidate numeric
        // counter snapshot. Bounded varint extensions are recognized in opt-in
        // diagnostics only; extended snapshots remain incomplete and cannot
        // support completion fallback or pass privacy admission.
        str:String=r.public_if(crate::privacy::character_stats, false)?,
    };
    44 => Text, "TEXT", ServerToClient, |r| {
        name:String=r.text_sender()?,
        objectId:i32=r.i32()?,
        numStars:i16=r.i16()?,
        bubbleTime:u8=r.u8()?,
        recipient:String=r.secret_string(Identity)?,
        text:String=if name.starts_with('#') {r.npc_text(&name)?} else {r.secret_string(PlayerText)?},
        cleanText:String=if name.starts_with('#') {r.npc_text(&name)?} else {r.secret_string(PlayerText)?},
        isSupporter:bool=r.bool()?,
        starBackground:i32=r.i32()?,
    };
    30 => PlayerShoot, "PLAYERSHOOT", ClientToServer, |r| {
        time:i32=r.i32()?,
        bulletId:i16=r.i16()?,
        weaponId:u16=r.u16()?,
        projectileId:i8=r.i8()?,
        startingPos:WorldPosData=WorldPosData::read(&mut r)?,
        #[serde(serialize_with="crate::canonical::float")] angle:f32=r.f32()?,
        isBurst:bool=r.bool()?,
        patternIdx:i8=r.i8()?,
        attackType:i8=r.i8()?,
        playerPosition:WorldPosData=WorldPosData::read(&mut r)?,
    };
    49 => AllyShoot, "ALLYSHOOT", ServerToClient, |r| {
        bulletId:u16=r.u16()?,
        ownerId:i32=r.i32()?,
        containerType:i32=r.i32()?,
        #[serde(serialize_with="crate::canonical::float")] angle:f32=r.f32()?,
        inspiredBuff:bool=r.bool()?,
    };
    75 => Damage, "DAMAGE", ServerToClient, |r| {
        targetId:i32=r.i32()?,
        effects:Vec<u8>={let n=r.u8()?;r.list(i32::from(n),1,Reader::u8)?},
        damageAmount:u16=r.u16()?,
        damageProperties:bool=r.bool()?,
        bulletId:u16=r.u16()?,
        objectId:i32=r.i32()?,
    };
    64 => Aoe, "AOE", ServerToClient, |r| {
        pos:WorldPosData=WorldPosData::read(&mut r)?,
        #[serde(serialize_with="crate::canonical::float")] radius:f32=r.f32()?,
        damage:u16=r.u16()?,
        effect:u8=r.u8()?,
        #[serde(serialize_with="crate::canonical::float")] duration:f32=r.f32()?,
        origType:u16=r.u16()?,
        color:i32=r.i32()?,
        armorPiercing:bool=r.bool()?,
    };
    103 => GroundDamage, "GROUNDDAMAGE", ClientToServer, |r| {
        time:i32=r.i32()?,
        position:WorldPosData=WorldPosData::read(&mut r)?,
    };
    // RS incoming/ReconnectPacket.java: name and host identify the destination game server.
    45 => Reconnect, "RECONNECT", ServerToClient, |r| {
        name:String=r.public_string()?,
        host:String=r.public_string()?,
        port:u16=r.u16()?,
        gameId:i32=r.i32()?,
        keyTime:i32=r.i32()?,
        key:HexBytes=HexBytes(r.secret_bytes()?),
    };
    114 => ExaltationUpdate, "EXALTATION_BONUS_CHANGED", ServerToClient, |r| {
        objType:i16=r.i16()?,
        dexterityProgress:i32=r.ci()?,
        speedProgress:i32=r.ci()?,
        vitalityProgress:i32=r.ci()?,
        wisdomProgress:i32=r.ci()?,
        defenseProgress:i32=r.ci()?,
        attackProgress:i32=r.ci()?,
        manaProgress:i32=r.ci()?,
        healthProgress:i32=r.ci()?,
    };
    46 => Death, "DEATH", ServerToClient, |r| {
        accountId:String=r.secret_string(Credential)?,
        charId:i32=r.ci()?,
        killedBy:String=r.string()?,
        gravestoneType:i32=r.i32()?,
        totalFame:i32=r.ci()?,
        fameData:Vec<FameData>={let n=r.ci()?;r.list(n,4,FameData::read)?},
        stats:String=r.string()?,
    };
    42 => Update, "UPDATE", ServerToClient, |r| {
        pos:WorldPosData=WorldPosData::read(&mut r)?,
        levelType:i8=r.i8()?,
        tiles:Vec<GroundTileData>={let n=r.ci()?;r.list(n,6,GroundTileData::read)?},
        newObjects:Vec<ObjectData>={let n=r.ci()?;r.list(n,12,ObjectData::read)?},
        drops:Vec<i32>={let n=r.ci()?;r.list(n,1,Reader::ci)?},
    };
    10 => NewTick, "NEWTICK", ServerToClient, |r| {
        tickId:i32=r.i32()?,
        tickTime:i32=r.i32()?,
        serverRealTimeMS:u32=r.u32()?,
        serverLastTimeRTTMS:u16=r.u16()?,
        status:Vec<ObjectStatusData>={let n=r.u16()?;r.list(i32::from(n),10,ObjectStatusData::read)?},
    };
    12 => ServerPlayerShoot, "SERVERPLAYERSHOOT", ServerToClient, |r| {
        bulletId:i16=r.i16()?,
        ownerId:i32=r.i32()?,
        containerType:i32=r.i32()?,
        startingPos:WorldPosData=WorldPosData::read(&mut r)?,
        #[serde(serialize_with="crate::canonical::float")] angle:f32=r.f32()?,
        damage:i16=r.i16()?,
        summonerId:i32=r.i32()?,
        bulletType:i8=if r.remaining()>0 {r.i8()?}else{0},
        bulletCount:i8=if r.remaining()>0 {r.i8()?}else{0},
        #[serde(serialize_with="crate::canonical::float")] anglesBetweenBullets:f32=if r.remaining()>0 {r.f32()?}else{0.0},
    };
    35 => EnemyShoot, "ENEMYSHOOT", ServerToClient, |r| {
        bulletId:i16=r.i16()?,
        ownerId:i32=r.i32()?,
        bulletType:u8=r.u8()?,
        startingPos:WorldPosData=WorldPosData::read(&mut r)?,
        #[serde(serialize_with="crate::canonical::float")] angle:f32=r.f32()?,
        damage:i16=r.i16()?,
        #[serde(skip)] has_tail:bool=r.remaining()>0,
        numShots:u8=if has_tail {r.u8()?}else{255},
        #[serde(serialize_with="crate::canonical::float")] angleInc:f32=if has_tail {r.f32()?}else{0.0},
    };
    // RS incoming/MapInfoPacket.java: map names, version and modifiers describe server game data.
    92 => MapInfo, "MAPINFO", ServerToClient, |r| {
        width:i32=r.i32()?,
        height:i32=r.i32()?,
        name:String=r.public_string()?,
        displayName:String=r.public_string()?,
        realmName:String=r.public_string()?,
        fp:i32=r.i32()?,
        seed:i32=fp,
        background:i32=r.i32()?,
        #[serde(serialize_with="crate::canonical::float")] difficulty:f32=r.f32()?,
        allowPlayerTeleport:bool=r.bool()?,
        noSave:bool=r.bool()?,
        showDisplays:bool=r.bool()?,
        maxPlayerCount:i16=r.i16()?,
        gameOpenedTime:i32=r.i32()?,
        versionNumber:String=r.public_string()?,
        unknown1:i16=r.i16()?,
        viewDistance:i16=r.i16()?,
        unknown2:bool=r.bool()?,
        unknownInt:i32=r.i32()?,
        #[serde(skip)] modifiers:[Option<String>;5]=modifiers(r.public_string()?),
        dungeonModifiers:Option<String>=modifiers[0].clone(),
        dungeonModifiers2:Option<String>=modifiers[1].clone(),
        dungeonModifiers3:Option<String>=modifiers[2].clone(),
        dungeonModifiers4:Option<String>=modifiers[3].clone(),
        dungeonGrade:Option<String>=modifiers[4].clone(),
        bgColor:i16=r.i16()?,
        #[serde(skip)] has_scores:bool=r.remaining()>=8,
        maxRealmScore:i32=if has_scores {r.i32()?}else{-1},
        currentRealmScore:i32=if has_scores {r.i32()?}else{-1},
    };
    67 => Notification, "NOTIFICATION", ServerToClient, |r| {
        #[serde(flatten)] data:NotificationData=NotificationData::read(&mut r)?,
    };
    // RS incoming/QuestObjectIdPacket.java: signed target then compressed count/IDs.
    82 => QuestObjId, "QUESTOBJID", ServerToClient, |r| {
        objectId:i32=r.i32()?,
        list:Vec<i32>={
            let count = r.ci()?;
            if !(0..=64).contains(&count) { return Err(ProtocolError::Definition); }
            r.list(count, 1, Reader::ci)?
        },
    };
    // Absolute current score; the maximum belongs to MAPINFO.
    169 => RealmScoreUpdate, "REALM_SCORE_UPDATE", ServerToClient, |r| {
        score:i32=r.i32()?,
    };
}

pub fn known_name(id: u8) -> &'static str {
    match id {
        0 => "FAILURE",
        1 => "TELEPORT",
        3 => "CLAIM_LOGIN_REWARD_MSG",
        4 => "DELETE_PET",
        5 => "REQUESTTRADE",
        6 => "QUEST_FETCH_RESPONSE",
        7 => "JOINGUILD",
        8 => "PING",
        9 => "PLAYERTEXT",
        10 => "NEWTICK",
        11 => "SHOWEFFECT",
        12 => "SERVERPLAYERSHOOT",
        13 => "USEITEM",
        14 => "TRADEACCEPTED",
        15 => "GUILDREMOVE",
        16 => "PETUPGRADEREQUEST",
        18 => "GOTO",
        19 => "INVDROP",
        20 => "OTHERHIT",
        21 => "NAMERESULT",
        22 => "BUYRESULT",
        23 => "HATCH_PET",
        24 => "ACTIVE_PET_UPDATE_REQUEST",
        25 => "ENEMYHIT",
        26 => "GUILDRESULT",
        27 => "EDITACCOUNTLIST",
        28 => "TRADECHANGED",
        30 => "PLAYERSHOOT",
        31 => "PONG",
        33 => "PET_CHANGE_SKIN_MSG",
        34 => "TRADEDONE",
        35 => "ENEMYSHOOT",
        36 => "ACCEPTTRADE",
        37 => "CHANGEGUILDRANK",
        38 => "PLAYSOUND",
        39 => "VERIFY_EMAIL",
        40 => "SQUAREHIT",
        41 => "NEW_ABILITY",
        42 => "UPDATE",
        44 => "TEXT",
        45 => "RECONNECT",
        46 => "DEATH",
        47 => "USEPORTAL",
        48 => "QUEST_ROOM_MSG",
        49 => "ALLYSHOOT",
        51 => "RESKIN",
        52 => "RESET_DAILY_QUESTS",
        53 => "PET_CHANGE_FORM_MSG",
        55 => "INVSWAP",
        56 => "CHANGETRADE",
        57 => "CREATE",
        58 => "QUEST_REDEEM",
        59 => "CREATEGUILD",
        60 => "SETCONDITION",
        61 => "LOAD",
        62 => "MOVE",
        63 => "KEY_INFO_RESPONSE",
        64 => "AOE",
        65 => "GOTOACK",
        66 => "GLOBAL_NOTIFICATION",
        67 => "NOTIFICATION",
        69 => "CLIENTSTAT",
        74 => "HELLO",
        75 => "DAMAGE",
        76 => "ACTIVEPETUPDATE",
        77 => "INVITEDTOGUILD",
        78 => "PETYARDUPDATE",
        79 => "PASSWORD_PROMPT",
        81 => "UPDATEACK",
        82 => "QUESTOBJID",
        83 => "PIC",
        84 => "REALM_HERO_LEFT_MSG",
        85 => "BUY",
        86 => "TRADESTART",
        87 => "EVOLVE_PET",
        88 => "TRADEREQUESTED",
        89 => "AOEACK",
        90 => "PLAYERHIT",
        91 => "CANCELTRADE",
        92 => "MAPINFO",
        93 => "LOGIN_REWARD_MSG",
        94 => "KEY_INFO_REQUEST",
        95 => "INVRESULT",
        96 => "QUEST_REDEEM_RESPONSE",
        97 => "CHOOSENAME",
        98 => "QUEST_FETCH_ASK",
        99 => "ACCOUNTLIST",
        101 => "CREATE_SUCCESS",
        102 => "CHECKCREDITS",
        103 => "GROUNDDAMAGE",
        104 => "GUILDINVITE",
        105 => "ESCAPE",
        106 => "FILE",
        107 => "RESKIN_UNLOCK",
        108 => "NEW_CHARACTER_INFORMATION",
        109 => "UNLOCK_INFORMATION",
        112 => "QUEUE_INFORMATION",
        113 => "QUEUE_CANCEL",
        114 => "EXALTATION_BONUS_CHANGED",
        115 => "REDEEM_EXALTATION_REWARD",
        117 => "VAULT_UPDATE",
        118 => "FORGE_REQUEST",
        119 => "FORGE_RESULT",
        120 => "FORGE_UNLOCKED_BLUEPRINTS",
        121 => "SHOOT_ACK",
        122 => "CHANGE_ALLYSHOOT",
        123 => "GET_PLAYERS_LIST_MESSAGE",
        124 => "MODERATOR_ACTION_MESSAGE",
        126 => "CREEP_MOVE_MESSAGE",
        129 => "CUSTOM_MAP_DELETE",
        131 => "CUSTOM_MAP_LIST",
        133 => "CREEP_HIT",
        134 => "PLAYER_CALLOUT",
        136 => "BUY_REFINEMENT",
        137 => "DASH",
        138 => "DASH_ACK",
        139 => "STATS",
        140 => "BUY_CUSTOMISATION_SOCKET",
        145 => "FAVOUR_PET",
        146 => "SKIN_RECYCLE",
        147 => "UNKNOWN147",
        148 => "DAMAGE_BOOST",
        149 => "CLAIM_BATTLE_PASS",
        150 => "CLAIM_BP_MILESTONE_RESULT",
        151 => "BOOST_BP_MILESTONE",
        154 => "CONVERT_SEASONAL_CHARACTER",
        155 => "RETITLE",
        156 => "SET_GRAVE_STONE",
        157 => "SET_ABILITY",
        159 => "EMOTE",
        160 => "BUY_EMOTE",
        162 => "SET_TRACKED_SEASON",
        163 => "CLAIM_MISSION",
        164 => "UNKNOWN164",
        165 => "UNKNOWN165",
        166 => "STASIS",
        167 => "SET_DISCOVERABLE",
        169 => "REALM_SCORE_UPDATE",
        170 => "CLAIM_REWARDS_INFO_PROMPT",
        171 => "CLAIM_CHEST_REWARD",
        172 => "CHEST_REWARD_RESULT",
        173 => "UNLOCK_ENCHANTMENT_SLOT",
        175 => "UNLOCK_ENCHANTMENT",
        177 => "APPLY_ENCHANTMENT",
        180 => "ACTIVATE_CRUCIBLE",
        181 => "UNKNOWN181",
        182 => "CRUCIBLE_REQUEST",
        183 => "CRUCIBLE_RESPONSE",
        185 => "UPGRADE_ENCHANTER",
        187 => "UPGRADE_ENCHANTMENT",
        189 => "REROLL_ALL_ENCHANTMENTS",
        190 => "UNKNOWN190",
        191 => "RESET_ENCHANTMENT_REROLL_COUNT",
        200 => "CREATE_PARTY_MESSAGE",
        204 => "PARTY_ACTION_RESULT",
        209 => "PARTY_INVITE_RESPONSE",
        208 => "INCOMING_PARTY_INVITE",
        207 => "PARTY_ACTION",
        210 => "INCOMING_PARTY_MEMBER_INFO",
        212 => "PARTY_MEMBER_ADDED",
        214 => "PARTY_LIST_MESSAGE",
        215 => "PARTY_JOIN_REQUEST",
        217 => "PARTY_REQUEST_RESPONSE",
        218 => "FOR_RECONNECT",
        222 => "LOADING_SCREEN",
        _ => "UNKNOWN",
    }
}
pub fn stat_name(id: u8) -> Option<&'static str> {
    match id {
        0 => Some("MAX_HP_STAT"),
        1 => Some("HP_STAT"),
        2 => Some("SIZE_STAT"),
        3 => Some("MAX_MP_STAT"),
        4 => Some("MP_STAT"),
        5 => Some("NEXT_LEVEL_EXP_STAT"),
        6 => Some("EXP_STAT"),
        7 => Some("LEVEL_STAT"),
        8 => Some("INVENTORY_0_STAT"),
        9 => Some("INVENTORY_1_STAT"),
        10 => Some("INVENTORY_2_STAT"),
        11 => Some("INVENTORY_3_STAT"),
        12 => Some("INVENTORY_4_STAT"),
        13 => Some("INVENTORY_5_STAT"),
        14 => Some("INVENTORY_6_STAT"),
        15 => Some("INVENTORY_7_STAT"),
        16 => Some("INVENTORY_8_STAT"),
        17 => Some("INVENTORY_9_STAT"),
        18 => Some("INVENTORY_10_STAT"),
        19 => Some("INVENTORY_11_STAT"),
        20 => Some("ATTACK_STAT"),
        21 => Some("DEFENSE_STAT"),
        22 => Some("SPEED_STAT"),
        23 => Some("DISCOVERABLE"),
        24 => Some("SEASONAL"),
        25 => Some("SKIN_ID"),
        26 => Some("VITALITY_STAT"),
        27 => Some("WISDOM_STAT"),
        28 => Some("DEXTERITY_STAT"),
        29 => Some("CONDITION_STAT"),
        30 => Some("NUM_STARS_STAT"),
        31 => Some("NAME_STAT"),
        32 => Some("TEX1_STAT"),
        33 => Some("TEX2_STAT"),
        34 => Some("MERCHANDISE_TYPE_STAT"),
        35 => Some("CREDITS_STAT"),
        36 => Some("MERCHANDISE_PRICE_STAT"),
        37 => Some("ACTIVE_STAT"),
        38 => Some("ACCOUNT_ID_STAT"),
        39 => Some("FAME_STAT"),
        40 => Some("MERCHANDISE_CURRENCY_STAT"),
        41 => Some("CONNECT_STAT"),
        42 => Some("MERCHANDISE_COUNT_STAT"),
        43 => Some("MERCHANDISE_MINS_LEFT_STAT"),
        44 => Some("MERCHANDISE_DISCOUNT_STAT"),
        45 => Some("MERCHANDISE_RANK_REQ_STAT"),
        46 => Some("MAX_HP_BOOST_STAT"),
        47 => Some("MAX_MP_BOOST_STAT"),
        48 => Some("ATTACK_BOOST_STAT"),
        49 => Some("DEFENSE_BOOST_STAT"),
        50 => Some("SPEED_BOOST_STAT"),
        51 => Some("VITALITY_BOOST_STAT"),
        52 => Some("WISDOM_BOOST_STAT"),
        53 => Some("DEXTERITY_BOOST_STAT"),
        54 => Some("OWNER_ACCOUNT_ID_STAT"),
        55 => Some("RANK_REQUIRED_STAT"),
        56 => Some("NAME_CHOSEN_STAT"),
        57 => Some("CURR_FAME_STAT"),
        58 => Some("NEXT_CLASS_QUEST_FAME_STAT"),
        59 => Some("LEGENDARY_RANK_STAT"),
        60 => Some("SINK_LEVEL_STAT"),
        61 => Some("BXP_STAT"),
        62 => Some("GUILD_NAME_STAT"),
        63 => Some("GUILD_RANK_STAT"),
        64 => Some("BREATH_STAT"),
        65 => Some("XP_BOOSTED_STAT"),
        66 => Some("XP_TIMER_STAT"),
        67 => Some("LD_TIMER_STAT"),
        68 => Some("LT_TIMER_STAT"),
        71 => Some("MATERIAL_STAT"),
        72 => Some("MATERIAL_CAP_STAT"),
        80 => Some("UNIQUE_DATA_STRING"),
        81 => Some("PET_INSTANCE_ID_STAT"),
        82 => Some("PET_NAME_STAT"),
        83 => Some("PET_TYPE_STAT"),
        84 => Some("PET_RARITY_STAT"),
        85 => Some("PET_MAX_ABILITY_POWER_STAT"),
        86 => Some("PET_FAMILY_STAT"),
        87 => Some("PET_FIRST_ABILITY_POINT_STAT"),
        88 => Some("PET_SECOND_ABILITY_POINT_STAT"),
        89 => Some("PET_THIRD_ABILITY_POINT_STAT"),
        90 => Some("PET_FIRST_ABILITY_POWER_STAT"),
        91 => Some("PET_SECOND_ABILITY_POWER_STAT"),
        92 => Some("PET_THIRD_ABILITY_POWER_STAT"),
        93 => Some("PET_FIRST_ABILITY_TYPE_STAT"),
        94 => Some("PET_SECOND_ABILITY_TYPE_STAT"),
        95 => Some("PET_THIRD_ABILITY_TYPE_STAT"),
        96 => Some("NEW_CON_STAT"),
        97 => Some("FORTUNE_TOKEN_STAT"),
        98 => Some("SUPPORTER_POINTS_STAT"),
        99 => Some("SUPPORTER_STAT"),
        101 => Some("TIME_JOINED_STAT"),
        102 => Some("PROJECTILE_SPEED_MULT"),
        103 => Some("PROJECTILE_LIFE_MULT"),
        104 => Some("OPENED_AT_TIMESTAMP"),
        105 => Some("EXALTED_ATK"),
        106 => Some("EXALTED_DEF"),
        107 => Some("EXALTED_SPD"),
        108 => Some("EXALTED_VIT"),
        109 => Some("EXALTED_WIS"),
        110 => Some("EXALTED_DEX"),
        111 => Some("EXALTED_HP"),
        112 => Some("EXALTED_MP"),
        113 => Some("EXALTATION_BONUS_DAMAGE"),
        114 => Some("OWNER_OBJECT_ID"),
        115 => Some("GRAVE_ACCOUNT_ID"),
        116 => Some("POTION_ONE_TYPE"),
        117 => Some("POTION_TWO_TYPE"),
        118 => Some("POTION_THREE_TYPE"),
        119 => Some("POTION_BELT"),
        120 => Some("FORGE_FIRE"),
        121 => Some("MODIFIERS_STAT"),
        122 => Some("DIFFICULTY_STAT"),
        123 => Some("OBJECT_IS_ACTIVE_STAT"),
        124 => Some("POWER_LEVEL_STAT"),
        125 => Some("ANIMATION_STAT"),
        126 => Some("ANIMATION_TIMESTAMP_STAT"),
        127 => Some("DUST_STAT"),
        128 => Some("CRUCIBLE_STAT"),
        129 => Some("SHADER_STAT"),
        130 => Some("BACKPACK_SLOTS_STAT"),
        131 => Some("BACKPACK_0_STAT"),
        132 => Some("BACKPACK_1_STAT"),
        133 => Some("BACKPACK_2_STAT"),
        134 => Some("BACKPACK_3_STAT"),
        135 => Some("BACKPACK_4_STAT"),
        136 => Some("BACKPACK_5_STAT"),
        137 => Some("BACKPACK_6_STAT"),
        138 => Some("BACKPACK_7_STAT"),
        139 => Some("BACKPACK_8_STAT"),
        140 => Some("BACKPACK_9_STAT"),
        141 => Some("BACKPACK_10_STAT"),
        142 => Some("BACKPACK_11_STAT"),
        143 => Some("BACKPACK_12_STAT"),
        144 => Some("BACKPACK_13_STAT"),
        145 => Some("BACKPACK_14_STAT"),
        146 => Some("BACKPACK_15_STAT"),
        147 => Some("DUST_AMOUNT_STAT"),
        155 => Some("BloodRitualStat"),
        _ => None,
    }
}

#[derive(Serialize)]
pub struct WorldPosData {
    #[serde(serialize_with = "crate::canonical::float")]
    pub x: f32,
    #[serde(serialize_with = "crate::canonical::float")]
    pub y: f32,
}
impl WorldPosData {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        Ok(Self {
            x: r.f32()?,
            y: r.f32()?,
        })
    }
}
#[derive(Serialize)]
pub struct GroundTileData {
    pub x: i16,
    pub y: i16,
    pub r#type: u16,
}
impl GroundTileData {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        Ok(Self {
            x: r.i16()?,
            y: r.i16()?,
            r#type: r.u16()?,
        })
    }
}
#[derive(Serialize)]
pub struct ObjectData {
    pub objectType: u16,
    pub status: ObjectStatusData,
}
impl ObjectData {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        let objectType = r.u16()?;
        r.object_type(Some(objectType));
        let status = ObjectStatusData::read(r)?;
        r.object_type(None);
        Ok(Self { objectType, status })
    }
}
#[derive(Serialize)]
pub struct ObjectStatusData {
    pub objectId: i32,
    pub pos: WorldPosData,
    pub stats: Vec<StatData>,
}
impl ObjectStatusData {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        let objectId = r.ci()?;
        r.object_id(objectId);
        let pos = WorldPosData::read(r)?;
        let count = r.ci()?;
        Ok(Self {
            objectId,
            pos,
            stats: r.list(count, 3, StatData::read)?,
        })
    }
}
#[derive(Serialize)]
pub struct StatData {
    pub statTypeNum: u8,
    pub statType: Option<&'static str>,
    pub statValue: i32,
    pub stringStatValue: Option<String>,
    pub statValueTwo: i32,
}
impl StatData {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        let statTypeNum = r.u8()?;
        let statType = stat_name(statTypeNum);
        // RS StatData.isStringStat defines wire kind independently of enum names
        // and privacy policy. Unmapped numeric IDs still have a known byte extent.
        let (statValue, stringStatValue) = if matches!(
            statTypeNum,
            6 | 31 | 38 | 54 | 62 | 71 | 72 | 80 | 82 | 115 | 121 | 127 | 128 | 147 | 155
        ) {
            let policy = crate::privacy::STRING_STATS
                .iter()
                .find(|entry| entry.0 == statTypeNum)
                .map(|&(_, name, policy)| (name, policy));
            (0, Some(r.stat_string(statTypeNum, policy)?))
        } else {
            if statType.is_none() {
                r.evidence(
                    "statTypeNum",
                    Some(statTypeNum),
                    crate::privacy::EvidenceReason::UnmappedNumericStat,
                );
            }
            (r.ci()?, None)
        };
        Ok(Self {
            statTypeNum,
            statType,
            statValue,
            stringStatValue,
            statValueTwo: r.ci()?,
        })
    }
}
#[derive(Serialize)]
pub struct FameData {
    pub achievement: String,
    pub fameLevel: i32,
    pub fameAdded: i32,
}
impl FameData {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        r.field_name("achievement");
        Ok(Self {
            achievement: r.string()?,
            fameLevel: r.ci()?,
            fameAdded: r.ci()?,
        })
    }
}
fn failure_name(n: i32) -> Option<&'static str> {
    match n {
        4 => Some("IncorrectVersion"),
        5 => Some("BadKey"),
        6 => Some("InvalidTeleportTarget"),
        7 => Some("EmailVerificationNeeded"),
        9 => Some("TeleportRealmBlock"),
        10 => Some("WrongServerEntered"),
        11 => Some("ServerFull"),
        15 => Some("ServerQueue"),
        _ => None,
    }
}
fn notification_name(n: i8) -> Option<&'static str> {
    match n {
        0 => Some("StatIncrease"),
        1 => Some("ServerMessage"),
        2 => Some("ErrorMessage"),
        3 => Some("StickyMessage"),
        4 => Some("Global"),
        5 => Some("Queue"),
        6 => Some("ObjectText"),
        7 => Some("PlayerDeath"),
        8 => Some("PortalOpened"),
        9 => Some("TeleportationError"),
        10 => Some("PlayerCallout"),
        11 => Some("ProgressBar"),
        12 => Some("Behavior"),
        13 => Some("Emote"),
        14 => Some("Victory"),
        15 => Some("MissionRefresh"),
        16 => Some("MissionsProgressOnlyRefresh"),
        20 => Some("BlueprintUnlock"),
        21 => Some("WithIcon"),
        22 => Some("FameBonus"),
        23 => Some("ForgeFire"),
        // Effect 24 with extra=32 and a two-byte numeric tail has been observed.
        // Its field semantics are unestablished; retain the Definition failure.
        _ => None,
    }
}
#[derive(Serialize, Default)]
pub struct NotificationData {
    pub effect: Option<&'static str>,
    pub extra: i8,
    pub objectId: i32,
    pub message: Option<String>,
    pub uiExtra: i32,
    pub realmQueueMessageType: i32,
    pub queuePos: i32,
    pub color: i32,
    pub pictureType: i32,
    pub senderObjectId: i32,
    pub numberOfStars: i32,
    pub progressMax: i32,
    pub progressValue: i16,
    pub emoteType: i32,
}
impl NotificationData {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        let code = r.i8()?;
        let effect = Some(notification_name(code).ok_or(ProtocolError::Definition)?);
        let extra = r.i8()?;
        if code == 11 && extra & !3 != 0 {
            r.evidence(
                "extra",
                None,
                crate::privacy::EvidenceReason::UnknownNotificationLayout,
            );
        }
        let mut n = Self {
            effect,
            extra,
            ..Self::default()
        };
        r.field_name("message");
        match code {
            0..=3 | 9 => n.message = Some(r.string()?),
            4 => {
                n.message = Some(r.string()?);
                n.uiExtra = i32::from(r.i16()?);
            }
            5 => {
                n.realmQueueMessageType = r.i32()?;
                n.queuePos = i32::from(r.i16()?);
            }
            6 => {
                n.message = Some(r.string()?);
                n.objectId = r.i32()?;
                n.color = r.i32()?;
            }
            7 | 8 => {
                n.message = Some(r.notice_text(code == 7)?);
                n.pictureType = r.i32()?;
            }
            10 => {
                n.message = Some(r.string()?);
                n.senderObjectId = r.i32()?;
                n.numberOfStars = i32::from(r.i16()?);
            }
            11 if extra != 0 => {
                if extra & 3 != 0 {
                    n.message = Some(r.string()?);
                }
                n.progressMax = r.i32()?;
                n.progressValue = r.i16()?;
            }
            12 => {
                n.message = Some(r.string()?);
                n.pictureType = r.i32()?;
                n.color = r.i32()?;
            }
            13 => {
                n.objectId = r.i32()?;
                n.emoteType = r.i32()?;
            }
            _ => {}
        }
        Ok(n)
    }
}
fn modifiers(s: String) -> [Option<String>; 5] {
    let parts: Vec<_> = s.split(';').collect();
    let mut result: [Option<String>; 5] = std::array::from_fn(|i| {
        if i < 4 {
            parts.get(i).map(|s| s.to_string())
        } else {
            None
        }
    });
    if let Some(grade) = parts.get(4).and_then(|s| s.strip_prefix('|')) {
        result[4] = Some(grade.into());
    } else {
        for i in (0..4).rev() {
            if let Some((left, right)) = result[i].as_deref().and_then(|s| s.split_once('|')) {
                let (left, right) = (left.to_owned(), right.to_owned());
                result[i] = Some(left);
                result[4] = Some(right);
                break;
            }
        }
    }
    result
}

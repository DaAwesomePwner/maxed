use maxed_types::Catalog;
use serde_json::json;

// Minimal synthetic combat metadata for the two committed reconnect fixtures.
// Neutral enchant/crucible definitions exercise decoding without shipping a client catalog.
pub fn catalog(target_type: u16, target_name: &str, realm: bool) -> Catalog {
    // Known neutral weapon damage-range metadata (D-weapon-enchant), not client values.
    // All enchant metadata here is synthetic and deliberately neutral. ID 337 is the
    // enchant recorded on the Shaitan fixture's weapon (588); the real client value
    // for it differs. shaitan_recorded_sources_are_reconstructed_without_false_uncertainty
    // needs a supported definition so recorded evidence resolves as known (not
    // estimated); a neutral 1.0 keeps the fixture's damage independent of client data.
    let neutral_weapon =
        json!({"support":"supported","min_multiplier":1.0,"max_multiplier":1.0,"reasons":[]});
    serde_json::from_value(json!({
        "format_version":1,"client_fingerprint":"synthetic-fixture-subset",
        "items":[
            {"object_type":588,"name":"Synthetic weapon","tier":null,"slot_type":3,"projectiles":[{"index":0,"min_damage":180,"max_damage":255,"armor_piercing":false,"lifetime_millis":null}],"scaling":null,"sprite_key":null},
            {"object_type":589,"name":"Synthetic ability","tier":null,"slot_type":15,"projectiles":[{"index":0,"min_damage":400,"max_damage":500,"armor_piercing":true,"lifetime_millis":null}],"scaling":{"stat_id":21,"minimum":9,"damage_per_stat":5.0,"shots":6,"origin_item":null},"sprite_key":null}
        ],
        "objects":[
            {"object_type":775,"name":"Synthetic class","is_player":true,"is_boss":false,"is_miniboss":false,"sprite_key":null},
            {"object_type":target_type,"name":target_name,"is_player":false,"is_boss":!realm,"is_miniboss":false,"event_boss":realm,"realm_event_key":realm.then_some("The_Plague_Doctor"),"sprite_key":null}
        ],
        "classes":[],"enchants":[
            {"id":337,"name":"Synthetic enchant","description":"","stat_damage_multiplier":1.0,"weapon_damage":neutral_weapon},
            {"id":423,"name":"Synthetic enchant","description":"","stat_damage_multiplier":1.0,"weapon_damage":neutral_weapon},
            {"id":1203,"name":"Synthetic enchant","description":"","stat_damage_multiplier":1.0,"weapon_damage":neutral_weapon}
        ],"bags":[],"sprites":[],"tiles":[],"crucible_bonuses":[{"id":"5614074501562368","damage_multiplier":1.0}],
        "dungeons":[{"name":if realm { "Realm of the Mad God" } else { "Lair of Shaitan" },"aliases":[],"kind":if realm { "realm" } else { "dungeon" },"completion_index":null}]
    })).unwrap()
}

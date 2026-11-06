//! Curated dungeon metadata from RealmEye Wiki (Sep 2025), supplied by the
//! notification reference: https://www.realmeye.com/wiki/dungeons.
//! Ratings describe dungeons, not the difficulty of an observed instance.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DungeonCategory {
    Realm,
    RealmEvent,
    Advanced,
    OryxCastle,
    Wormhole,
    Seasonal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DungeonMeta {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub difficulty: f32,
    pub exalt: bool,
    pub exalt_gateway: bool,
    pub category: DungeonCategory,
}

macro_rules! dungeon {
    ($name:literal, [$($alias:literal),*], $difficulty:literal, $exalt:literal, $gateway:literal, $category:ident) => {
        DungeonMeta {
            name: $name, aliases: &[$($alias),*], difficulty: $difficulty,
            exalt: $exalt, exalt_gateway: $gateway, category: DungeonCategory::$category,
        }
    };
}

#[rustfmt::skip]
pub static DUNGEON_META: &[DungeonMeta] = &[
    dungeon!("The Inner Workings", [], 0.0, false, false, Realm),
    dungeon!("Pirate Cave", [], 1.0, false, false, Realm),
    dungeon!("Forest Maze", [], 1.0, false, false, Realm),
    dungeon!("Spider Den", [], 1.5, false, false, Realm),
    dungeon!("Forbidden Jungle", [], 1.5, false, false, Realm),
    dungeon!("The Hive", [], 2.0, false, false, Realm),
    dungeon!("Snake Pit", [], 2.5, false, false, Realm),
    dungeon!("Sprite World", [], 2.5, false, false, Realm),
    dungeon!("Cave of a Thousand Treasures", [], 2.5, false, false, Realm),
    dungeon!("Ancient Ruins", [], 3.0, false, false, Realm),
    dungeon!("Magic Woods", [], 3.0, false, false, Realm),
    dungeon!("Candyland Hunting Grounds", [], 3.5, false, false, Realm),
    dungeon!("Undead Lair", ["UDL"], 3.5, false, false, Realm),
    dungeon!("Puppet Master's Theatre", [], 4.0, false, false, Realm),
    dungeon!("Toxic Sewers", [], 4.0, false, false, Realm),
    dungeon!("Cursed Library", [], 4.0, false, false, Realm),
    dungeon!("Mad Lab", [], 4.0, false, false, Realm),
    dungeon!("Abyss of Demons", ["Abyss"], 4.0, false, false, Realm),
    dungeon!("Manor of the Immortals", [], 4.0, false, false, Realm),
    dungeon!("Haunted Cemetery", [], 4.5, false, false, Realm),
    dungeon!("The Machine", [], 4.5, false, false, Realm),
    dungeon!("Davy Jones' Locker", [], 5.0, false, false, Realm),
    dungeon!("Ocean Trench", [], 5.0, false, false, Realm),
    dungeon!("The Crawling Depths", [], 5.5, false, false, Realm),
    dungeon!("Woodland Labyrinth", [], 5.5, false, false, Realm),
    dungeon!("Deadwater Docks", [], 5.5, false, false, Realm),
    dungeon!("Puppet Master's Encore", [], 5.5, false, false, Realm),
    dungeon!("Cnidarian Reef", [], 5.5, false, false, Realm),
    dungeon!("Parasite Chambers", [], 5.5, false, false, Realm),
    dungeon!("The Tavern", [], 5.5, false, false, Realm),
    dungeon!("Sulfurous Wetlands", [], 6.0, false, false, Realm),
    dungeon!("Mountain Temple", [], 6.0, false, false, Realm),
    dungeon!("Lair of Draconis", [], 6.0, false, false, Realm),
    dungeon!("Tomb of the Ancients", ["Tomb"], 6.0, false, false, Realm),
    dungeon!("The Third Dimension", [], 6.0, false, false, Realm),
    dungeon!("Lair of Shaitan", ["Shaitan"], 6.0, false, false, Realm),
    dungeon!("Secluded Thicket", [], 6.5, false, false, Realm),
    dungeon!("High Tech Terror", ["HTT"], 6.5, false, false, Realm),
    dungeon!("Ice Citadel", ["Ice Cave"], 7.0, true, false, Realm),
    dungeon!("Moonlight Village", ["MV"], 9.0, true, false, Realm),
    dungeon!("Beachzone", [], 0.0, false, false, RealmEvent),
    dungeon!("The Nest", [], 7.0, true, false, RealmEvent),
    dungeon!("Cultist Hideout", ["Cult"], 7.0, true, false, RealmEvent),
    dungeon!("Fungal Cavern", [], 7.5, false, true, RealmEvent),
    dungeon!("Crystal Cavern", [], 7.5, true, false, RealmEvent),
    dungeon!("Spectral Penitentiary", ["Spectral"], 8.0, true, false, RealmEvent),
    dungeon!("Kogbold Steamworks", [], 8.0, true, false, RealmEvent),
    dungeon!("Lost Halls", ["LH"], 8.0, true, false, RealmEvent),
    dungeon!("The Void", ["Void"], 8.5, true, false, RealmEvent),
    dungeon!("The Shatters", ["Shatts"], 10.0, true, false, RealmEvent),
    dungeon!("Heroic Undead Lair", [], 6.0, false, false, Advanced),
    dungeon!("Infernal Abyss of Demons", [], 6.0, false, false, Advanced),
    dungeon!("Plagued Nest", ["Advanced Nest", "Adv Nest"], 8.5, true, false, Advanced),
    dungeon!("Advanced Kogbold Steamworks", ["Adv Steamworks", "ASW"], 9.0, true, false, Advanced),
    dungeon!("Court of Oryx", [], 3.5, false, false, OryxCastle),
    dungeon!("Oryx's Castle", [], 4.5, false, false, OryxCastle),
    dungeon!("Oryx's Chamber", [], 4.5, false, false, OryxCastle),
    dungeon!("Wine Cellar", [], 6.0, false, false, OryxCastle),
    dungeon!("Oryx's Sanctuary", ["O3"], 9.5, true, false, OryxCastle),
    dungeon!("Malogia", [], 5.0, false, false, Wormhole),
    dungeon!("Untaris", [], 5.0, false, false, Wormhole),
    dungeon!("Katalund", [], 5.0, false, false, Wormhole),
    dungeon!("Forax", [], 5.0, false, false, Wormhole),
    dungeon!("Rainbow Road", [], 0.0, false, false, Seasonal),
    dungeon!("Santa's Workshop", [], 0.0, false, false, Seasonal),
    dungeon!("Battle for the Nexus", [], 5.0, false, false, Seasonal),
    dungeon!("Belladonna's Garden", [], 5.5, false, false, Seasonal),
    dungeon!("Queen Bunny Chamber", [], 5.5, false, false, Seasonal),
    dungeon!("Ice Tomb", [], 6.0, false, false, Seasonal),
    dungeon!("Mad God Mayhem", [], 6.0, false, false, Seasonal),
    dungeon!("White Snake Invasion I", [], 6.5, false, false, Seasonal),
    dungeon!("The Trials of Cronus", [], 6.5, false, false, Seasonal),
    dungeon!("Hidden Interregnum", [], 7.0, false, false, Seasonal),
    dungeon!("White Snake Invasion II", [], 7.5, false, false, Seasonal),
    dungeon!("Oryxmania", [], 8.0, false, false, Seasonal),
    dungeon!("White Snake Invasion III", [], 8.5, false, false, Seasonal),
 ];

pub fn dungeon_meta(name: &str) -> Option<&'static DungeonMeta> {
    DUNGEON_META.iter().find(|meta| {
        meta.name.eq_ignore_ascii_case(name)
            || meta
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

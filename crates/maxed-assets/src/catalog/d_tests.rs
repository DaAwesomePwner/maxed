use super::*;
use crate::rules;

fn node(body: &str) -> xml::Node {
    let mut result = None;
    xml::records(
        format!("<Objects><Object>{body}</Object></Objects>").as_bytes(),
        &mut |n| {
            result = Some(n);
            Ok(())
        },
    )
    .unwrap();
    result.unwrap()
}

#[test]
fn parse_item_stat_bonuses_from_activate_on_equip() {
    let n = node(
        r#"
        <ActivateOnEquip stat="21" amount="4">IncrementStat</ActivateOnEquip>
        <ActivateOnEquip stat="DEFENSE" amount="2">IncrementStat</ActivateOnEquip>
        <ActivateOnEquip stat="HP" amount="20">IncrementStat</ActivateOnEquip>
        <ActivateOnEquip stat="28" amount="-1">IncrementStat</ActivateOnEquip>
        <ActivateOnEquip stat="UNKNOWN" amount="4">IncrementStat</ActivateOnEquip>
        <ActivateOnEquip stat="99" amount="4">IncrementStat</ActivateOnEquip>
        <ActivateOnEquip stat="21" amount="900">StatModMult</ActivateOnEquip>
        <Mutators><ActivateOnEquip stat="21" amount="900">IncrementStat</ActivateOnEquip></Mutators>
    "#,
    );
    let (bonuses, unknown) = parse_item_stat_bonuses(&n).unwrap();
    assert_eq!(bonuses, [(0, 20), (21, 6), (28, -1)].into());
    assert_eq!(unknown, 2);
    let (empty, unknown) = parse_item_stat_bonuses(&node("")).unwrap();
    assert!(empty.is_empty());
    assert_eq!(unknown, 0);
}

#[test]
fn stat_bonuses_reject_malformed_and_overflowing_amounts() {
    for amount in ["NaN", "inf", "1.5", "2147483648", "bad"] {
        let n = node(&format!(
            r#"<ActivateOnEquip stat="21" amount="{amount}">IncrementStat</ActivateOnEquip>"#
        ));
        assert!(matches!(
            parse_item_stat_bonuses(&n),
            Err(AssetError::InvalidCatalog)
        ));
    }
    let n = node(
        r#"<ActivateOnEquip stat="21" amount="2147483647">IncrementStat</ActivateOnEquip><ActivateOnEquip stat="DEF" amount="1">IncrementStat</ActivateOnEquip>"#,
    );
    assert!(parse_item_stat_bonuses(&n).is_err());
}

#[test]
fn soulbound_tag_presence() {
    assert!(parse_soulbound(&node("<Soulbound/>")));
    assert!(!parse_soulbound(&node("")));
    assert!(!parse_soulbound(&node("<Nested><Soulbound/></Nested>")));
}

#[test]
fn portal_dungeon_name_and_class() {
    let n = node("<Class>Portal</Class><DungeonName> Synthetic Cave </DungeonName>");
    assert_eq!(parse_object_class(&n), Some("Portal"));
    assert_eq!(parse_dungeon_name(&n), Some("Synthetic Cave"));
    assert_eq!(
        parse_dungeon_name(&node(
            "<Class>Equipment</Class><DungeonName>Cave</DungeonName>"
        )),
        None
    );
    assert_eq!(
        parse_dungeon_name(&node("<Class>Portal</Class><DungeonName/>")),
        None
    );
    assert_eq!(parse_object_class(&node("")), None);
}

#[test]
fn event_boss_list_resolves_by_exact_name() {
    let names = [
        "Grand Sphinx",
        "Hermit God",
        "Lord of the Lost Lands",
        "Cube God",
        "Skull Shrine",
        "Ghost Ship",
        "Avatar of the Forgotten King",
        "Oasis Giant",
        "Phoenix Lord",
        "Ghost King",
        "Rock Dragon",
        "Pentaract Tower",
        "Crystal Prisoner",
    ];
    assert_eq!(rules::event_bosses(), names);
    for name in names {
        assert!(parse_event_boss(
            &node(&format!("<DisplayId>{name}</DisplayId>")),
            &BTreeMap::new()
        )
        .unwrap());
        assert!(!parse_event_boss(
            &node(&format!("<DisplayId>{name} Minion</DisplayId>")),
            &BTreeMap::new()
        )
        .unwrap());
    }
    assert!(!parse_event_boss(
        &node("<DisplayId>grand sphinx</DisplayId>"),
        &BTreeMap::new()
    )
    .unwrap());
    let mut n = node("");
    n.attrs.insert("id".into(), "Grand Sphinx".into());
    assert!(parse_event_boss(&n, &BTreeMap::new()).unwrap());
}

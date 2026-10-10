use maxed_assets::parse_equipment;

fn tier(object_type: u16, name: &str, slot: u8, tags: &str) -> Option<String> {
    let xml = format!(
        r#"<Objects><Object type="{object_type}" id="{name}"><Class>Equipment</Class><SlotType>{slot}</SlotType>{tags}</Object></Objects>"#
    );
    parse_equipment(xml.as_bytes()).unwrap().remove(0).tier
}

#[test]
fn greater_health_potion_drops_xml_ut_label() {
    assert_eq!(
        tier(2795, "Greater Health Potion", 10, "<Labels>UT</Labels>"),
        None
    );
}

#[test]
fn consumable_drops_xml_st_label() {
    assert_eq!(
        tier(1, "Synthetic Consumable", 10, "<Labels>ST</Labels>"),
        None
    );
}

#[test]
fn equipment_keeps_xml_ut_and_st_labels() {
    assert_eq!(
        tier(3109, "Staff of the Cosmic Whole", 17, "<Labels>UT</Labels>"),
        Some("UT".into())
    );
    assert_eq!(
        tier(1, "Synthetic Equipment", 9, "<Labels>ST</Labels>"),
        Some("ST".into())
    );
}

#[test]
fn blueprint_suffix_assigns_ut_without_xml_label() {
    for name in [
        "Kogbold Multitool Blueprint",
        "Shaitan Combo Blueprint",
        "Common Blueprint",
        "Greater Blueprint",
        "Superior Blueprint",
        "Paramount Blueprint",
        "Exalted Blueprint",
    ] {
        assert_eq!(tier(1, name, 10, ""), Some("UT".into()), "{name}");
    }
}

#[test]
fn vial_of_soul_extract_assigns_ut() {
    assert_eq!(
        tier(5338, "Vial of Soul Extract", 10, ""),
        Some("UT".into())
    );
}

#[test]
fn alternate_vial_of_soul_extract_assigns_ut() {
    assert_eq!(
        tier(23743, "Vial of Soul Extract", 10, ""),
        Some("UT".into())
    );
}

#[test]
fn kogbold_enhancement_core_assigns_ut() {
    assert_eq!(
        tier(49473, "Kogbold Enhancement Core", 10, ""),
        Some("UT".into())
    );
}

#[test]
fn shiny_kogbold_enhancement_core_assigns_ut() {
    assert_eq!(
        tier(3984, "Shiny Kogbold Enhancement Core", 10, ""),
        Some("UT".into())
    );
}

#[test]
fn allowlist_only_matches_consumables_and_exact_suffixes() {
    for name in [
        "Blueprint",
        "Blueprint Fragment",
        "Common Blueprint Fragment",
        "Common blueprint",
    ] {
        assert_eq!(tier(1, name, 10, "<Labels>UT</Labels>"), None, "{name}");
    }
    assert_eq!(tier(1, "Common Blueprint", 9, ""), None);
    assert_eq!(
        tier(49473, "Synthetic Equipment", 9, "<Labels>ST</Labels>"),
        Some("ST".into())
    );
}

#[test]
fn numeric_tiers_take_precedence_over_labels_and_allowlist() {
    for (object_type, name, slot) in [
        (2793, "Potion of Life", 10),
        (49473, "Kogbold Enhancement Core", 10),
        (1, "Common Blueprint", 10),
        (2806, "Wand of Recompense", 8),
    ] {
        assert_eq!(
            tier(
                object_type,
                name,
                slot,
                "<Tier>12</Tier><Labels>UT</Labels>"
            ),
            Some("12".into())
        );
    }
}

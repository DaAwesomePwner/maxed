use maxed_assets::{build_catalog, parse_equipment, validate_catalog, ExtractedAssets, TextAsset};

fn assets(xml: &[u8]) -> ExtractedAssets {
    ExtractedAssets {
        english: Default::default(),
        texts: vec![TextAsset {
            name: "synthetic".into(),
            bytes: xml.to_vec(),
        }],
        textures: vec![],
        sprite_mapping: vec![],
    }
}

#[test]
fn enchant_stat_damage_mutators_use_java_float_products_and_ignore_unrelated_effects() {
    let xml = br#"<Enchantments>
      <Enchantment type="123" id="Synthetic"><Mutators>
        <ActivateOnEquip amount="1.25">StatModMult</ActivateOnEquip>
        <ActivateOnEquip amount="900">LootBonus</ActivateOnEquip>
        <MultiplyMinDamage>7</MultiplyMinDamage>
        <ActivateOnEquip amount="1.1">StatModMult</ActivateOnEquip>
        <ActivateOnEquip amount="invalid">StatModMult</ActivateOnEquip>
      </Mutators></Enchantment>
      <Enchantment type="124" id="Neutral"/>
    </Enchantments>"#;
    let c = build_catalog(assets(xml), "a".repeat(64)).unwrap();
    assert_eq!(
        c.enchants[0].stat_damage_multiplier,
        Some(f64::from(1.25f32 * 1.1f32))
    );
    assert_eq!(c.enchants[1].stat_damage_multiplier, Some(1.0));
    let round = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
    assert_eq!(c, round);
    for amount in ["NaN", "inf", "-1"] {
        let bad = format!(
            r#"<Enchantments><Enchantment type="1" id="Synthetic"><Mutators><ActivateOnEquip amount="{amount}">StatModMult</ActivateOnEquip></Mutators></Enchantment></Enchantments>"#
        );
        assert!(build_catalog(assets(bad.as_bytes()), "a".repeat(64)).is_err());
    }
}

#[test]
fn equipment_and_unknown_arithmetic() {
    let xml=br#"<Objects><Object type="0x100" id="Demo Wand"><Class>Equipment</Class><SlotType>8</SlotType><Projectile id="2"><MinDamage>100</MinDamage><MaxDamage>110</MaxDamage><ArmorPiercing/></Projectile><Projectile id="3"><MinDamage>5</MinDamage></Projectile></Object></Objects>"#;
    let items = parse_equipment(xml).unwrap();
    assert_eq!(items[0].object_type, 256);
    assert_eq!(items[0].slot_type, Some(8));
    assert_eq!(items[0].projectiles.len(), 1);
    assert_eq!(items[0].projectiles[0].index, 2);
    assert_eq!(items[0].projectiles[0].min_damage, 100);
    assert_eq!(items[0].projectiles[0].max_damage, 110);
    assert!(items[0].projectiles[0].armor_piercing);
}

#[test]
fn class_caps_and_boss_tags_are_distinct() {
    let c=build_catalog(assets(br#"<Objects><Object type="1" id="Player"><Class>Player</Class><MaxHitPoints max="750">100</MaxHitPoints></Object><Object type="2" id="Boss"><Labels>BOSS</Labels></Object><Object type="3" id="Mini"><Labels>MINIBOSS</Labels></Object></Objects>"#), "a".repeat(64)).unwrap();
    assert_eq!(c.classes[0].stat_caps[0].value, 750);
    assert!(c.objects[1].is_boss);
    assert!(!c.objects[2].is_boss);
    assert!(c.objects[2].is_miniboss);
    validate_catalog(&c).unwrap();
    let round = serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
    assert_eq!(c, round);
}

#[test]
fn rejects_xml_entities_depth_duplicates_and_invalid_math() {
    for xml in [br#"<!DOCTYPE x [<!ENTITY e SYSTEM "file:///secret">]><Objects/>"#.as_slice(), br#"<Objects><Object type="1" id="x"/><Object type="1" id="x"/></Objects>"#, br#"<Objects><Object type="1" id="x"><Class>Equipment</Class><Projectile><MinDamage>10</MinDamage><MaxDamage>2</MaxDamage></Projectile></Object></Objects>"#] {
        assert!(build_catalog(assets(xml),"a".repeat(64)).is_err());
    }
    let nested = format!("{}{}", "<x>".repeat(65), "</x>".repeat(65));
    assert!(build_catalog(assets(nested.as_bytes()), "a".repeat(64)).is_err());
}

#[test]
fn reference_override_order_scaling_origin_and_unknown_sprite() {
    let mut input=assets(br#"<Objects><Object type="1" id="Weapon"><Class>Equipment</Class><Activate scalingStat="WIS" statModDamage="2" statModScalingMin="40" numShots="3"/><OnPlayerShootActivate type="0x2"/></Object><Object type="2" id="Proc"><Class>Projectile</Class><Texture><File>absent</File><Index>4</Index></Texture></Object></Objects>"#);
    input.texts[0].name = "a".into();
    input.texts.push(TextAsset {
        name: "z".into(),
        bytes: br#"<Objects><Object type="3" id="Final"/></Objects>"#.to_vec(),
    });
    input.texts.insert(
        0,
        TextAsset {
            name: "b".into(),
            bytes: br#"<Objects><Object type="3" id="Earlier"/></Objects>"#.to_vec(),
        },
    );
    let c = build_catalog(input, "a".repeat(64)).unwrap();
    assert_eq!(c.objects[2].name, "Final");
    assert!(c.items[1].sprite_key.is_none());
    let scaling = c.items[1].scaling.as_ref().unwrap();
    assert_eq!(scaling.origin_item, Some(1));
    assert_eq!(scaling.shots, 3);
    assert_eq!(scaling.minimum, 40);
    assert_eq!(scaling.stat_id, 27);
    let white = c.bags.iter().find(|b| b.object_type == 1296).unwrap();
    assert!(white.is_white);
    assert_eq!(white.base_type, 1292);
    assert_eq!(white.rank, Some(255));
    assert_eq!(
        c.dungeons
            .iter()
            .find(|d| d.name == "The Shatters")
            .unwrap()
            .completion_index,
        Some(63)
    );
    assert!(c.crucible_bonuses.is_empty());
}

#[test]
fn unsupported_wire_types_and_fractional_lifetimes_stay_unavailable() {
    let c=build_catalog(assets(br#"<Objects><Object type="0xc283a" id="Editor-only"/><Object type="1" id="Weapon"><Class>Equipment</Class><Projectile><Damage>10</Damage><LifetimeMS>357.142857</LifetimeMS></Projectile></Object></Objects>"#),"a".repeat(64)).unwrap();
    assert_eq!(c.objects.len(), 1);
    assert_eq!(c.items[0].projectiles[0].lifetime_millis, None);
}

#[test]
fn validation_rejects_paths_nonfinite_and_unknown_references() {
    let base = build_catalog(assets(b"<Objects/>"), "a".repeat(64)).unwrap();
    let mut c = base.clone();
    c.sprites.push(maxed_types::SpriteRef {
        key: "a".into(),
        sheet: "../outside.png".into(),
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        trim: None,
    });
    assert!(validate_catalog(&c).is_err());
    let mut c = base.clone();
    c.crucible_bonuses.push(maxed_types::CrucibleDef {
        id: "test".into(),
        damage_multiplier: f64::INFINITY,
    });
    assert!(validate_catalog(&c).is_err());
    let mut c = base;
    c.objects.push(maxed_types::ObjectDef {
        class_name: None,
        dungeon_name: None,
        event_boss: None,
        realm_event_key: None,
        object_type: 1,
        name: "Unknown sprite".into(),
        is_player: false,
        is_boss: false,
        is_miniboss: false,
        sprite_key: Some("missing:0".into()),
    });
    assert!(validate_catalog(&c).is_err());
}

#[test]
fn nested_scaling_and_proc_references_follow_dom_order() {
    let xml = br#"<Objects><Object type="1" id="Skull"><Class>Equipment</Class>
      <OnConditionEndActivate scalingStat="ATT" statModDamage="9"/>
      <Ability><Inner><Activate scalingStat="WIS" statModDamage="3" statModScalingMin="55"/></Inner>
        <OnPlayerShootActivate type="0x2"/><Activate type="0x3"/></Ability>
      <Activate scalingStat="DEF" statModDamage="7"/>
    </Object><Object type="2" id="Proc"><Class>Projectile</Class></Object>
    <Object type="3" id="Missile"><Class>Projectile</Class></Object></Objects>"#;
    let c = build_catalog(assets(xml), "a".repeat(64)).unwrap();
    for (i, item) in c.items.iter().enumerate() {
        let scaling = item.scaling.as_ref().unwrap();
        assert_eq!(scaling.stat_id, 27);
        assert_eq!(scaling.damage_per_stat, 3.0);
        assert_eq!(scaling.minimum, 55);
        assert_eq!(scaling.origin_item, (i != 0).then_some(1));
    }
}

#[test]
fn nonpositive_scaling_does_not_hide_later_valid_elements() {
    let xml = br#"<Objects><Object type="1" id="Later"><Class>Equipment</Class>
      <Activate scalingStat="DEF" statModDamage="0"/>
      <Ability><Activate scalingStat="ATT" statModDamage="-3"/>
      <Activate scalingStat="WIS" statModDamage="2"/></Ability>
    </Object><Object type="2" id="Fallback"><Class>Equipment</Class>
      <Activate scalingStat="DEF" statModDamage="0"/>
      <Ability><OnConditionEndActivate scalingStat="ATT" statModDamage="4"/></Ability>
    </Object><Object type="3" id="None"><Class>Equipment</Class>
      <Activate scalingStat="DEF" statModDamage="-1"/>
    </Object></Objects>"#;
    let items = parse_equipment(xml).unwrap();
    assert_eq!(items[0].scaling.as_ref().unwrap().damage_per_stat, 2.0);
    assert_eq!(items[1].scaling.as_ref().unwrap().damage_per_stat, 4.0);
    assert!(items[2].scaling.is_none());
}

#[test]
fn duplicate_errors_identify_only_safe_sections() {
    for (tag, section) in [
        ("Object", "objects"),
        ("Ground", "ground"),
        ("Enchantment", "enchantments"),
    ] {
        let xml = format!(
            "<root><{tag} type='42' id='PRIVATE_XML'/><{tag} type='42' id='PRIVATE_XML'/></root>"
        );
        let error = build_catalog(assets(xml.as_bytes()), "a".repeat(64)).unwrap_err();
        assert!(
            matches!(error, maxed_assets::AssetError::InvalidXmlSection { section: s } if s == section)
        );
        assert!(error.to_string().contains(section));
        assert!(!error.to_string().contains("PRIVATE_XML"));
    }
    let error = parse_equipment(br#"<Objects><Object type="42" id="secret"><Class>Equipment</Class></Object><Object type="42" id="secret"><Class>Equipment</Class></Object></Objects>"#).unwrap_err();
    assert!(matches!(
        error,
        maxed_assets::AssetError::InvalidXmlSection {
            section: "equipment"
        }
    ));
}

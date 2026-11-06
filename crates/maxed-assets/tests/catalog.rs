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

fn character_catalog(body: &str) -> maxed_types::Catalog {
    build_catalog(assets(body.as_bytes()), "a".repeat(64)).unwrap()
}

fn firing(body: &str) -> maxed_types::ItemDef {
    parse_equipment(format!(r#"<Objects><Object type="1" id="Synthetic"><Class>Equipment</Class>{body}</Object></Objects>"#).as_bytes())
        .unwrap().remove(0)
}

#[test]
fn firing_fixed_ranged_counts_inheritance_and_overrides() {
    use maxed_types::{CatalogSupport::Supported, WeaponSchedule::Simultaneous};
    let item = firing(
        r#"<RateOfFire>0.8</RateOfFire><NumProjectiles>9</NumProjectiles>
      <Subattack projectileId="0"><NumProjectiles>2</NumProjectiles></Subattack>
      <Subattack projectileId="3"><RateOfFire>1.25</RateOfFire><NumProjectiles>4</NumProjectiles></Subattack>
      <Projectile id="0"><Damage>100</Damage><LifetimeMS>12.5</LifetimeMS></Projectile>
      <Projectile id="3"><MinDamage>20</MinDamage><MaxDamage>30</MaxDamage><ArmorPiercing/></Projectile>"#,
    );
    let f = item.weapon_firing.unwrap();
    assert_eq!(f.support, Supported);
    assert_eq!(f.groups.len(), 2);
    assert_eq!(
        (
            f.groups[0].projectile_index,
            f.groups[0].count,
            f.groups[0].rate_factor
        ),
        (0, 2, 0.8)
    );
    assert_eq!(
        (
            f.groups[1].projectile_index,
            f.groups[1].count,
            f.groups[1].rate_factor
        ),
        (3, 4, 1.25)
    );
    assert!(f.groups.iter().all(|g| g.schedule == Simultaneous));
    assert!(f.reasons.is_empty());
    assert_eq!(item.projectiles[0].min_damage, 100);
    assert_eq!(item.projectiles[0].max_damage, 100);
    assert_eq!(item.projectiles[0].lifetime_millis, None);
    assert_eq!(item.projectiles[1].min_damage, 20);
    assert_eq!(item.projectiles[1].max_damage, 30);
    assert!(item.projectiles[1].armor_piercing);
}

#[test]
fn firing_normal_defaults_do_not_use_scaling_or_list_length() {
    for (count, expected) in [("", 1), ("<NumProjectiles>3</NumProjectiles>", 3)] {
        let item = firing(&format!(
            r#"<RateOfFire>1</RateOfFire>{count}
          <Activate scalingStat="WIS" statModDamage="2" numShots="7"/>
          <Projectile id="0"><Damage>9</Damage></Projectile>"#
        ));
        // Ability activation is outside the reviewed normal-fire shape, but its shots cannot change count.
        assert_eq!(item.weapon_firing.unwrap().groups[0].count, expected);
        assert_eq!(item.scaling.unwrap().shots, 7);
    }
    let plain =
        firing(r#"<RateOfFire>1</RateOfFire><Projectile id="0"><Damage>9</Damage></Projectile>"#);
    assert_eq!(
        plain.weapon_firing.unwrap().support,
        maxed_types::CatalogSupport::Supported
    );
    for body in [
        r#"<Projectile id="0"><Damage>9</Damage></Projectile>"#,
        r#"<RateOfFire>1</RateOfFire><Projectile><Damage>9</Damage></Projectile>"#,
        r#"<RateOfFire>1</RateOfFire><Subattack projectileId="0"/><Projectile id="0"><Damage>9</Damage></Projectile>"#,
        r#"<RateOfFire>1</RateOfFire><Subattack><NumProjectiles>1</NumProjectiles></Subattack><Projectile id="0"><Damage>9</Damage></Projectile>"#,
    ] {
        let f = firing(body).weapon_firing.unwrap();
        assert_eq!(f.support, maxed_types::CatalogSupport::Unsupported);
        assert!(!f.reasons.is_empty());
    }
    assert!(firing("").weapon_firing.is_none());
}

#[test]
fn firing_missing_and_skipped_projectiles_keep_only_a_partial_subtotal() {
    for second in [
        "",
        r#"<Projectile id="2"><MinDamage>4</MinDamage></Projectile>"#,
    ] {
        let item = firing(&format!(
            r#"<RateOfFire>1</RateOfFire>
          <Subattack projectileId="0"><NumProjectiles>1</NumProjectiles></Subattack>
          <Subattack projectileId="2"><NumProjectiles>1</NumProjectiles></Subattack>
          <Projectile id="0"><Damage>9</Damage></Projectile>{second}"#
        ));
        assert_eq!(item.projectiles.len(), 1);
        let f = item.weapon_firing.unwrap();
        assert_eq!(f.support, maxed_types::CatalogSupport::Partial);
        assert_eq!(f.omitted_projectile_indices, [2]);
        assert_eq!(f.groups.len(), 1);
        assert!(!f.reasons.is_empty());
    }
    for second in [
        r#"<Projectile id="2"><Damage>4</Damage></Projectile>"#,
        r#"<Projectile id="2"><MinDamage>4</MinDamage></Projectile>"#,
    ] {
        let f = firing(&format!(r#"<RateOfFire>1</RateOfFire><Projectile id="0"><Damage>9</Damage></Projectile>{second}"#)).weapon_firing.unwrap();
        assert_eq!(f.support, maxed_types::CatalogSupport::Partial);
        assert_eq!(f.omitted_projectile_indices, [2]);
    }
}

#[test]
fn firing_duplicate_indices_fail_even_when_one_has_no_damage() {
    for projectiles in [
        r#"<Projectile id="0"/><Projectile id="0"><Damage>1</Damage></Projectile>"#,
        r#"<Projectile id="0"><Damage>1</Damage></Projectile><Projectile id="0"/>"#,
        r#"<Projectile id="0"/><Projectile id="0"/>"#,
    ] {
        let xml = format!(
            r#"<Objects><Object type="1" id="Synthetic"><Class>Equipment</Class>{projectiles}</Object></Objects>"#
        );
        assert!(parse_equipment(xml.as_bytes()).is_err());
    }
}

#[test]
fn firing_unreviewed_schedules_never_become_simultaneous() {
    for special in [
        "<Alternating/>",
        "<Charged/>",
        "<Burst/>",
        "<Conditional/>",
        "<FutureSchedule/>",
    ] {
        let f = firing(&format!(r#"<RateOfFire>1</RateOfFire>{special}<Projectile id="0"><Damage>9</Damage></Projectile>"#)).weapon_firing.unwrap();
        assert_eq!(f.support, maxed_types::CatalogSupport::Unsupported);
        assert!(f
            .groups
            .iter()
            .all(|g| g.schedule == maxed_types::WeaponSchedule::Unsupported));
        assert!(!f.reasons.is_empty());
    }
    let f = firing(r#"<RateOfFire>1</RateOfFire>
      <Subattack projectileId="0"><NumProjectiles>1</NumProjectiles></Subattack>
      <Subattack projectileId="1" condition="PRIVATE"><NumProjectiles>1</NumProjectiles></Subattack>
      <Projectile id="0"><Damage>1</Damage></Projectile><Projectile id="1"><Damage>2</Damage></Projectile>"#).weapon_firing.unwrap();
    assert_eq!(f.support, maxed_types::CatalogSupport::Partial);
    assert_eq!(
        f.groups[1].schedule,
        maxed_types::WeaponSchedule::Unsupported
    );
    assert!(!format!("{f:?}").contains("PRIVATE"));
}

#[test]
fn firing_invalid_rates_counts_and_duplicate_fields_are_unknown() {
    for rate in ["0", "-1", "NaN", "inf", "invalid"] {
        let f = firing(&format!(
            r#"<RateOfFire>{rate}</RateOfFire><Projectile id="0"><Damage>1</Damage></Projectile>"#
        ))
        .weapon_firing
        .unwrap();
        assert_eq!(f.support, maxed_types::CatalogSupport::Unsupported);
        assert!(f.groups.is_empty());
    }
    for count in ["0", "257", "-1", "1.5", "invalid"] {
        let f = firing(&format!(r#"<RateOfFire>1</RateOfFire><NumProjectiles>{count}</NumProjectiles><Projectile id="0"><Damage>1</Damage></Projectile>"#)).weapon_firing.unwrap();
        assert_eq!(f.support, maxed_types::CatalogSupport::Unsupported);
    }
    let f = firing(r#"<RateOfFire>1</RateOfFire><RateOfFire>2</RateOfFire><Projectile id="0"><Damage>1</Damage></Projectile>"#).weapon_firing.unwrap();
    assert_ne!(f.support, maxed_types::CatalogSupport::Supported);
    let many = format!(
        r#"<Objects><Object type="1" id="Synthetic"><RateOfFire>1</RateOfFire>{}<Projectile id="0"><Damage>1</Damage></Projectile></Object></Objects>"#,
        r#"<Subattack projectileId="0"><NumProjectiles>1</NumProjectiles></Subattack>"#.repeat(257)
    );
    assert!(parse_equipment(many.as_bytes()).is_err());
}

fn regen(mutators: &str) -> maxed_types::EnchantRegenDef {
    character_catalog(&format!(r#"<Enchantments><Enchantment type="1" id="Synthetic"><Mutators>{mutators}</Mutators></Enchantment></Enchantments>"#))
        .enchants.remove(0).regeneration.unwrap()
}

#[test]
fn regen_flat_and_fractional_units_are_per_second_without_percent_conversion() {
    let r = regen(
        r#"<ActivateOnEquip stat="HP" amount="2.5">FlatRegen</ActivateOnEquip>
      <ActivateOnEquip stat="MP" amount="0.02">PercentageRegen</ActivateOnEquip>"#,
    );
    assert_eq!(r.support, maxed_types::CatalogSupport::Supported);
    assert_eq!(r.hp[0].flat_per_sec, Some(2.5));
    assert_eq!(r.hp[0].fraction_of_max_per_sec, None);
    assert_eq!(r.mp[0].fraction_of_max_per_sec, Some(0.02));
    assert_eq!(r.mp[0].flat_per_sec, None);
    assert_eq!(r.hp[0].source_units, ["units_per_second"]);
    assert_eq!(r.mp[0].source_units, ["fraction_of_max_per_second"]);
    assert_eq!(r.source_units.len(), 2);
    assert!(r.reasons.is_empty());
}

#[test]
fn regen_empty_is_supported_but_unknown_or_interval_effects_are_not_zero() {
    for mutators in [
        "",
        "<MultiplyRateOfFire>1.1</MultiplyRateOfFire>",
        r#"<ActivateOnEquip amount="5">LootBonus</ActivateOnEquip>"#,
    ] {
        let r = regen(mutators);
        assert_eq!(r.support, maxed_types::CatalogSupport::Supported);
        assert!(r.hp.is_empty() && r.mp.is_empty());
    }
    for effect in [
        r#"<ActivateOnEquip stat="HP" amount="4" interval="2">FlatRegen</ActivateOnEquip>"#,
        r#"<ActivateOnEquip stat="HP" amount="4" intervalMS="1000">FlatRegen</ActivateOnEquip>"#,
        r#"<ActivateOnEquip stat="HP" amount="4">FutureRegen</ActivateOnEquip>"#,
        r#"<ActivateOnEquip stat="HEALTH" amount="4">FlatRegen</ActivateOnEquip>"#,
        r#"<ActivateOnEquip stat="HP">FlatRegen</ActivateOnEquip>"#,
        r#"<Conditional><ActivateOnEquip stat="HP" amount="4">FlatRegen</ActivateOnEquip></Conditional>"#,
    ] {
        let r = regen(effect);
        assert_eq!(r.support, maxed_types::CatalogSupport::Unsupported);
        assert!(r.hp.is_empty() && r.mp.is_empty());
        assert!(!r.reasons.is_empty());
        let partial = regen(&format!(
            r#"{effect}<ActivateOnEquip stat="MP" amount="3">FlatRegen</ActivateOnEquip>"#
        ));
        assert_eq!(partial.support, maxed_types::CatalogSupport::Partial);
        assert_eq!(partial.mp[0].flat_per_sec, Some(3.0));
    }
    for amount in ["NaN", "inf", "-1", "bad"] {
        assert_eq!(
            regen(&format!(
                r#"<ActivateOnEquip stat="HP" amount="{amount}">FlatRegen</ActivateOnEquip>"#
            ))
            .support,
            maxed_types::CatalogSupport::Unsupported
        );
    }
}

#[test]
fn character_catalog_validation_rejects_invalid_metadata_and_preserves_legacy_absence() {
    let mut c = character_catalog(
        r#"<root><Object type="1" id="Synthetic"><Class>Equipment</Class>
      <RateOfFire>1</RateOfFire><Projectile id="0"><Damage>1</Damage></Projectile></Object>
      <Enchantment type="1" id="Synthetic"><Mutators><ActivateOnEquip stat="HP" amount="2">FlatRegen</ActivateOnEquip></Mutators></Enchantment></root>"#,
    );
    let mut json = serde_json::to_value(&c).unwrap();
    json["items"][0]
        .as_object_mut()
        .unwrap()
        .remove("weapon_firing");
    json["enchants"][0]
        .as_object_mut()
        .unwrap()
        .remove("regeneration");
    let old: maxed_types::Catalog = serde_json::from_value(json).unwrap();
    assert!(old.items[0].weapon_firing.is_none());
    assert!(old.enchants[0].regeneration.is_none());
    validate_catalog(&old).unwrap();
    for value in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        let mut bad = c.clone();
        bad.items[0].weapon_firing.as_mut().unwrap().groups[0].rate_factor = value;
        assert!(validate_catalog(&bad).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, -1.0] {
        let mut bad = c.clone();
        bad.enchants[0].regeneration.as_mut().unwrap().hp[0].flat_per_sec = Some(value);
        assert!(validate_catalog(&bad).is_err());
    }
    for count in [0, 257] {
        let mut bad = c.clone();
        bad.items[0].weapon_firing.as_mut().unwrap().groups[0].count = count;
        assert!(validate_catalog(&bad).is_err());
    }
    let mut bad = c.clone();
    bad.items[0].weapon_firing.as_mut().unwrap().groups[0].projectile_index = 3;
    assert!(validate_catalog(&bad).is_err());
    let mut bad = c.clone();
    bad.items[0]
        .weapon_firing
        .as_mut()
        .unwrap()
        .omitted_projectile_indices
        .push(1);
    assert!(validate_catalog(&bad).is_err());
    let mut bad = c.clone();
    bad.items[0].weapon_firing.as_mut().unwrap().groups[0].schedule =
        maxed_types::WeaponSchedule::Unsupported;
    assert!(validate_catalog(&bad).is_err());
    let mut bad = c.clone();
    bad.enchants[0].regeneration.as_mut().unwrap().hp[0].source_units = vec!["unknown".into()];
    assert!(validate_catalog(&bad).is_err());
    c.enchants[0].regeneration.as_mut().unwrap().hp[0].flat_per_sec = None;
    assert!(validate_catalog(&c).is_err());
}

#[test]
fn complete_firing_metadata_cannot_hide_an_unreferenced_projectile_on_reload() {
    let mut c = character_catalog(
        r#"<Objects><Object type="1" id="Synthetic">
      <Class>Equipment</Class><RateOfFire>1</RateOfFire>
      <Subattack projectileId="0"><NumProjectiles>1</NumProjectiles></Subattack>
      <Subattack projectileId="1"><NumProjectiles>1</NumProjectiles></Subattack>
      <Projectile id="0"><Damage>1</Damage></Projectile>
      <Projectile id="1"><Damage>2</Damage></Projectile></Object></Objects>"#,
    );
    let round: maxed_types::Catalog =
        serde_json::from_slice(&serde_json::to_vec(&c).unwrap()).unwrap();
    validate_catalog(&round).unwrap();
    c.items[0].weapon_firing.as_mut().unwrap().groups.pop();
    assert!(validate_catalog(&c).is_err());
}

#[test]
fn enriches_dungeons_with_curated_metadata() {
    use maxed_types::DungeonCategory;
    let c = build_catalog(assets(br#"<Objects>
        <Object type="1" id="Ice"><Class>Portal</Class><DungeonName>Ice Cave</DungeonName><DungeonPortal/></Object>
        <Object type="2" id="Advanced"><Class>Portal</Class><DungeonName>Advanced Nest</DungeonName><DungeonPortal/></Object>
        <Object type="3" id="Unknown"><Class>Portal</Class><DisplayId>ASW</DisplayId><DungeonName>Uncharted</DungeonName><DungeonPortal/></Object>
    </Objects>"#), "a".repeat(64)).unwrap();
    let ice = c.dungeons.iter().find(|d| d.name == "Ice Cave").unwrap();
    assert_eq!(ice.difficulty, Some(7.0));
    assert!(ice.exalt);
    assert_eq!(ice.category, Some(DungeonCategory::Realm));
    let advanced = c
        .dungeons
        .iter()
        .find(|d| d.name == "Advanced Nest")
        .unwrap();
    assert_eq!(advanced.difficulty, Some(8.5));
    assert!(advanced.exalt);
    assert_eq!(advanced.category, Some(DungeonCategory::Advanced));
    let fungal = c
        .dungeons
        .iter()
        .find(|d| d.name == "Fungal Cavern")
        .unwrap();
    assert!(fungal.exalt_gateway);
    assert!(!fungal.exalt);
    let unknown = c.dungeons.iter().find(|d| d.name == "Uncharted").unwrap();
    assert!(unknown.aliases.iter().any(|alias| alias == "ASW"));
    assert_eq!(unknown.difficulty, None);
    assert_eq!(unknown.category, None);
    for dungeon in &c.dungeons {
        let meta = maxed_types::dungeon_meta(&dungeon.name);
        assert_eq!(dungeon.difficulty, meta.map(|meta| meta.difficulty));
        assert_eq!(dungeon.category, meta.map(|meta| meta.category));
        assert_eq!(dungeon.exalt, meta.is_some_and(|meta| meta.exalt));
        assert_eq!(
            dungeon.exalt_gateway,
            meta.is_some_and(|meta| meta.exalt_gateway)
        );
        // Existing reviewed names must retain metadata without a catalog-only alias.
        if dungeon.name != "Uncharted" {
            assert!(
                meta.is_some()
                    || dungeon
                        .aliases
                        .iter()
                        .all(|alias| maxed_types::dungeon_meta(alias).is_none())
            );
        }
    }
}

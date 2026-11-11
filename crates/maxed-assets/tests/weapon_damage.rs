//! Synthetic enchant weapon damage-range mutators (D-weapon-enchant).
use maxed_assets::{build_catalog, validate_catalog, ExtractedAssets, TextAsset};
use maxed_types::{Catalog, CatalogSupport, EnchantWeaponDamageDef};

fn catalog(enchants: &str) -> Catalog {
    let assets = ExtractedAssets {
        english: Default::default(),
        texts: vec![TextAsset {
            name: "synthetic".into(),
            bytes: format!("<Enchantments>{enchants}</Enchantments>").into_bytes(),
        }],
        textures: vec![],
        sprite_mapping: vec![],
    };
    build_catalog(assets, "a".repeat(64)).unwrap()
}

fn one(mutators: &str) -> EnchantWeaponDamageDef {
    catalog(&format!(
        r#"<Enchantment type="1" id="Synthetic"><Mutators>{mutators}</Mutators></Enchantment>"#
    ))
    .enchants
    .remove(0)
    .weapon_damage
    .unwrap()
}

fn supported(min: f32, max: f32) -> EnchantWeaponDamageDef {
    EnchantWeaponDamageDef {
        support: CatalogSupport::Supported,
        min_multiplier: Some(f64::from(min)),
        max_multiplier: Some(f64::from(max)),
        reasons: vec![],
    }
}

fn unsupported(reasons: &[&str]) -> EnchantWeaponDamageDef {
    EnchantWeaponDamageDef {
        support: CatalogSupport::Unsupported,
        min_multiplier: None,
        max_multiplier: None,
        reasons: reasons.iter().map(|r| (*r).into()).collect(),
    }
}

#[test]
fn paired_bounds_multiply_separately_in_f32_xml_order() {
    let min = r#"<MultiplyMinDamage projectileId="-1">"#;
    let max = r#"<MultiplyMaxDamage projectileId="-1">"#;
    assert_eq!(
        one(&format!(
            "{min}0.8</MultiplyMinDamage>{max}0.8</MultiplyMaxDamage>\
             <MultiplyRateOfFire subAttackIndex=\"-1\">1.35</MultiplyRateOfFire>"
        )),
        supported(0.8, 0.8)
    );
    // Unpaired and repeated mutators keep separate per-bound f32 products.
    assert_eq!(
        one(&format!(
            "{min}1.1</MultiplyMinDamage>{max}0.95</MultiplyMaxDamage>{min}0.9</MultiplyMinDamage>"
        )),
        supported(1.1f32 * 0.9f32, 0.95)
    );
    assert_eq!(
        one(&format!("{max} 1.05 </MultiplyMaxDamage>")),
        supported(1.0, 1.05)
    );
}

#[test]
fn reviewed_unrelated_mutators_and_empty_enchants_are_known_neutral() {
    let neutral = r#"<ActivateOnEquip stat="ATT" amount="4">IncrementStat</ActivateOnEquip>
        <ActivateOnEquip amount="1.1">StatModMult</ActivateOnEquip>
        <ActivateOnEquip mult="0.95">DamageMultSelf</ActivateOnEquip>
        <MultiplySpeed>0.9</MultiplySpeed><MultiplyLifetimeMS>1.1</MultiplyLifetimeMS>
        <MultiplySize>1.25</MultiplySize><MultiplyMPCost>0.9</MultiplyMPCost>
        <AddOnPlayerShootActivate proc="Synthetic" cooldown="1"/>"#;
    assert_eq!(one(neutral), supported(1.0, 1.0));
    let c = catalog(
        r#"<Enchantment type="2" id="Empty"/><Enchantment type="3" id="NoMutators"><Mutators/></Enchantment>"#,
    );
    for e in &c.enchants {
        assert_eq!(e.weapon_damage, Some(supported(1.0, 1.0)));
    }
}

#[test]
fn uninterpretable_shapes_are_unsupported_never_neutral() {
    for (mutators, reason) in [
        (
            r#"<MultiplyMinDamage projectileId="0">0.8</MultiplyMinDamage>"#,
            "unsupported_damage_scope",
        ),
        (
            "<MultiplyMaxDamage>0.8</MultiplyMaxDamage>",
            "unsupported_damage_scope",
        ),
        (
            r#"<MultiplyMinDamage projectileId="-1" subAttackIndex="0">0.8</MultiplyMinDamage>"#,
            "unsupported_damage_scope",
        ),
        (
            r#"<MultiplyMinDamage projectileId="-1"><Value>0.8</Value></MultiplyMinDamage>"#,
            "unsupported_damage_scope",
        ),
        (
            r#"<MultiplyMinDamage projectileId="-1">-0.5</MultiplyMinDamage>"#,
            "invalid_damage_multiplier",
        ),
        (
            r#"<MultiplyMaxDamage projectileId="-1">NaN</MultiplyMaxDamage>"#,
            "invalid_damage_multiplier",
        ),
        (
            r#"<MultiplyMaxDamage projectileId="-1">synthetic</MultiplyMaxDamage>"#,
            "invalid_damage_multiplier",
        ),
        (
            r#"<MultiplyMaxDamage projectileId="-1">1e30</MultiplyMaxDamage><MultiplyMaxDamage projectileId="-1">1e30</MultiplyMaxDamage>"#,
            "invalid_damage_multiplier",
        ),
        (
            r#"<SetSubAttack subAttackIndex="0" subAttackObject="Synthetic">0</SetSubAttack>"#,
            "unsupported_subattack",
        ),
        (
            r#"<AddSubAttack subAttackIndex="1" subAttackObject="Synthetic">1</AddSubAttack>"#,
            "unsupported_subattack",
        ),
        (
            "<MultiplyDamage>2</MultiplyDamage>",
            "unsupported_weapon_mutator",
        ),
        (
            r#"<ActivateOnEquip amount="2">SyntheticDamageBoost</ActivateOnEquip>"#,
            "unsupported_weapon_mutator",
        ),
    ] {
        assert_eq!(one(mutators), unsupported(&[reason]), "{mutators}");
    }
    let c = catalog(
        r#"<Enchantment type="1" id="Synthetic"><Mutators kind="synthetic"/></Enchantment>"#,
    );
    assert_eq!(
        c.enchants[0].weapon_damage,
        Some(unsupported(&["unsupported_weapon_mutator"]))
    );
}

#[test]
fn validation_rejects_invalid_metadata_and_keeps_legacy_absence_unknown() {
    let c = catalog(
        r#"<Enchantment type="1" id="Synthetic"><Mutators><MultiplyMinDamage projectileId="-1">0.8</MultiplyMinDamage><MultiplyMaxDamage projectileId="-1">0.8</MultiplyMaxDamage></Mutators></Enchantment>"#,
    );
    validate_catalog(&c).unwrap();
    let mut json = serde_json::to_value(&c).unwrap();
    json["enchants"][0]
        .as_object_mut()
        .unwrap()
        .remove("weapon_damage");
    let old: Catalog = serde_json::from_value(json).unwrap();
    assert_eq!(old.enchants[0].weapon_damage, None);
    validate_catalog(&old).unwrap();
    let mut good = c.clone();
    good.enchants[0].weapon_damage = Some(unsupported(&["unsupported_subattack"]));
    validate_catalog(&good).unwrap();
    let invalid = [
        supported(-1.0, 0.8),
        supported(0.8, f32::INFINITY),
        EnchantWeaponDamageDef {
            min_multiplier: None,
            ..supported(0.8, 0.8)
        },
        EnchantWeaponDamageDef {
            reasons: vec!["unsupported_subattack".into()],
            ..supported(0.8, 0.8)
        },
        EnchantWeaponDamageDef {
            max_multiplier: Some(1.0),
            ..unsupported(&["unsupported_subattack"])
        },
        unsupported(&[]),
        unsupported(&["synthetic_reason"]),
        EnchantWeaponDamageDef {
            support: CatalogSupport::Partial,
            ..supported(0.8, 0.8)
        },
    ];
    for metadata in invalid {
        let mut bad = c.clone();
        bad.enchants[0].weapon_damage = Some(metadata.clone());
        assert!(validate_catalog(&bad).is_err(), "{metadata:?}");
    }
    let mut bad = c.clone();
    bad.enchants[0]
        .weapon_damage
        .as_mut()
        .unwrap()
        .min_multiplier = Some(f64::MAX);
    assert!(validate_catalog(&bad).is_err());
}

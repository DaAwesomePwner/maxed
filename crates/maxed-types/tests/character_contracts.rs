use maxed_types::demo::demo_catalog;

use maxed_types::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

fn round_trip<T: Serialize + DeserializeOwned>(value: &T) -> Value {
    let json = serde_json::to_value(value).unwrap();
    let restored: T = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), json);
    json
}

fn revision() -> Value {
    json!({"stream_id": "synthetic-stream", "character_id": "synthetic-character",
        "input_hash": "a".repeat(64), "catalog_fingerprint": null})
}

fn request() -> Value {
    json!({"defense": 0, "comparison_item_types": [], "expected_revision": revision()})
}

#[test]
fn legacy_json_keeps_new_evidence_unknown() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/p1-demo.json")).unwrap();
    let item: ItemDef = serde_json::from_value(fixture["catalog"]["items"][0].clone()).unwrap();
    assert_eq!(item.weapon_firing, None);
    let enchant: EnchantDef = serde_json::from_value(json!({"id": 1, "name": "Synthetic",
        "description": "", "future_field": true}))
    .unwrap();
    assert_eq!(enchant.regeneration, None);
    let mut old = fixture["snapshot"]["character"].clone();
    old["future_field"] = json!(true);
    let character: CharacterView = serde_json::from_value(old).unwrap();
    assert_eq!(character.calculation_revision, None);
    assert_eq!(character.equipment[0].observed_empty, None);
    let stat: StatLine =
        serde_json::from_value(json!({"stat_id": 20, "future_field": true})).unwrap();
    assert_eq!(stat.split_evidence, None);
    assert!(round_trip(&character).get("calculation_revision").is_none());
    assert!(
        round_trip(&EngineEvent::CharacterUpdated(character))["character_updated"]
            .get("calculation_revision")
            .is_none()
    );
}

#[test]
fn catalog_metadata_preserves_absent_unsupported_partial_and_supported_empty() {
    for support in ["unsupported", "partial", "supported"] {
        let mut item = serde_json::to_value(demo_catalog().items.remove(0)).unwrap();
        item["weapon_firing"] = json!({"support": support, "groups": [],
            "omitted_projectile_indices": [], "reasons": []});
        let typed: ItemDef = serde_json::from_value(item).unwrap();
        assert_eq!(round_trip(&typed)["weapon_firing"]["support"], support);
        let enchant: EnchantDef = serde_json::from_value(json!({"id": 1, "name": "Synthetic",
            "description": "", "regeneration": {"support": support, "hp": [], "mp": [],
            "reasons": [], "source_units": []}}))
        .unwrap();
        assert_eq!(round_trip(&enchant)["regeneration"]["support"], support);
    }
    let mut item = demo_catalog().items.remove(0);
    item.weapon_firing = None;
    assert!(round_trip(&item).get("weapon_firing").is_none());
    let enchant: EnchantDef =
        serde_json::from_value(json!({"id": 1, "name": "Synthetic", "description": ""})).unwrap();
    assert!(round_trip(&enchant).get("regeneration").is_none());
}

#[test]
fn requests_reject_unknown_fields_and_invalid_controls() {
    for defense in [0, 10_000] {
        let mut input = request();
        input["defense"] = json!(defense);
        input["comparison_item_types"] = json!([1, 2]);
        round_trip(&serde_json::from_value::<CharacterCalculationRequest>(input).unwrap());
    }
    for (field, value) in [
        ("defense", json!(10_001)),
        ("defense", json!(-1)),
        ("defense", json!(0.5)),
        ("comparison_item_types", json!([1, 2, 3])),
        ("comparison_item_types", json!([1, 1])),
        ("player", json!({})),
        ("stats", json!([])),
    ] {
        let mut input = request();
        input[field] = value;
        assert!(
            serde_json::from_value::<CharacterCalculationRequest>(input).is_err(),
            "{field}"
        );
    }
}

#[test]
fn revisions_validate_native_id_bounds_and_lowercase_hashes() {
    for field in ["stream_id", "character_id"] {
        for invalid in [String::new(), "x".repeat(257), "synthetic\n".into()] {
            let mut input = request();
            input["expected_revision"][field] = json!(invalid);
            assert!(serde_json::from_value::<CharacterCalculationRequest>(input).is_err());
        }
        let mut input = request();
        input["expected_revision"][field] = json!("x".repeat(256));
        assert!(serde_json::from_value::<CharacterCalculationRequest>(input).is_ok());
    }
    for field in ["input_hash", "catalog_fingerprint"] {
        for invalid in [
            String::new(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
        ] {
            let mut input = request();
            input["expected_revision"][field] = json!(invalid);
            assert!(serde_json::from_value::<CharacterCalculationRequest>(input).is_err());
        }
    }
    let mut input = request();
    input["expected_revision"]["catalog_fingerprint"] = json!("0123456789abcdef".repeat(4));
    assert!(serde_json::from_value::<CharacterCalculationRequest>(input.clone()).is_ok());
    input["expected_revision"]["stats"] = json!([]);
    assert!(serde_json::from_value::<CharacterCalculationRequest>(input).is_err());
}

#[test]
fn split_evidence_vocabulary_is_stable() {
    let reasons = json!([
        "missing_exalt",
        "invalid_exalt",
        "missing_operands",
        "overflow",
        "negative_base",
        "negative_other",
        "unverified_placement"
    ]);
    for placement in [
        "unresolved",
        "provisional_inside_boost",
        "verified_inside_boost",
        "verified_inside_base",
    ] {
        let evidence = json!({"placement": placement, "cap_eligible": false, "reasons": reasons});
        let typed: StatSplitEvidence = serde_json::from_value(evidence.clone()).unwrap();
        assert_eq!(round_trip(&typed), evidence);
    }
}

fn value(number: Option<f64>, completeness: CalculationCompleteness) -> CharacterValue {
    CharacterValue {
        value: number,
        evidence: CalculationEvidence::Modeled,
        completeness,
        reasons: vec!["synthetic_reason".into()],
        exclusions: vec!["base_regeneration".into()],
    }
}

fn calculation() -> CharacterCalculation {
    let unknown = value(None, CalculationCompleteness::Unavailable);
    CharacterCalculation {
        revision: serde_json::from_value(revision()).unwrap(),
        observed_at: Timestamp(1),
        catalog_fingerprint: None,
        formula_version: "character-v1".into(),
        freshness: CharacterFreshness::LastSeen,
        issues: vec![CoverageIssue::MissingCatalog],
        applied_defense: 0,
        weapons: vec![],
        regeneration: CharacterRegeneration {
            applied_wis: None,
            wis: unknown.clone(),
            effects: vec![],
            hp_subtotal: unknown.clone(),
            mp_subtotal: unknown.clone(),
            hp_total: unknown.clone(),
            mp_total: unknown.clone(),
            base_vit: unknown.clone(),
            base_mp: unknown,
        },
        exalt_damage: DeferredExaltDamage {
            status: ExaltDamageStatus::Deferred,
            value: None,
            reason: "deferred_pending_verification".into(),
        },
    }
}

#[test]
fn completeness_and_freshness_remain_independent() {
    for (number, completeness, tag) in [
        (Some(0.0), CalculationCompleteness::Complete, "complete"),
        (Some(2.0), CalculationCompleteness::Partial, "partial"),
        (None, CalculationCompleteness::Unavailable, "unavailable"),
    ] {
        let mut result = calculation();
        result.regeneration.hp_subtotal = value(number, completeness);
        for freshness in [
            CharacterFreshness::Live,
            CharacterFreshness::LastSeen,
            CharacterFreshness::Died,
        ] {
            result.freshness = freshness;
            assert_eq!(
                round_trip(&result)["regeneration"]["hp_subtotal"]["completeness"],
                tag
            );
        }
    }
    let range = CharacterRange {
        min: Some(0),
        max: Some(20),
        evidence: CalculationEvidence::Catalog,
        completeness: CalculationCompleteness::Complete,
        reasons: vec![],
        exclusions: vec![],
    };
    assert_eq!(round_trip(&range)["min"], 0);
}

#[test]
fn nested_nonfinite_values_fail_serialization() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut result = calculation();
        result.regeneration.hp_subtotal.value = Some(bad);
        assert!(serde_json::to_string(&result).is_err());
        let mut item = demo_catalog().items.remove(0);
        item.weapon_firing = Some(WeaponFiringDef {
            subattack_projectile_indices: None,
            support: CatalogSupport::Supported,
            groups: vec![WeaponAttackGroup {
                group_id: "synthetic-group".into(),
                projectile_index: 0,
                count: 1,
                rate_factor: bad,
                schedule: WeaponSchedule::Simultaneous,
                reasons: vec![],
            }],
            omitted_projectile_indices: vec![],
            reasons: vec![],
        });
        assert!(serde_json::to_string(&item).is_err());
        for fractional in [false, true] {
            let effect = RegenEffect {
                flat_per_sec: if fractional { None } else { Some(bad) },
                fraction_of_max_per_sec: if fractional { Some(bad) } else { None },
                source_units: vec!["synthetic_per_second".into()],
            };
            let enchant = EnchantDef {
                id: 1,
                name: "Synthetic".into(),
                description: String::new(),
                stat_damage_multiplier: None,
                weapon_damage: None,
                regeneration: Some(EnchantRegenDef {
                    support: CatalogSupport::Supported,
                    hp: vec![effect],
                    mp: vec![],
                    reasons: vec![],
                    source_units: vec![],
                }),
            };
            assert!(serde_json::to_string(&enchant).is_err());
        }
    }
}

#[test]
fn deferred_exalt_rejects_every_numeric_value() {
    round_trip(&calculation().exalt_damage);
    for number in [0.0, 1.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut result = calculation();
        result.exalt_damage.value = Some(number);
        assert!(serde_json::to_string(&result).is_err());
    }
    for invalid in [
        json!({"status": "deferred", "value": 1.0, "reason": "deferred"}),
        json!({"status": "available", "value": null, "reason": "deferred"}),
    ] {
        assert!(serde_json::from_value::<DeferredExaltDamage>(invalid).is_err());
    }
}

#[test]
fn populated_weapon_and_regeneration_rows_preserve_independent_evidence() {
    let known = value(Some(2.0), CalculationCompleteness::Complete);
    let unknown = value(None, CalculationCompleteness::Unavailable);
    let range = CharacterRange {
        min: Some(0),
        max: Some(20),
        evidence: CalculationEvidence::Catalog,
        completeness: CalculationCompleteness::Complete,
        reasons: vec![],
        exclusions: vec![],
    };
    let mut result = calculation();
    result.weapons.push(CharacterWeaponCalculation {
        item_type: Some(200),
        applied_att: Some(25),
        applied_dex: None,
        applied_primary_mask: Some(0),
        rate_factor: unknown.clone(),
        catalog_range: range.clone(),
        hit_range: range.clone(),
        hit_mean: known.clone(),
        attacks_per_sec: unknown.clone(),
        projectiles_per_sec: unknown.clone(),
        dps_subtotal: known.clone(),
        dps_total: unknown.clone(),
        groups: vec![CharacterAttackGroupCalculation {
            group_id: "synthetic-group".into(),
            projectile_index: 0,
            armor_piercing: None,
            count: 2,
            rate_factor: known.clone(),
            catalog_range: range.clone(),
            hit_range: range,
            hit_mean: known.clone(),
            attacks_per_sec: unknown.clone(),
            projectiles_per_sec: unknown.clone(),
            dps: unknown.clone(),
        }],
    });
    result
        .regeneration
        .effects
        .push(CharacterRegenContribution {
            source_slot: 0,
            enchant_id: 1,
            resource: CharacterRegenResource::Hp,
            applied_maximum: None,
            flat_per_sec: CharacterValue {
                evidence: CalculationEvidence::Catalog,
                ..known.clone()
            },
            fraction_of_max_per_sec: CharacterValue {
                value: Some(0.01),
                evidence: CalculationEvidence::Catalog,
                ..known.clone()
            },
            source_units: vec!["synthetic_per_second".into()],
            subtotal: value(Some(2.0), CalculationCompleteness::Partial),
            total: unknown,
        });
    let json = round_trip(&result);
    assert_eq!(
        json["weapons"][0]["groups"][0]["armor_piercing"],
        Value::Null
    );
    assert_eq!(json["regeneration"]["effects"][0]["subtotal"]["value"], 2.0);
    assert_eq!(
        json["regeneration"]["effects"][0]["total"]["value"],
        Value::Null
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut weapon_bad = result.clone();
        weapon_bad.weapons[0].groups[0].rate_factor.value = Some(bad);
        assert!(serde_json::to_string(&weapon_bad).is_err());
        let mut regen_bad = result.clone();
        regen_bad.regeneration.effects[0]
            .fraction_of_max_per_sec
            .value = Some(bad);
        assert!(serde_json::to_string(&regen_bad).is_err());
    }
}

#[test]
fn directly_constructed_requests_can_be_validated_before_use() {
    let valid: CharacterCalculationRequest = serde_json::from_value(request()).unwrap();
    assert!(valid.validate().is_ok());
    let mut invalid = valid.clone();
    invalid.defense = 10_001;
    assert!(invalid.validate().is_err());
    invalid = valid.clone();
    invalid.comparison_item_types = vec![1, 1];
    assert!(invalid.validate().is_err());
    invalid.comparison_item_types = vec![1, 2, 3];
    assert!(invalid.validate().is_err());
    invalid = valid.clone();
    invalid.expected_revision.stream_id.clear();
    assert!(invalid.validate().is_err());
    invalid = valid.clone();
    invalid.expected_revision.character_id.clear();
    assert!(invalid.validate().is_err());
    invalid = valid.clone();
    invalid.expected_revision.input_hash = "A".repeat(64);
    assert!(invalid.validate().is_err());
    invalid = valid;
    invalid.expected_revision.catalog_fingerprint = Some(String::new());
    assert!(invalid.validate().is_err());
}

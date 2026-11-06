use maxed_types::DamageBucket;
use serde_json::{json, Value};

#[test]
fn legacy_damage_bucket_without_others_deserializes() {
    let mut value = json!({
        "from": "0", "until": "1000000", "total": 550.0, "local": 100.0,
    });
    let bucket: DamageBucket = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(bucket.others_damage, None);
    assert_eq!(bucket.others_presence, None);
    let restored = serde_json::to_value(bucket).unwrap();
    for field in ["others_damage", "others_presence"] {
        assert_eq!(restored.get(field), Some(&Value::Null));
        value[field] = Value::Null;
    }
    assert_eq!(restored, value);
}

#[test]
fn generated_damage_bucket_marks_others_optional_nullable() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../app/ui/src/lib/bindings/DamageBucket.ts");
    let binding = std::fs::read_to_string(path).unwrap();
    assert!(binding.contains("others_damage?: number | null"));
    assert!(binding.contains("others_presence?: number | null"));
}

#[test]
fn party_bucket_fields_require_finite_values() {
    let value = json!({
        "from": "0", "until": "1000000", "total": 550.0, "local": 100.0,
        "others_damage": 400.0, "others_presence": 2.0,
    });
    let bucket: DamageBucket = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&bucket).unwrap(), value);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut damage = bucket.clone();
        damage.others_damage = Some(invalid);
        assert!(serde_json::to_value(damage).is_err());
        let mut presence = bucket.clone();
        presence.others_presence = Some(invalid);
        assert!(serde_json::to_value(presence).is_err());
    }
}

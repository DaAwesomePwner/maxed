use maxed_types::ObjectVisual;
use serde_json::{json, Value};
use ts_rs::TS;

#[test]
fn visual_item_tier_optional_and_catalog_only() {
    let old = json!({ "object_type": 65535, "name": null, "sprite": null, "bag": null });
    let mut visual: ObjectVisual = serde_json::from_value(old).unwrap();
    assert_eq!(visual.item_tier, None);
    assert_eq!(
        serde_json::to_value(&visual).unwrap()["item_tier"],
        Value::Null
    );
    for tier in ["UT", "ST", "14"] {
        visual.item_tier = Some(tier.into());
        let value = serde_json::to_value(&visual).unwrap();
        assert_eq!(value["item_tier"], tier);
        assert_eq!(
            serde_json::from_value::<ObjectVisual>(value).unwrap(),
            visual
        );
    }
    assert!(ObjectVisual::decl().contains("item_tier?: string | null"));
}

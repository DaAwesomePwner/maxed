use maxed_types::*;

#[test]
fn settings_without_loot_load_with_empty_chase() {
    let text = r#"{"version":1,"adapter_id":null,"assets_path":null,"save_recordings":false,"recording_limit_bytes":1000000000,"update_channel":"stable"}"#;
    let settings: AppSettings = serde_json::from_str(text).expect("v1 file without loot loads");
    assert!(settings.loot.chase.is_empty());
}

#[test]
fn chase_item_round_trips_and_rejects_unknown_fields() {
    let item = ChaseItem {
        item_type: 3180,
        dungeon: Some("The Shatters".into()),
        pinned_at: Timestamp(5),
    };
    let text = serde_json::to_string(&item).unwrap();
    assert_eq!(serde_json::from_str::<ChaseItem>(&text).unwrap(), item);
    assert!(serde_json::from_str::<ChaseItem>(
        r#"{"item_type":1,"dungeon":null,"pinned_at":"1","x":1}"#
    )
    .is_err());
}

#[test]
fn loot_place_is_tagged() {
    assert_eq!(
        serde_json::to_string(&LootPlace::Outside).unwrap(),
        r#"{"kind":"outside"}"#
    );
    assert_eq!(
        serde_json::to_string(&LootPlace::Dungeon {
            name: "Lost Halls".into()
        })
        .unwrap(),
        r#"{"kind":"dungeon","name":"Lost Halls"}"#
    );
}

#[test]
fn drop_filter_rejects_unknown_fields() {
    assert!(serde_json::from_str::<DropFilter>(
        r#"{"min_rank":null,"place":null,"item_type":null,"enchanted_only":false,"range":null,"cursor":null,"limit":50,"x":0}"#
    ).is_err());
}

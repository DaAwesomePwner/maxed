use maxed_types::*;
use ts_rs::TS;

#[test]
fn home_detail_null_finite_and_bounded_preview_contract() {
    let mut detail = HomeRunDetail {
        run_id: "synthetic-run".into(),
        local_share: None,
        share_partial: true,
        observed_drop_count: 7,
        loot: vec![demo::demo_run_story().drops.remove(0); 3],
        loot_preview_limited: true,
        issues: vec![CoverageIssue::StreamGap],
    };
    let value = serde_json::to_value(&detail).unwrap();
    assert!(value["local_share"].is_null());
    assert_eq!(value["loot"].as_array().unwrap().len(), 3);
    assert_eq!(value["observed_drop_count"], 7);
    assert_eq!(
        serde_json::from_value::<HomeRunDetail>(value).unwrap(),
        detail
    );
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        detail.local_share = Some(number);
        assert!(serde_json::to_string(&detail).is_err());
    }
    detail.local_share = None;
    let invalid = serde_json::to_string(&detail)
        .unwrap()
        .replace("\"local_share\":null", "\"local_share\":1e999");
    assert!(serde_json::from_str::<HomeRunDetail>(&invalid).is_err());
    let declaration = HomeRunDetail::decl();
    assert!(declaration.contains("local_share: number | null"));
    assert!(HomeSummary::decl().contains("recent_details: Array<HomeRunDetail>"));
}

#[test]
fn scope_and_large_decimal_timestamps_round_trip() {
    let scopes = [
        (
            HomeScope::Session {
                id: "synthetic-session".into(),
            },
            serde_json::json!({"session": {"id": "synthetic-session"}}),
        ),
        (
            HomeScope::Today {
                range: TimeRange {
                    start: Timestamp(9_007_199_254_740_993),
                    end: Timestamp(9_007_199_254_740_994),
                },
            },
            serde_json::json!({"today": {"range": {
                "start": "9007199254740993", "end": "9007199254740994"
            }}}),
        ),
    ];
    for (scope, value) in scopes {
        assert_eq!(serde_json::to_value(&scope).unwrap(), value);
        assert_eq!(serde_json::from_value::<HomeScope>(value).unwrap(), scope);
    }
    let summary = demo::demo_home_summary();
    let value = serde_json::to_value(&summary).unwrap();
    for field in [
        "recent_details",
        "fame_characters",
        "fame_missing_baselines",
        "fame_sampled",
        "unranked_drop_count",
    ] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<HomeSummary>(missing).is_err());
    }
    assert_eq!(
        serde_json::from_value::<HomeSummary>(value).unwrap(),
        summary
    );
}

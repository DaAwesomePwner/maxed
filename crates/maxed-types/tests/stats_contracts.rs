use maxed_types::*;

#[test]
fn filters_reject_unknown_fields() {
    let ok: StatsFilter = serde_json::from_str(r#"{"range":null,"character_id":null}"#).unwrap();
    assert_eq!(
        ok,
        StatsFilter {
            range: None,
            character_id: None
        }
    );
    assert!(
        serde_json::from_str::<StatsFilter>(r#"{"range":null,"character_id":null,"x":1}"#).is_err()
    );
    assert!(serde_json::from_str::<StatsSessionFilter>(
        r#"{"range":null,"character_id":null,"include_empty":false,"cursor":null,"limit":50,"x":1}"#
    )
    .is_err());
}

#[test]
fn visit_round_trips_with_snake_case_kind_and_string_times() {
    let visit = MapVisitView {
        id: "stream:visit:7".into(),
        stream_id: "stream".into(),
        session_id: "session".into(),
        character_id: Some("synth-character".into()),
        run_id: None,
        map: "Vault".into(),
        kind: AreaKind::OtherHub,
        realm_instance_key: None,
        started: Timestamp(5),
        ended: Some(Timestamp(9)),
        source: VisitSource::Live,
    };
    let json = serde_json::to_value(&visit).unwrap();
    assert_eq!(json["kind"], "other_hub");
    assert_eq!(json["source"], "live");
    assert_eq!(json["started"], "5");
    assert_eq!(serde_json::from_value::<MapVisitView>(json).unwrap(), visit);
}

#[test]
fn export_request_and_result_shapes() {
    let request: StatsExportRequest = serde_json::from_str(
        r#"{"kind":{"type":"session_runs","id":"s1"},"filter":{"range":null,"character_id":null},"include_empty":false}"#,
    )
    .unwrap();
    assert_eq!(
        request.kind,
        StatsExportKind::SessionRuns { id: "s1".into() }
    );
    assert_eq!(
        serde_json::to_value(StatsExportResult::Saved { rows: 3 }).unwrap(),
        serde_json::json!({"status":"saved","rows":3})
    );
    assert_eq!(
        serde_json::to_value(StatsExportResult::Cancelled).unwrap(),
        serde_json::json!({"status":"cancelled"})
    );
}

#[test]
fn fame_per_hour_needs_five_minutes() {
    assert_eq!(fame_per_hour(100, MIN_STATS_RATE_MICROS - 1), None);
    assert_eq!(fame_per_hour(100, MIN_STATS_RATE_MICROS), Some(1200.0));
    assert_eq!(fame_per_hour(250, 3_600_000_000), Some(250.0));
    assert_eq!(fame_per_hour(-30, 3_600_000_000), Some(-30.0));
}

#[test]
fn non_finite_rates_are_rejected_on_decode() {
    let row = r#"{"group":"realm","map":"Realm","visits":1,"time_micros":1,"fame":0,
        "missing_baseline":false,"fame_per_hour":"NaN","clears":null,"deaths":null}"#;
    assert!(serde_json::from_str::<StatsMapRow>(row).is_err());
}

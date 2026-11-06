use maxed_types::*;
use serde_json::{json, Value};
use ts_rs::TS;

#[test]
fn exact_counts_use_decimal_strings_in_json_and_typescript() {
    for count in [0, 9_007_199_254_740_993, u64::MAX] {
        let value = serde_json::to_value(StorageCount(count)).unwrap();
        assert_eq!(value, json!(count.to_string()));
        assert_eq!(
            serde_json::from_value::<StorageCount>(value).unwrap().0,
            count
        );
    }
    for invalid in [
        json!(1),
        json!("-1"),
        json!("+1"),
        json!("1.5"),
        json!("00"),
        json!("007"),
        json!(""),
        json!(" 7"),
        json!("7 "),
        json!("\t7"),
        json!("7\n"),
        json!("18446744073709551616"),
    ] {
        assert!(serde_json::from_value::<StorageCount>(invalid).is_err());
    }
    assert!(StorageCount::decl().contains("= string"));
    assert!(DatabaseStorage::decl().contains("page_size: number | null"));
    assert!(DatabaseStorage::decl().contains("main_bytes: StorageCount | null"));
}

fn fields(value: &Value, expected: &[&str]) {
    let mut actual: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut expected = expected.to_vec();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn serialized_field_allowlist_excludes_identity_paths_heads_and_payloads() {
    let maintenance = StorageMaintenance {
        jobs: vec![StorageJobProgress {
            phase: StorageMaintenancePhase::CopyingReceipts,
            paused: true,
            needs_resume: true,
            reason: Some(StorageMaintenanceReason::BlockedStream),
            requested_at: Timestamp(1),
            updated_at: Timestamp(2),
            counters: StorageProgressCounters::default(),
        }],
        totals: StorageProgressCounters::default(),
        last_success_at: None,
    };
    let status = StorageStatus {
        measured_at: Timestamp(3),
        database: DatabaseStorage::default(),
        backups: BackupStorage::default(),
        recordings: RecordingStorage::default(),
        receipt_estimate: StorageEstimate::default(),
        maintenance: Some(maintenance.clone()),
        failure: Some(StorageFailure::Superseded),
    };
    let value = serde_json::to_value(&status).unwrap();
    fields(
        &value,
        &[
            "measured_at",
            "database",
            "backups",
            "recordings",
            "receipt_estimate",
            "maintenance",
            "failure",
        ],
    );
    fields(
        &value["database"],
        &[
            "state",
            "main_bytes",
            "wal_bytes",
            "shm_bytes",
            "page_size",
            "page_count",
            "freelist_count",
            "allocated_bytes",
            "reusable_bytes",
            "auto_vacuum",
            "journal_mode",
        ],
    );
    fields(&value["backups"], &["retained_bytes", "temporary_bytes"]);
    fields(
        &value["recordings"],
        &["physical_bytes", "accounted_bytes", "quota_bytes"],
    );
    fields(
        &value["receipt_estimate"],
        &["bytes", "basis", "measured_at"],
    );
    fields(
        &value["maintenance"],
        &["jobs", "totals", "last_success_at"],
    );
    fields(
        &value["maintenance"]["jobs"][0],
        &[
            "phase",
            "paused",
            "needs_resume",
            "reason",
            "requested_at",
            "updated_at",
            "counters",
        ],
    );
    let counters = &[
        "eligible_records",
        "eligible_bytes",
        "processed_records",
        "processed_bytes",
        "blocked_records",
        "blocked_bytes",
    ];
    fields(&value["maintenance"]["jobs"][0]["counters"], counters);
    fields(&value["maintenance"]["totals"], counters);
    let notice = serde_json::to_value(StorageMaintenanceChanged {
        measured_at: Timestamp(3),
        maintenance: Some(maintenance),
        failure: Some(StorageFailure::Superseded),
    })
    .unwrap();
    fields(&notice, &["measured_at", "maintenance", "failure"]);
    assert_eq!(notice["maintenance"], value["maintenance"]);
    assert_eq!(
        serde_json::from_value::<StorageStatus>(value).unwrap(),
        status
    );
}

#[test]
fn unknown_measurements_and_unmeasured_estimates_are_null_never_zero() {
    let database = serde_json::to_value(DatabaseStorage::default()).unwrap();
    assert_eq!(database["state"], "unavailable");
    for (key, value) in database.as_object().unwrap() {
        if key != "state" {
            assert!(value.is_null(), "{key}");
        }
    }
    assert_eq!(
        serde_json::to_value(StorageEstimate::default()).unwrap(),
        json!({
            "bytes": null, "basis": "legacy_receipt_logical_bytes", "measured_at": null
        })
    );
    for value in [
        serde_json::to_value(BackupStorage::default()).unwrap(),
        serde_json::to_value(RecordingStorage::default()).unwrap(),
        serde_json::to_value(StorageProgressCounters::default()).unwrap(),
    ] {
        assert!(value.as_object().unwrap().values().all(Value::is_null));
    }
}

#[test]
fn upgrade_and_managed_inventory_have_only_local_safe_metadata() {
    let upgrade = serde_json::to_value(HistoryUpgradeStatus {
        phase: HistoryUpgradePhase::Blocked,
        schema_version: Some(6),
        required_bytes: Some(StorageCount(100)),
        free_bytes: None,
        failure: Some(StorageFailure::SpaceUnknown),
        last_failure: Some(StorageFailure::InsufficientSpace),
    })
    .unwrap();
    fields(
        &upgrade,
        &[
            "phase",
            "schema_version",
            "required_bytes",
            "free_bytes",
            "failure",
            "last_failure",
        ],
    );
    assert_eq!(upgrade["free_bytes"], Value::Null);
    assert_eq!(upgrade["required_bytes"], "100");
    let backup = serde_json::to_value(ManagedStorageBackup {
        id: "1".into(),
        role: StorageBackupRole::Initial,
        completed_at: None,
        bytes: None,
        ready: false,
        in_use: false,
    })
    .unwrap();
    fields(
        &backup,
        &["id", "role", "completed_at", "bytes", "ready", "in_use"],
    );
    assert_eq!(backup["completed_at"], Value::Null);
    assert!(!StorageMaintenanceChanged::decl().contains("ManagedStorageBackup"));
}

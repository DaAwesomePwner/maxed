use maxed_types::{CaptureState, CaptureStatus, Timestamp};
#[test]
fn transport_time_and_idle_status_are_explicit() {
    let time = Timestamp(u64::MAX);
    let json = serde_json::to_string(&time).expect("synthetic time serializes");
    assert_eq!(json, "\"18446744073709551615\"");
    assert_eq!(serde_json::from_str::<Timestamp>(&json).unwrap(), time);
    assert_eq!(CaptureStatus::default().state, CaptureState::Stopped);
}

#[test]
fn legacy_capture_status_loads_without_packet_diagnostics() {
    let status: CaptureStatus = serde_json::from_str(r#"{"state":"Stopped","message":"Capture stopped","dropped_frames":0,"unknown_packets":3,"decode_failures":2}"#).unwrap();
    assert!(status.packet_diagnostics.is_empty());
    assert_eq!(status.unknown_packets, 3);
    assert_eq!(status.decode_failures, 2);
}

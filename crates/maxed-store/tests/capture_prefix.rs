use crate::common;
use maxed_store::{apply_batch, migrate, StoreError};
use maxed_types::*;
use rusqlite::Connection;

#[test]
fn leading_capture_events_keep_receipts_without_domain_rows() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let mut events = vec![EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "capture-prefix".into(),
        sequence: Sequence(Timestamp(1)),
        at: Timestamp(0),
        event: EngineEvent::CaptureStateChanged(CaptureStatus::default()),
    }];
    apply_batch(&mut db, &events).unwrap();
    let count = |db: &Connection, table: &str| -> i64 {
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count(&db, "play_sessions"), 0);
    let mut missing_session = events[0].clone();
    missing_session.sequence = Sequence(Timestamp(2));
    let mut run = demo::demo_snapshot().run.unwrap();
    run.character_id = None;
    missing_session.event = EngineEvent::RunStarted(run);
    assert!(matches!(
        apply_batch(&mut db, &[missing_session]),
        Err(StoreError::InvalidEvent)
    ));
    assert_eq!(common::receipts::count(&db), 1);
    let mut gap = events[0].clone();
    gap.sequence = Sequence(Timestamp(3));
    assert!(matches!(
        apply_batch(&mut db, &[gap]),
        Err(StoreError::SequenceGap)
    ));
    events.push(EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "capture-prefix".into(),
        sequence: Sequence(Timestamp(2)),
        at: Timestamp(1),
        event: EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
    });
    let mut capture = events[0].clone();
    capture.sequence = Sequence(Timestamp(3));
    events.push(capture);
    apply_batch(&mut db, &events).unwrap();
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(count(&db, "play_sessions"), 1);
    assert_eq!(common::receipts::count(&db), 3);
}

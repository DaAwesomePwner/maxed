use maxed_store::{open_reader, start_writer, StoreError, WriterConfig};
use rusqlite::Connection;

#[test]
fn file_writer_uses_wal_and_reader_rejects_future_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite");
    let writer = start_writer(path.clone(), WriterConfig::default()).unwrap();
    assert_eq!(writer.shutdown().unwrap().sequence.0 .0, 0);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    drop(open_reader(&path).unwrap());
    db.pragma_update(None, "user_version", 13).unwrap();
    assert!(matches!(open_reader(&path), Err(StoreError::NewerSchema)));
}

use maxed_store::{
    migrate, read_crucible_definitions as read, upsert_crucible_definitions as write,
};
use maxed_types::{CrucibleDef, Timestamp};
use rusqlite::Connection;

fn def(id: &str, damage_multiplier: f64) -> CrucibleDef {
    CrucibleDef {
        id: id.into(),
        damage_multiplier,
    }
}
fn db() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    db
}

#[test]
fn fresh_upgrade_checksums_and_rollback() {
    use crate::common::installed_schema::{checksums, install, schema, user_version};
    let fresh = db();
    assert_eq!(user_version(&fresh), 20);
    assert!(read(&fresh).unwrap().is_empty());
    let mut old = Connection::open_in_memory().unwrap();
    install(&old, 19);
    old.execute(
        "INSERT INTO characters(id,observed_at,json) VALUES('synthetic',1,'{}')",
        [],
    )
    .unwrap();
    let sums = checksums(&old);
    let objects = schema(&old);
    assert!(read(&old).unwrap().is_empty());
    old.execute("CREATE TABLE crucible_definitions(id TEXT)", [])
        .unwrap();
    assert!(migrate(&mut old).is_err());
    assert_eq!(user_version(&old), 19);
    assert_eq!(checksums(&old), sums);
    old.execute("DROP TABLE crucible_definitions", []).unwrap();
    migrate(&mut old).unwrap();
    assert_eq!(schema(&old), schema(&fresh));
    assert!(objects.iter().all(|o| schema(&old).contains(o)));
    assert_eq!(&checksums(&old)[..19], sums);
    assert_eq!(
        old.query_row("SELECT id FROM characters", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "synthetic"
    );
    migrate(&mut old).unwrap();
    old.execute(
        "UPDATE schema_migrations SET checksum='bad' WHERE version=20",
        [],
    )
    .unwrap();
    assert!(migrate(&mut old).is_err());
}

#[test]
fn upserts_validate_and_preserve_first_observation() {
    let mut db = db();
    assert_eq!(
        write(&mut db, &[def("b", 1.), def("a", 100.)], Timestamp(10)).unwrap(),
        2
    );
    assert_eq!(
        write(
            &mut db,
            &[
                def("b", 2.),
                def("", 1.),
                def(&"x".repeat(65), 1.),
                def("nan", f64::NAN),
                def("inf", f64::INFINITY),
                def("zero", 0.),
                def("negative", -1.),
                def("large", 100.1)
            ],
            Timestamp(20)
        )
        .unwrap(),
        1
    );
    assert_eq!(read(&db).unwrap(), vec![def("a", 100.), def("b", 2.)]);
    assert_eq!(
        db.query_row(
            "SELECT first_observed_at,last_observed_at FROM crucible_definitions WHERE id='b'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        )
        .unwrap(),
        (10, 20)
    );
    assert!(write(&mut db, &[def("c", 1.)], Timestamp(u64::MAX)).is_err());
    db.execute_batch("CREATE TRIGGER fail_crucible BEFORE INSERT ON crucible_definitions WHEN NEW.id='fail' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(write(&mut db, &[def("c", 1.), def("fail", 1.)], Timestamp(30)).is_err());
    assert_eq!(read(&db).unwrap().len(), 2);
    for value in ["0", "101", "NULL", "'bad'"] {
        assert!(db
            .execute_batch(&format!(
                "INSERT INTO crucible_definitions VALUES('invalid',{value},0,0)"
            ))
            .is_err());
    }
}

#[test]
fn bounded_eviction_orders_by_time_then_id() {
    let mut db = db();
    let defs: Vec<_> = (0..4096).map(|i| def(&format!("{i:04}"), 1.)).collect();
    assert_eq!(write(&mut db, &defs, Timestamp(10)).unwrap(), 4096);
    write(&mut db, &[def("0000", 2.)], Timestamp(20)).unwrap();
    write(&mut db, &[def("new", 1.)], Timestamp(30)).unwrap();
    let rows = read(&db).unwrap();
    assert_eq!(rows.len(), 4096);
    assert_eq!(rows[0], def("0000", 2.));
    assert!(!rows.iter().any(|r| r.id == "0001"));
    write(&mut db, &[def("old", 1.)], Timestamp(0)).unwrap();
    assert_eq!(read(&db).unwrap(), rows);
}

#[test]
fn reader_skips_bad_rows_and_supports_readonly_and_absent_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    crate::common::installed_schema::install(&db, 19);
    let ro =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert!(read(&ro).unwrap().is_empty());
    migrate(&mut db).unwrap();
    db.execute_batch("DROP TABLE crucible_definitions;
        CREATE TABLE crucible_definitions(id,damage_multiplier);
        INSERT INTO crucible_definitions VALUES('',1),('bad','text'),('null',NULL),('negative',-1),('a',2),('z',1);").unwrap();
    assert_eq!(read(&ro).unwrap(), vec![def("a", 2.), def("z", 1.)]);
    db.execute("DROP TABLE crucible_definitions", []).unwrap();
    assert!(read(&ro).unwrap().is_empty());
    assert_eq!(crate::common::installed_schema::user_version(&ro), 20);
}

#[test]
fn chat_retention_and_party_clears_preserve_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    write(&mut db, &[def("saved", 1.5)], Timestamp(1)).unwrap();
    for op in [
        maxed_store::ChatMaintenance::Clear,
        maxed_store::ChatMaintenance::Prune {
            before: Timestamp(u64::MAX),
        },
    ] {
        maxed_store::maintain_chat_file(&path, op).unwrap();
        assert_eq!(read(&db).unwrap(), vec![def("saved", 1.5)]);
    }
    for marks_and_notes in [false, true] {
        maxed_store::party_write_file(
            &path,
            &maxed_store::PartyWrite::Clear(maxed_types::ClearPartyHistory { marks_and_notes }),
        )
        .unwrap();
        assert_eq!(read(&db).unwrap(), vec![def("saved", 1.5)]);
    }
    db.execute_batch("VACUUM").unwrap();
    crate::common::installed_schema::assert_integrity(&db);
    assert_eq!(read(&db).unwrap(), vec![def("saved", 1.5)]);
}

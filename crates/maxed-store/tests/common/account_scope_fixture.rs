//! Attribution for older, deliberately single-account reader fixtures. New isolation
//! tests seed explicit ownership and never call this compatibility fixture helper.
use maxed_store::{StoreError, StoreReader};
use maxed_types::EventEnvelope;
use rusqlite::Connection;

pub const ACCOUNT: &str =
    "account-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub fn attribute(db: &Connection) {
    db.execute("INSERT OR IGNORE INTO accounts VALUES(?1)", [ACCOUNT])
        .unwrap();
    let mut character = maxed_types::demo::demo_snapshot().character.unwrap();
    character.id = "synthetic-fixture-owner".into();
    character.account_id = Some(ACCOUNT.into());
    db.execute(
        "INSERT OR IGNORE INTO characters(id,observed_at,json,account_id) VALUES(?1,0,?2,?3)",
        rusqlite::params![
            character.id,
            serde_json::to_string(&character).unwrap(),
            ACCOUNT
        ],
    )
    .unwrap();
    db.execute(
        "UPDATE characters SET account_id=?1 WHERE account_id IS NULL",
        [ACCOUNT],
    )
    .unwrap();
    db.execute(
        "UPDATE runs SET character_id='synthetic-fixture-owner' WHERE character_id IS NULL",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE map_visits SET character_id='synthetic-fixture-owner' WHERE character_id IS NULL",
        [],
    )
    .unwrap();
    db.execute("INSERT OR IGNORE INTO characters(id,observed_at,json,account_id) SELECT DISTINCT character_id,0,?1,?2 FROM observation_characters WHERE character_id IS NOT NULL", rusqlite::params![serde_json::to_string(&character).unwrap(),ACCOUNT]).unwrap();
    db.execute("UPDATE deaths SET character_id='synthetic-fixture-owner' WHERE character_id IS NULL AND run_id IS NULL", []).unwrap();
    for (kind, table) in [("fight", "fights"), ("drop", "drops")] {
        db.execute(&format!("INSERT OR IGNORE INTO observation_characters SELECT ?1,id,'synthetic-fixture-owner' FROM {table} WHERE run_id IS NULL"),[kind]).unwrap();
        db.execute(&format!("UPDATE observation_characters SET character_id='synthetic-fixture-owner' WHERE kind=?1 AND character_id IS NULL AND observation_id IN (SELECT id FROM {table} WHERE run_id IS NULL)"),[kind]).unwrap();
    }
}

pub fn apply_batch(db: &mut Connection, events: &[EventEnvelope]) -> Result<(), StoreError> {
    maxed_store::apply_batch(db, events)?;
    attribute(db);
    Ok(())
}

pub fn open_reader(path: &std::path::Path) -> Result<StoreReader, StoreError> {
    let db = Connection::open(path)?;
    if db.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='accounts'",
        [],
        |r| r.get::<_, i64>(0),
    )? > 0
    {
        attribute(&db);
    }
    maxed_store::open_reader(path)
}

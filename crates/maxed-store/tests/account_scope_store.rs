use crate::common::envelope;
use maxed_store::{accounts::*, apply_batch, migrate};
use maxed_types::*;
use rusqlite::Connection;

fn account(c: char) -> String {
    format!("account-{}", c.to_string().repeat(64))
}
fn character(id: &str, account_id: Option<String>) -> EngineEvent {
    let mut c = demo::demo_snapshot().character.unwrap();
    c.id = id.into();
    c.account_id = account_id;
    EngineEvent::CharacterUpdated(c)
}
fn identity(at: u64, id: Option<&str>, a: Option<String>) -> EngineEvent {
    EngineEvent::LocalIdentityChanged {
        at: Timestamp(at),
        character_id: id.map(Into::into),
        account_id: a,
    }
}
fn seed(db: &mut Connection, mut events: Vec<EngineEvent>) -> Vec<EventEnvelope> {
    if !matches!(events.first(), Some(EngineEvent::SessionUpdated(_))) {
        events.insert(
            0,
            EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
        );
    }
    let events: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, e)| envelope(i as u64 + 1, e))
        .collect();
    apply_batch(db, &events).unwrap();
    events
}
fn owner(db: &Connection, view: &str, id: &str) -> Option<String> {
    db.query_row(
        &format!("SELECT account_id FROM {view} WHERE id=?1"),
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn account_scope_interleaving_revisions_merge_conflict_and_replay() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let snap = demo::demo_snapshot();
    let mut f = snap.fights[0].clone();
    f.owner_character_id = Some("provisional".into());
    let mut d = snap.drops[0].clone();
    d.owner_character_id = Some("provisional".into());
    let mut revised_f = f.clone();
    revised_f.owner_character_id = Some("b-7".into());
    let mut revised_d = d.clone();
    revised_d.owner_character_id = Some("b-7".into());
    let mut run = snap.run.unwrap();
    run.character_id = Some("a-7".into());
    let events = seed(
        &mut db,
        vec![
            EngineEvent::SessionUpdated(snap.session.unwrap()),
            character("provisional", None),
            character("a-7", Some(account('a'))),
            character("b-7", Some(account('b'))),
            identity(10, Some("provisional"), None),
            EngineEvent::RunStarted(run),
            EngineEvent::FightStarted(f.clone()),
            EngineEvent::BagDropped(d.clone()),
            EngineEvent::CharacterMerged {
                provisional_id: "provisional".into(),
                verified_id: "a-7".into(),
            },
            identity(20, Some("a-7"), Some(account('a'))),
            identity(30, None, None),
            identity(40, Some("b-7"), Some(account('b'))),
            EngineEvent::FightUpdated(revised_f),
            EngineEvent::DropUpdated(revised_d),
            character("a-7", Some(account('b'))),
            identity(50, None, None),
        ],
    );
    assert_eq!(owner(&db, "character_accounts", "a-7"), Some(account('a')));
    assert_eq!(owner(&db, "character_accounts", "b-7"), Some(account('b')));
    assert_eq!(owner(&db, "fight_accounts", &f.id), Some(account('a')));
    assert_eq!(owner(&db, "drop_accounts", &d.id), Some(account('a')));
    assert_eq!(current_account(&db).unwrap(), Some(account('b')));
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM identity_spans WHERE started=10",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        account('a')
    );
    assert_eq!(
        db.query_row(
            "SELECT sum(ended-started) FROM identity_spans WHERE account_id=?1",
            [account('a')],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        20
    );
    assert_eq!(
        db.query_row(
            "SELECT sum(ended-started) FROM identity_spans WHERE account_id=?1",
            [account('b')],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        10
    );
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM identity_spans", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        5
    );
}

#[test]
fn account_scope_legacy_attach_exact_idempotent_and_progress() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![character("a-7", None), character("b-7", Some(account('b')))],
    );
    let ids = vec!["a-7".into(), "a-7".into(), "b-7".into(), "7".into()];
    assert_eq!(
        attach_legacy_characters(&mut db, &account('a'), &ids).unwrap(),
        AttachCounts {
            attached: 1,
            already_attached: 0,
            conflicting: 1,
            unmatched: 1
        }
    );
    assert_eq!(
        attach_legacy_characters(&mut db, &account('a'), &ids)
            .unwrap()
            .already_attached,
        1
    );
    assert_eq!(current_account(&db).unwrap(), None);
    assert_eq!(recovery_progress(&db, &account('a')).unwrap(), None);
    record_recovery_progress(&mut db, &account('a'), 65535).unwrap();
    record_recovery_progress(&mut db, &account('a'), 10).unwrap();
    assert_eq!(recovery_progress(&db, &account('a')).unwrap(), Some(65535));
}

#[test]
fn account_scope_current_verified_only_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("account.sqlite");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![
            character("a-7", Some(account('a'))),
            identity(10, Some("a-7"), None),
        ],
    );
    assert_eq!(current_account(&db).unwrap(), None);
    apply_batch(
        &mut db,
        &[
            envelope(4, identity(20, Some("a-7"), Some(account('a')))),
            envelope(5, identity(30, None, None)),
            envelope(6, identity(40, Some("pending"), None)),
        ],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        maxed_store::open_reader(&path)
            .unwrap()
            .current_account()
            .unwrap(),
        Some(account('a'))
    );
}

#[test]
fn account_scope_migration_016_populated_remains_unknown() {
    use sha2::{Digest, Sha256};
    let mut db = Connection::open_in_memory().unwrap();
    // Reconstruct installed schemas, never downgrade a current-schema database.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .collect();
    paths.sort();
    for (i, path) in paths.iter().take(16).enumerate() {
        let sql = std::fs::read_to_string(path).unwrap();
        db.execute_batch(&sql).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            rusqlite::params![
                i as i64 + 1,
                format!("{:x}", Sha256::digest(sql.as_bytes()))
            ],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 16).unwrap();
    db.execute(
        "INSERT INTO characters(id,observed_at,json) VALUES('legacy',0,'{}')",
        [],
    )
    .unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(owner(&db, "character_accounts", "legacy"), None);
    assert_eq!(current_account(&db).unwrap(), None);
}

#[test]
fn account_scope_reader_contract_unknown_and_conflicting_death() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let snap = demo::demo_snapshot();
    let mut run = snap.run.unwrap();
    run.character_id = Some("a-7".into());
    let run_id = run.id.clone();
    let mut fight = snap.fights[0].clone();
    fight.owner_character_id = None;
    seed(
        &mut db,
        vec![
            EngineEvent::SessionUpdated(snap.session.unwrap()),
            character("a-7", Some(account('a'))),
            character("b-7", Some(account('b'))),
            EngineEvent::RunStarted(run),
            EngineEvent::FightStarted(fight.clone()),
        ],
    );
    assert_eq!(owner(&db, "fight_accounts", &fight.id), Some(account('a')));
    fight.owner_character_id = Some("b-7".into());
    apply_batch(
        &mut db,
        &[envelope(6, EngineEvent::FightUpdated(fight.clone()))],
    )
    .unwrap();
    assert_eq!(owner(&db, "fight_accounts", &fight.id), Some(account('a')));
    db.execute("INSERT INTO deaths(stream_id,sequence,character_id,run_id,at) VALUES('synthetic',1,'b-7',?1,100),('synthetic',2,NULL,?1,100)",[run_id]).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM death_accounts WHERE sequence=1",
            [],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM death_accounts WHERE sequence=2",
            [],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        Some(account('a'))
    );
    let bad = envelope(7, identity(100, Some("a-7"), Some(account('b'))));
    assert!(apply_batch(&mut db, &[bad]).is_err());
    assert_eq!(current_account(&db).unwrap(), None);
}

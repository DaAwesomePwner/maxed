use super::*;
use maxed_types::*;

fn context(path: std::path::PathBuf, namespace: Namespace) -> super::super::home::HomeReadContext {
    super::super::home::HomeReadContext {
        database: path,
        namespace,
        generation: 0,
        stream_id: Some("synthetic".into()),
        absent: false,
        account: None,
        account_revision: 0,
        identity_sequence: None,
    }
}

#[test]
fn account_scope_resolution_ignores_other_streams_and_duplicate_identity() {
    let home = HomeReads::default();
    *home.context.lock().unwrap() = Some(context("unused".into(), Namespace::Live));
    assert!(home
        .observe_account(Some("demo"), "account-a", None)
        .is_none());
    assert!(home
        .observe_account(Some("synthetic"), "account-a", None)
        .is_some());
    assert!(home
        .observe_account(Some("synthetic"), "account-a", None)
        .is_none());
    assert_eq!(
        home.context
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .account
            .as_deref(),
        Some("account-a")
    );
    assert!(home
        .observe_account(Some("synthetic"), "account-b", None)
        .is_some());
    assert_eq!(home.account_revision.load(Ordering::Acquire), 2);
}

#[test]
fn account_scope_recovery_resumes_exact_batches_and_notifies_once() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let account = format!("account-{}", "a".repeat(64));
    let key = [7; 32];
    {
        let mut db = rusqlite::Connection::open(&path).unwrap();
        maxed_store::migrate(&mut db).unwrap();
        let events = [4096, 65535, 1, -1]
            .into_iter()
            .enumerate()
            .map(|(index, number)| {
                let mut c = demo::demo_snapshot().character.unwrap();
                c.id = maxed_engine::character_id_for(&key, &account, number).unwrap();
                c.account_id = None;
                EventEnvelope {
                    stream_id: "seed".into(),
                    sequence: Sequence(Timestamp(index as u64 + 2)),
                    at: Timestamp(1),
                    event: EngineEvent::CharacterUpdated(c),
                    fight_chart_omitted: false,
                }
            })
            .collect::<Vec<_>>();
        let mut events = events;
        events.insert(
            0,
            EventEnvelope {
                stream_id: "seed".into(),
                sequence: Sequence(Timestamp(1)),
                at: Timestamp(1),
                event: EngineEvent::CaptureStateChanged(CaptureStatus::default()),
                fight_chart_omitted: false,
            },
        );
        maxed_store::apply_batch(&mut db, &events).unwrap();
        maxed_store::accounts::record_recovery_progress(&mut db, &account, 4095).unwrap();
    }
    let writer = Arc::new(
        maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()).unwrap(),
    );
    let home = Arc::new(HomeReads::default());
    *home.context.lock().unwrap() = Some(context(path.clone(), Namespace::Live));
    let (send, receive) = std::sync::mpsc::channel();
    let mut recovery = Recovery::start(
        home.clone(),
        writer.clone(),
        path.clone(),
        key,
        Arc::new(move |notice| {
            send.send(notice).unwrap();
        }),
    );
    let mut recovery = recovery.take().unwrap();
    recovery.observe(&account);
    let notice = receive
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(matches!(
        notice.reason,
        AccountHistoryChangeReason::LegacyRecovered
    ));
    recovery.observe(&account);
    recovery.stop();
    assert!(receive.try_recv().is_err());
    let reader = maxed_store::open_reader(&path).unwrap();
    assert_eq!(reader.recovery_progress(&account).unwrap(), Some(65535));
    drop(reader);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account_id=?1",
            [&account],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account_id IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    drop(db);
    drop(recovery);
    Arc::try_unwrap(writer).ok().unwrap().shutdown().unwrap();
    home.shutdown_reads();
}

#[test]
fn account_scope_loss_and_provisional_keep_last_verified() {
    let home = HomeReads::default();
    *home.context.lock().unwrap() = Some(context("unused".into(), Namespace::Live));
    for (character, account) in [
        (Some("c"), Some("account-a")),
        (None, None),
        (Some("provisional"), None),
    ] {
        let event = EngineEvent::LocalIdentityChanged {
            at: Timestamp(0),
            character_id: character.map(str::to_owned),
            account_id: account.map(str::to_owned),
        };
        if let Some(account) = verified_account(&event) {
            home.observe_account(Some("synthetic"), account, None);
        }
        assert_eq!(
            home.context
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .account
                .as_deref(),
            Some("account-a")
        );
    }
    let mut character = demo::demo_snapshot().character.unwrap();
    character.account_id = Some("account-b".into());
    character.identity_verified = false;
    assert!(verified_account(&EngineEvent::CharacterUpdated(character.clone())).is_none());
    character.identity_verified = true;
    assert_eq!(
        verified_account(&EngineEvent::CharacterUpdated(character)),
        Some("account-b")
    );
}

#[test]
fn account_scope_demo_never_starts_recovery() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("demo.sqlite3");
    let writer = Arc::new(
        maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()).unwrap(),
    );
    let home = Arc::new(HomeReads::default());
    *home.context.lock().unwrap() = Some(context(path.clone(), Namespace::Demo));
    assert!(Recovery::start(
        home,
        writer.clone(),
        path,
        [0; 32],
        Arc::new(|_| panic!("demo recovery notice"))
    )
    .is_none());
    Arc::try_unwrap(writer).ok().unwrap().shutdown().unwrap();
}

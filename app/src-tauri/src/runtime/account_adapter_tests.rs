use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[test]
fn account_scope_live_adapter_notifies_pins_transition_and_starts_one_sweep() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let account = format!("account-{}", "a".repeat(64));
    let key = [7; 32];
    let home = Arc::new(super::super::home::HomeReads::default());
    *home.context.lock().unwrap() = Some(super::super::home::HomeReadContext {
        database: path.clone(),
        namespace: super::super::Namespace::Live,
        generation: 0,
        stream_id: Some("synthetic".into()),
        absent: false,
        account: None,
        account_revision: 0,
        identity_sequence: None,
    });
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    *home.on_recovery_attempt.lock().unwrap() = Some(Arc::new(move || {
        count.fetch_add(1, Ordering::AcqRel);
    }));
    let (batch, reached) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    *home.on_recovery_batch.lock().unwrap() = Some(Arc::new(move |_| {
        batch.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let (notice, notices) = crossbeam_channel::unbounded();
    let announced = Arc::new(Mutex::new(Vec::new()));
    let recorded = announced.clone();
    let notice_home = home.clone();
    let mut adapter = Adapter::open(
        path,
        unavailable_catalog(),
        EngineConfig {
            stream_id: "synthetic".into(),
            identity_key: key,
            ..Default::default()
        },
        WriterConfig {
            stream_id: "synthetic".into(),
            flush_millis: 60_000,
            ..Default::default()
        },
        true,
        Arc::new(Views::default()),
        Bridge {
            account_changed: Arc::new(move |value| {
                recorded.lock().unwrap().push(
                    notice_home
                        .context
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .identity_sequence,
                );
                notice.send(value).unwrap();
            }),
            ..Default::default()
        },
    )
    .unwrap();
    adapter.home = Some(home.clone());
    adapter.live = true;
    let mut character = demo::demo_snapshot().character.unwrap();
    character.identity_verified = true;
    character.account_id = Some(account.clone());
    let identity = EngineEvent::LocalIdentityChanged {
        at: Timestamp(1),
        character_id: Some(character.id.clone()),
        account_id: Some(account),
    };
    adapter.events(vec![
        EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
        EngineEvent::CharacterUpdated(character.clone()),
        identity.clone(),
    ]);
    assert!(matches!(
        notices.recv_timeout(Duration::from_secs(5)).unwrap().reason,
        AccountHistoryChangeReason::AccountSwitched
    ));
    assert_eq!(
        *announced.lock().unwrap(),
        vec![Some(Sequence(Timestamp(3)))]
    );
    assert_eq!(
        home.context
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .identity_sequence,
        Some(Sequence(Timestamp(3)))
    );
    adapter.drain().unwrap();
    reached.recv_timeout(Duration::from_secs(10)).unwrap();
    for _ in 0..10 {
        adapter.events(vec![
            EngineEvent::CharacterUpdated(character.clone()),
            identity.clone(),
        ]);
    }
    adapter.drain().unwrap();
    assert!(notices.try_recv().is_err());
    assert_eq!(attempts.load(Ordering::Acquire), 1);
    adapter.recovery.as_ref().unwrap().cancel();
    release.send(()).unwrap();
    adapter.close().unwrap();
    assert!(notices.try_recv().is_err());
    home.shutdown_reads();
}

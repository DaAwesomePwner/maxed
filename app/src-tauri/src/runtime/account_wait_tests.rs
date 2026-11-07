use super::tests::context;
use super::*;
use maxed_types::*;

fn waiting_runtime() -> (tempfile::TempDir, Arc<CaptureRuntime>) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    maxed_store::migrate(&mut rusqlite::Connection::open(&path).unwrap()).unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let mut current = context(path, Namespace::Live);
    current.account = Some(format!("account-{}", "a".repeat(64)));
    current.identity_sequence = Some(Sequence(Timestamp(3)));
    *runtime.shared.home.context.lock().unwrap() = Some(current);
    runtime.shared.home.preparation.lock().unwrap().ready = true;
    let mut status = runtime.shared.views.store.lock().unwrap();
    status.state = StoreState::Ready;
    status.stream_id = Some("synthetic".into());
    status.durable = Some(Sequence(Timestamp(2))); // CharacterUpdated only.
    drop(status);
    (root, runtime)
}
fn read_waiting(runtime: &CaptureRuntime) -> Result<Option<String>, String> {
    let owner = runtime.shared.home.clone();
    let mut context = owner.context.lock().unwrap().clone().unwrap();
    // A queued request must pick up the later identity transition watermark.
    context.identity_sequence = Some(Sequence(Timestamp(2)));
    runtime.read_account_history(
        HomeJob {
            owner,
            context,
            account_scoped: true,
        },
        || None,
        |_, account| Ok(account.map(str::to_owned)),
    )
}
#[test]
fn account_scope_failed_writer_does_not_wait_for_identity() {
    let (_root, runtime) = waiting_runtime();
    runtime.shared.views.store.lock().unwrap().state = StoreState::Failed;
    *runtime.shared.home.on_identity_wait.lock().unwrap() =
        Some(Arc::new(|| panic!("failed writer must not wait")));
    assert_eq!(read_waiting(&runtime).unwrap_err(), UNAVAILABLE);
}
#[test]
fn account_scope_waits_for_identity_transition_not_character_projection() {
    let (_root, runtime) = waiting_runtime();
    let (send, entered) = crossbeam_channel::bounded(1);
    *runtime.shared.home.on_identity_wait.lock().unwrap() = Some(Arc::new(move || {
        let _ = send.try_send(());
    }));
    let reading = runtime.clone();
    let worker = std::thread::spawn(move || read_waiting(&reading));
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(!worker.is_finished());
    runtime.shared.views.store.lock().unwrap().durable = Some(Sequence(Timestamp(3)));
    assert!(worker.join().unwrap().unwrap().is_some());
}
#[test]
fn account_scope_writer_failure_interrupts_identity_wait() {
    let (_root, runtime) = waiting_runtime();
    let (send, entered) = crossbeam_channel::bounded(1);
    *runtime.shared.home.on_identity_wait.lock().unwrap() = Some(Arc::new(move || {
        let _ = send.try_send(());
    }));
    let reading = runtime.clone();
    let worker = std::thread::spawn(move || read_waiting(&reading));
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    runtime.shared.views.store.lock().unwrap().state = StoreState::Failed;
    assert_eq!(worker.join().unwrap().unwrap_err(), UNAVAILABLE);
}

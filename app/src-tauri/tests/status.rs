use maxed_app::runtime::CaptureRuntime;
use maxed_types::CaptureState;

#[test]
fn initial_status_and_repeated_stop_are_stopped() {
    let runtime = CaptureRuntime::default();
    assert_eq!(runtime.status().state, CaptureState::Stopped);
    runtime.stop().unwrap();
    runtime.stop().unwrap();
    assert_eq!(runtime.status().state, CaptureState::Stopped);
}

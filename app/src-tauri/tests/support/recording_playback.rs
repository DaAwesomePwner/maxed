use super::super::*;

#[test]
fn recording_defaults_off_and_no_files_for_replay() {
    for opted_in in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if opted_in {
            let settings = maxed_types::AppSettings {
                save_recordings: true,
                ..Default::default()
            };
            crate::settings::SettingsStore::new(root.path().into())
                .unwrap()
                .put(&settings)
                .unwrap();
        }
        let runtime = CaptureRuntime::default();
        let mut options = RuntimeOptions::live(root.path().into(), None);
        options.namespace = Namespace::Replay;
        let fixture = crate::demo::DemoResources::development_root().join("story.pcap");
        runtime
            .replay_reader(
                options,
                std::fs::File::open(fixture).unwrap(),
                Bridge::default(),
            )
            .unwrap();
        assert!(runtime.snapshot().sequence.0 .0 > 0);
        assert!(!root.path().join("recordings").exists());
        runtime.shutdown().unwrap();
        assert!(!root.path().join("recordings").exists());
    }
}

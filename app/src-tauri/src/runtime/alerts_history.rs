//! Invented bag events, injected calendar, isolated namespace databases.
use super::*;
use maxed_engine::EngineConfig;
use maxed_store::WriterConfig;
use maxed_types::*;

struct Calendar {
    window: Option<TimeRange>,
    seen: Arc<Mutex<Vec<Timestamp>>>,
}
impl super::alerts::AlertCalendar for Calendar {
    fn seven_day_window(&self, at: Timestamp) -> Result<TimeRange, String> {
        self.seen.lock().unwrap().push(at);
        self.window
            .clone()
            .ok_or_else(|| "Injected unavailable zone".into())
    }
}

#[test]
fn injected_calendar_uses_drop_clock_and_preserves_unavailable_context() {
    let dir = tempfile::tempdir().unwrap();
    for namespace in [Namespace::Live, Namespace::Demo, Namespace::Replay] {
        for available in [true, false] {
            let mut options =
                RuntimeOptions::live(dir.path().join(format!("{namespace:?}-{available}")), None);
            options.namespace = namespace;
            let path = options.database();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let catalog = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
            ))
            .unwrap();
            let mut adapter = Adapter::open(
                path.clone(),
                Arc::new(catalog),
                EngineConfig {
                    stream_id: "calendar-test".into(),
                    ..Default::default()
                },
                WriterConfig {
                    stream_id: "calendar-test".into(),
                    flush_millis: 60_000,
                    ..Default::default()
                },
                false,
                Arc::new(Views::default()),
                Bridge::default(),
            )
            .unwrap();
            adapter.ephemeral = namespace != Namespace::Live;
            adapter.live = namespace == Namespace::Live;
            let seen = Arc::new(Mutex::new(Vec::new()));
            let window = TimeRange {
                start: Timestamp(1_000_000),
                end: Timestamp(604_801_000_000),
            };
            adapter.alert_calendar = Arc::new(Calendar {
                window: available.then(|| window.clone()),
                seen: seen.clone(),
            });
            let session = demo::demo_snapshot().session.unwrap();
            adapter.events(vec![EngineEvent::SessionUpdated(session)]);
            let mut drop = demo::demo_run_story().drops.remove(0);
            drop.run_id = None;
            drop.at = Timestamp(5_000_000);
            drop.bag_rank = Some(255);
            // The adapter clock is still zero; the bag's clock selects its dates.
            adapter.events(vec![EngineEvent::BagDropped(drop.clone())]);
            adapter.drain().unwrap();
            adapter.writer_flush_for_test();
            let reader = maxed_store::open_reader(&path).unwrap();
            let filter = AlertFilter {
                kinds: vec![],
                exclude_kinds: vec![],
                kind: None,
                range: None,
                query: String::new(),
                cursor: None,
                limit: 50,
            };
            let original = reader.list_alerts(filter.clone()).unwrap().rows.remove(0);
            assert!(
                matches!(&original.details, Some(AlertDetails::WhiteBag { ordinal, window: saved, .. })
                if *ordinal == available.then_some(1) && *saved == available.then(|| window.clone()))
            );
            assert_eq!(*seen.lock().unwrap(), vec![drop.at]);
            adapter.alert_calendar = Arc::new(Calendar {
                window: None,
                seen: seen.clone(),
            });
            adapter.events(vec![EngineEvent::DropUpdated(drop)]);
            adapter.drain().unwrap();
            adapter.writer_flush_for_test();
            assert_eq!(reader.list_alerts(filter).unwrap().rows, vec![original]);
            adapter.close().unwrap();
        }
    }
}

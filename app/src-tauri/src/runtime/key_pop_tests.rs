//! Synthetic notice-to-store coverage; no recordings or native effects.
use super::history_replay_tests::reconstruction::*;
use super::join::{Adapter, Bridge, Views};
use maxed_types::*;
use std::sync::Arc;

#[test]
fn key_pop_notice_is_live_only_filtered_private_and_idempotent() {
    for (stream, identity, own, selected, expected) in [
        ("live-keys", true, false, None, 1),
        ("live-keys", true, true, None, 0),
        ("live-keys", true, false, Some(vec!["Lost Halls".into()]), 0),
        ("live-keys", true, false, Some(vec![]), 0),
        ("live-keys", true, false, Some(vec!["The Void".into()]), 1),
        ("replay-keys", true, false, None, 0),
        ("demo-keys", true, false, None, 0),
        ("live-keys", false, false, None, 0),
        ("live-keys", false, true, None, 0),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.sqlite3");
        let mut catalog = catalog();
        catalog.objects[2].dungeon_name = Some("The Void".into());
        let views = Arc::new(Views::default());
        views.alert_policy.lock().unwrap().key_pop_dungeons = selected;
        let mut adapter = Adapter::open(
            path.clone(),
            Arc::new(catalog),
            maxed_engine::EngineConfig {
                stream_id: stream.into(),
                ..config()
            },
            maxed_store::WriterConfig {
                stream_id: stream.into(),
                ..Default::default()
            },
            false,
            views,
            Bridge::default(),
        )
        .unwrap();
        adapter.live = stream == "live-keys";
        for (at, packet) in [
            (0, map("Nexus")),
            (1000, create(1, 1)),
            (2000, update(vec![player()], vec![])),
            (
                2500,
                tick(vec![status(1, vec![string(31, "local-test-marker")])]),
            ),
        ] {
            if !identity && (at == 1000 || at == 2500) {
                continue;
            }
            let events = apply(&mut adapter.engine, at, packet);
            adapter.events(events);
        }
        let opener = if own {
            "LOCAL-TEST-MARKER"
        } else {
            "remote-test-marker"
        };
        let packet = maxed_protocol::Packet::Notification(maxed_protocol::defs::Notification {
            data: maxed_protocol::defs::NotificationData {
                effect: Some("PortalOpened"),
                pictureType: 301,
                message: Some(
                    serde_json::json!({"k":"s.dungeon_opened_by","t":{"player":opener}})
                        .to_string(),
                ),
                ..Default::default()
            },
        });
        let events = apply(&mut adapter.engine, 3000, packet);
        let notice = events
            .iter()
            .find(|e| matches!(e, EngineEvent::InstanceEvent(_)))
            .unwrap()
            .clone();
        adapter.events(events);
        adapter.events(vec![notice.clone()]);
        adapter.submit_pending_for_test();
        adapter.writer_flush_for_test();
        let db = rusqlite::Connection::open(&path).unwrap();
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM alert_occurrences WHERE kind='key_popped'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, expected,
            "stream={stream}, identity={identity}, own={own}"
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM instance_events WHERE kind='portal_opened'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        if expected == 1 {
            let (source, json): (String, String) = db.query_row("SELECT o.source_key, a.json FROM alert_occurrences o JOIN alerts a ON a.id=o.alert_id WHERE o.kind='key_popped'", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
            let EngineEvent::InstanceEvent(notice) = notice else {
                unreachable!()
            };
            assert_eq!(source, notice.id);
            assert!(!json.contains("test-marker"));
            let alert: AlertView = serde_json::from_str(&json).unwrap();
            assert_eq!(alert.explanation, "The Void key popped");
            assert_eq!(
                super::alerts::notifications::content(&alert).0,
                "The Void key popped"
            );
        }
        drop(db);
        adapter.close().unwrap();
    }
}

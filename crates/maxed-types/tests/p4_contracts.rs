use maxed_types::*;
use ts_rs::TS;

#[test]
fn summary_rows_and_history_replies_preserve_wire_evidence() {
    let mut story = demo::demo_run_story();
    story.fights[0].started = Timestamp(u64::MAX);
    let value = serde_json::to_value(&story).unwrap();
    let row = &value["fights"][0];
    for key in ["players", "sources", "buckets"] {
        assert!(row.get(key).is_none());
    }
    assert_eq!(row["started"], u64::MAX.to_string());
    assert_eq!(row["ordinal"], 1);
    assert!(row["target_name"].is_null());
    let reply = HistoryReply::Ready { value: Some(story) };
    assert_eq!(serde_json::to_value(reply).unwrap()["status"], "ready");
    assert_eq!(
        serde_json::to_value(HistoryReply::<RunStory>::Superseded).unwrap(),
        serde_json::json!({"status":"superseded"})
    );
    assert_eq!(
        serde_json::to_value(HistoryReply::<RunStory>::Busy).unwrap(),
        serde_json::json!({"status":"busy"})
    );
    let declaration = HistoryReply::<RunStory>::decl();
    for status in ["ready", "superseded", "busy"] {
        assert!(declaration.contains(status));
    }
    let mut row = story_row();
    row.local_damage = None;
    row.local_dps = None;
    row.local_rank = None;
    let json = serde_json::to_value(row).unwrap();
    for key in ["local_damage", "local_dps", "local_rank"] {
        assert!(json.get(key).unwrap().is_null());
    }
}

fn story_row() -> RunFightSummary {
    demo::demo_run_story().fights.remove(0)
}

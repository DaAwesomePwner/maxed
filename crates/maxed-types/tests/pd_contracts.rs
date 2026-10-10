use maxed_types::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

const FIELDS: &[(&str, &[&str])] = &[
    ("RunView", &["context"]),
    (
        "FightView",
        &["kill", "target_hp", "target_kind", "player_series"],
    ),
    ("DropView", &["capture_scope"]),
    ("CharacterView", &["stat_panel"]),
    ("LiveSnapshot", &["instance", "run_totals"]),
    (
        "RunStory",
        &[
            "totals",
            "boss_fights",
            "boss_fights_limited",
            "events",
            "events_limited",
            "older_run_id",
            "newer_run_id",
        ],
    ),
    ("HomeSummary", &["dungeon_stats"]),
    ("ItemDef", &["soulbound", "stat_bonuses"]),
    ("ObjectDef", &["class_name", "dungeon_name", "event_boss"]),
    ("RunFilter", &["character_id"]),
    ("RunPage", &["details", "summary"]),
    ("RunRowDetail", &["previous_best_clear_micros"]),
];

fn legacy<T: DeserializeOwned + Serialize>(mut value: Value, name: &str) {
    let fields = FIELDS.iter().find(|(ty, _)| *ty == name).unwrap().1;
    for field in fields {
        value.as_object_mut().unwrap().remove(*field);
    }
    let restored =
        serde_json::to_value(serde_json::from_value::<T>(value.clone()).unwrap()).unwrap();
    for field in fields {
        assert_eq!(restored.get(*field), Some(&Value::Null), "{name}.{field}");
        value[*field] = Value::Null;
    }
    assert_eq!(restored, value);
}

#[test]
fn legacy_json_without_new_fields_deserializes() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/p1-demo.json")).unwrap();
    let snapshot = &fixture["snapshot"];
    legacy::<RunView>(snapshot["run"].clone(), "RunView");
    legacy::<FightView>(snapshot["fights"][0].clone(), "FightView");
    legacy::<DropView>(snapshot["drops"][0].clone(), "DropView");
    legacy::<CharacterView>(snapshot["character"].clone(), "CharacterView");
    legacy::<LiveSnapshot>(snapshot.clone(), "LiveSnapshot");
    legacy::<RunStory>(fixture["run_story"].clone(), "RunStory");
    legacy::<HomeSummary>(fixture["home_summary"].clone(), "HomeSummary");
    legacy::<ItemDef>(fixture["catalog"]["items"][0].clone(), "ItemDef");
    legacy::<ObjectDef>(fixture["catalog"]["objects"][0].clone(), "ObjectDef");
    legacy::<RunFilter>(
        json!({"dungeon":null,"outcome":null,"range":null,"sort":"newest","cursor":null,"limit":50}),
        "RunFilter",
    );
    legacy::<RunPage>(json!({"rows":[],"next":null}), "RunPage");
}

#[test]
fn new_fields_serialize_explicit_null() {
    let snapshot = demo::demo_snapshot();
    assert!(snapshot.instance.is_none() && snapshot.run_totals.is_none());
    let values = [
        serde_json::to_value(snapshot.run.as_ref().unwrap()).unwrap(),
        serde_json::to_value(&snapshot.fights[0]).unwrap(),
        serde_json::to_value(&snapshot.drops[0]).unwrap(),
        serde_json::to_value(snapshot.character.as_ref().unwrap()).unwrap(),
        serde_json::to_value(&snapshot).unwrap(),
        serde_json::to_value(demo::demo_run_story()).unwrap(),
        serde_json::to_value(demo::demo_home_summary()).unwrap(),
        serde_json::to_value(&demo::demo_catalog().items[0]).unwrap(),
        serde_json::to_value(&demo::demo_catalog().objects[0]).unwrap(),
        serde_json::to_value(RunFilter {
            character_id: None,
            dungeon: None,
            outcome: None,
            range: None,
            sort: RunSort::Newest,
            cursor: None,
            limit: 50,
        })
        .unwrap(),
        serde_json::to_value(RunPage {
            rows: vec![],
            next: None,
            details: None,
            summary: None,
        })
        .unwrap(),
    ];
    for ((name, fields), value) in FIELDS.iter().zip(values) {
        for field in *fields {
            assert_eq!(value.get(*field), Some(&Value::Null), "{name}.{field}");
        }
    }
}

#[test]
fn fight_end_killed_and_timeline_kinds_are_snake_case() {
    assert_eq!(serde_json::to_value(FightEnd::Killed).unwrap(), "killed");
    assert_eq!(
        serde_json::from_value::<FightEnd>(json!("killed")).unwrap(),
        FightEnd::Killed
    );
    for (kind, name) in [
        (TimelineKind::PartyDeath, "party_death"),
        (TimelineKind::PlayerLeft, "player_left"),
        (TimelineKind::PortalOpened, "portal_opened"),
        (TimelineKind::Kill, "kill"),
    ] {
        assert_eq!(serde_json::to_value(&kind).unwrap(), name);
        assert_eq!(
            serde_json::from_value::<TimelineKind>(json!(name)).unwrap(),
            kind
        );
    }
}

#[test]
fn instance_event_kind_is_internally_tagged() {
    for value in [
        json!({"type":"player_death", "player_name":"SynthAlpha", "level":null,
            "killed_by":null, "grave_type":null, "object_id":null, "class_id":null, "is_local":false}),
        json!({"type":"player_left", "object_id":123, "player_name":"SynthBravo", "class_id":null}),
        json!({"type":"portal_opened", "opener":null, "portal_type":null, "dungeon":null, "opener_is_local":null}),
        json!({"type":"realm_event_killed", "boss_key":"Synthetic", "object_type":null, "killer":null}),
    ] {
        let kind: InstanceEventKind = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&kind).unwrap(), value);
        let event = EngineEvent::InstanceEvent(InstanceEventView {
            id: "event-1".into(),
            session_id: "session-1".into(),
            run_id: None,
            map: None,
            at: Timestamp(123),
            kind,
            provisional: true,
        });
        let serialized = serde_json::to_value(&event).unwrap();
        assert_eq!(serialized["instance_event"]["kind"], value);
        assert_eq!(
            serde_json::from_value::<EngineEvent>(serialized).unwrap(),
            event
        );
    }
}

fn total() -> RunTotals {
    serde_json::from_value(
        json!({"fights":0,"kills":0,"first_hit":null,"last_hit":null,
        "total_damage":0.0,"unattributed_damage":0.0,"players":[],"local":null,
        "fame_delta":null,"partial":false}),
    )
    .unwrap()
}

#[test]
fn non_finite_floats_rejected() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut context: AreaContext = serde_json::from_value(json!({
            "entered_from_realm":false,"modifiers":[] }))
        .unwrap();
        context.difficulty = Some(invalid);
        assert!(serde_json::to_value(context).is_err());
        let series = PlayerSeries {
            player_id: "synthetic".into(),
            object_id: 123,
            is_local: true,
            points: vec![0.0, invalid],
        };
        assert!(serde_json::to_value(series).is_err());
        for field in ["total_damage", "unattributed_damage"] {
            let mut totals = total();
            if field == "total_damage" {
                totals.total_damage = invalid;
            } else {
                totals.unattributed_damage = invalid;
            }
            assert!(serde_json::to_value(totals).is_err());
        }
        for field in ["damage", "share", "dps"] {
            let mut player: RunPlayerTotal = serde_json::from_value(json!({
                "player_id":"synthetic", "is_local":true,"damage":1.0,"hits":1,"deaths":0}))
            .unwrap();
            match field {
                "damage" => player.damage = invalid,
                "share" => player.share = Some(invalid),
                _ => player.dps = Some(invalid),
            }
            assert!(serde_json::to_value(player).is_err());
        }
        let row = DungeonStatsRow {
            dungeon: "Synthetic".into(),
            runs: 1,
            clears: 1,
            deaths: 0,
            observed_clears: 1,
            mean_clear_micros: Some(invalid),
            best_clear_micros: None,
            all_time_best_clear_micros: None,
            last_started: Timestamp(0),
        };
        assert!(serde_json::to_value(row).is_err());
    }
    let valid = PlayerSeries {
        player_id: "synthetic".into(),
        object_id: 123,
        is_local: true,
        points: vec![0.0, 1.5, f64::MAX],
    };
    assert_eq!(
        serde_json::from_value::<PlayerSeries>(serde_json::to_value(&valid).unwrap()).unwrap(),
        valid
    );
}

#[test]
fn generated_bindings_mark_new_fields_optional_nullable() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/ui/src/lib/bindings");
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "ts") {
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                !text.contains("bigint"),
                "{} contains bigint",
                path.display()
            );
        }
    }
    for (name, fields) in FIELDS {
        let text = std::fs::read_to_string(root.join(format!("{name}.ts"))).unwrap();
        for field in *fields {
            let marker = format!("{field}?:");
            let rest = text
                .split_once(&marker)
                .unwrap_or_else(|| panic!("{name}.{field} not optional"))
                .1;
            assert!(
                rest.split(',').next().unwrap().contains("| null"),
                "{name}.{field} not nullable"
            );
        }
    }
}

#[test]
fn time_fields_match_json_wire_conventions() {
    let context: AreaContext = serde_json::from_value(json!({
        "entered_from_realm": false,
        "modifiers": [],
        "game_opened_epoch_s": "1800000000",
        "instance_age_micros": 120000000,
    }))
    .unwrap();
    assert_eq!(context.game_opened_epoch_s, Some(1_800_000_000));
    assert_eq!(context.instance_age_micros, Some(120_000_000));
    let value = serde_json::to_value(&context).unwrap();
    assert_eq!(value["game_opened_epoch_s"], json!("1800000000"));
    assert_eq!(value["instance_age_micros"], json!(120_000_000));
    for seconds in [i64::MIN, i64::MAX] {
        let mut boundary = context.clone();
        boundary.game_opened_epoch_s = Some(seconds);
        let value = serde_json::to_value(&boundary).unwrap();
        assert_eq!(value["game_opened_epoch_s"], json!(seconds.to_string()));
        assert_eq!(
            serde_json::from_value::<AreaContext>(value).unwrap(),
            boundary
        );
    }
    let row: DungeonStatsRow = serde_json::from_value(json!({
        "dungeon": "Synthetic", "runs": 1, "clears": 1, "deaths": 0,
        "observed_clears": 1, "mean_clear_micros": 120000000.0,
        "best_clear_micros": 120000000, "last_started": "1800000000000000",
    }))
    .unwrap();
    assert_eq!(row.best_clear_micros, Some(120_000_000));
    assert_eq!(row.all_time_best_clear_micros, None);
    assert_eq!(
        serde_json::to_value(row).unwrap()["best_clear_micros"],
        json!(120_000_000)
    );

    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/ui/src/lib/bindings");
    let context_ts = std::fs::read_to_string(root.join("AreaContext.ts")).unwrap();
    assert!(context_ts.contains("game_opened_epoch_s?: string | null"));
    assert!(context_ts.contains("instance_age_micros?: number | null"));
    let stats_ts = std::fs::read_to_string(root.join("DungeonStatsRow.ts")).unwrap();
    assert!(stats_ts.contains("best_clear_micros?: number | null"));
    assert!(stats_ts.contains("all_time_best_clear_micros?: number | null"));
}

#[test]
fn runs_list_integers_keep_the_numeric_json_wire_contract() {
    use ts_rs::TS;
    let detail = RunRowDetail::decl();
    for field in [
        "clear_micros: number | null",
        "best_clear_micros: number | null",
        "previous_best_clear_micros?: number | null",
    ] {
        assert!(detail.contains(field), "{detail}");
    }
    let summary = RunListSummary::decl();
    for field in ["time_in_runs_micros: number", "fame_delta: number | null"] {
        assert!(summary.contains(field), "{summary}");
    }
}

#[test]
fn legacy_run_details_and_pages_decode_without_previous_best() {
    let old_detail = json!({
        "run_id": "test-run", "character_name": null, "class_id": null,
        "local_share": null, "share_partial": false, "fame_delta": null,
        "loot_preview": [], "observed_drop_count": 0,
        "clear_micros": 552000000, "best_clear_micros": 552000000,
        "is_personal_best": true
    });
    legacy::<RunRowDetail>(old_detail.clone(), "RunRowDetail");
    let page: RunPage = serde_json::from_value(json!({
        "rows": [], "next": null, "details": [old_detail]
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(page).unwrap()["details"][0].get("previous_best_clear_micros"),
        Some(&Value::Null)
    );
    let modern: RunRowDetail = serde_json::from_value(json!({
        "run_id": "test-run", "character_name": null, "class_id": null,
        "local_share": null, "share_partial": false, "fame_delta": null,
        "loot_preview": [], "observed_drop_count": 0,
        "clear_micros": 552000000, "best_clear_micros": 552000000,
        "is_personal_best": true, "previous_best_clear_micros": 580000000
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(modern).unwrap()["previous_best_clear_micros"],
        json!(580000000)
    );
}

#[test]
fn legacy_event_page_decodes_without_entered_runs() {
    let page: maxed_types::EventPage = serde_json::from_str(r#"{"rows":[],"next":null}"#).unwrap();
    assert!(page.rows.is_empty());
    assert!(page.next.is_none());
    assert!(page.entered_runs.is_empty());
}

use maxed_types::{demo::*, *};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::fmt::Debug;
use ts_rs::TS;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) -> Value {
    let text = serde_json::to_string(value).unwrap();
    assert_eq!(&serde_json::from_str::<T>(&text).unwrap(), value);
    serde_json::from_str(&text).unwrap()
}

#[test]
fn name_key_strips_suffix_and_lowercases() {
    for (name, expected) in [
        ("SynthAlpha,fe14", "synthalpha"),
        ("SYNTHalpha", "synthalpha"),
        ("SynthBravo,fe14,extra", "synthbravo"),
        (",fe14", ""),
        ("", ""),
    ] {
        assert_eq!(party::name_key(name), expected);
    }
}

#[test]
fn party_players_cursor_round_trips_exact_sort_bits() {
    for value in [1.0_f64 / 3.0, 2.0 / 3.0, 0.0, -1.0, f64::MAX] {
        let cursor = PartyPlayersCursor {
            sort_value: value.to_bits().to_string(),
            key: "synthalpha".into(),
        };
        assert_eq!(
            round_trip(&cursor)["sort_value"],
            value.to_bits().to_string()
        );
        let encoded = serde_json::to_string(&cursor).unwrap();
        let decoded: PartyPlayersCursor = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.sort_value.parse::<u64>().unwrap(), value.to_bits());
    }
    assert!(PartyPlayersCursor::decl().contains("sort_value: string"));
}

#[test]
fn settings_without_party_load_with_defaults() {
    let settings: AppSettings = serde_json::from_value(json!({
        "version": 1,
        "adapter_id": null,
        "assets_path": null,
        "save_recordings": false,
        "recording_limit_bytes": 1_000_000_000,
        "update_channel": "stable"
    }))
    .unwrap();
    assert_eq!(settings.party, PartySettings::default());
    assert!(settings.party.presets.is_empty());
    assert_eq!(settings.party.default_preset_id, None);
    assert_eq!(round_trip(&settings)["version"], 1);
}

#[test]
fn player_presence_is_not_an_engine_event() {
    for (run_id, present) in [(None, false), (Some("synthetic-run".into()), true)] {
        let presence = PlayerPresence {
            scope_id: "synthetic-scope".into(),
            run_id,
            player_id: "synthetic-player".into(),
            object_id: 1,
            present,
            at: Timestamp(u64::MAX),
        };
        assert_eq!(round_trip(&presence), json!(presence));
        assert!(
            serde_json::from_value::<EngineEvent>(json!({"player_presence": presence})).is_err()
        );
    }
    assert!(!EngineEvent::decl().contains("player_presence"));
}

#[test]
fn fight_chart_marker_defaults_off_and_preserves_legacy_envelope_bytes() {
    let legacy =
        r#"{"stream_id":"synthetic","sequence":"1","at":"2","event":{"fight_updated":null}}"#;
    let mut value: Value = serde_json::from_str(legacy).unwrap();
    value["event"]["fight_updated"] =
        serde_json::to_value(demo_snapshot().fights.remove(0)).unwrap();
    let mut envelope: EventEnvelope = serde_json::from_value(value.clone()).unwrap();
    assert!(!envelope.fight_chart_omitted);
    assert_eq!(round_trip(&envelope), value);
    // Compare ordered struct serialization, not just JSON value equality: receipt
    // hashing must see exactly the same bytes as before this optional field.
    #[derive(Serialize)]
    struct LegacyEnvelope<'a> {
        stream_id: &'a str,
        sequence: Sequence,
        at: Timestamp,
        event: &'a EngineEvent,
    }
    let old = LegacyEnvelope {
        stream_id: &envelope.stream_id,
        sequence: envelope.sequence,
        at: envelope.at,
        event: &envelope.event,
    };
    assert_eq!(
        serde_json::to_vec(&envelope).unwrap(),
        serde_json::to_vec(&old).unwrap()
    );
    envelope.fight_chart_omitted = true;
    assert_eq!(round_trip(&envelope)["fight_chart_omitted"], true);
    assert!(EventEnvelope::decl().contains("fight_chart_omitted?: boolean"));
    value["fight_chart_omitted"] = json!(false);
    assert!(
        !serde_json::from_value::<EventEnvelope>(value)
            .unwrap()
            .fight_chart_omitted
    );
}

#[test]
fn legacy_enchant_metadata_is_loadable_without_claiming_a_neutral_multiplier() {
    let mut e: EnchantDef =
        serde_json::from_value(json!({"id":123,"name":"Synthetic","description":""})).unwrap();
    assert_eq!(e.stat_damage_multiplier, None);
    e.stat_damage_multiplier = Some(1.25);
    assert_eq!(round_trip(&e)["stat_damage_multiplier"], 1.25);
    assert!(EnchantDef::decl().contains("stat_damage_multiplier: number | null"));
    e.stat_damage_multiplier = Some(f64::INFINITY);
    assert!(serde_json::to_string(&e).is_err());
}

#[test]
fn legacy_weapon_damage_metadata_stays_absent_and_unsupported_has_no_multipliers() {
    let mut e: EnchantDef =
        serde_json::from_value(json!({"id":123,"name":"Synthetic","description":""})).unwrap();
    assert_eq!(e.weapon_damage, None);
    assert!(round_trip(&e).get("weapon_damage").is_none());
    let unsupported: EnchantWeaponDamageDef = serde_json::from_value(
        json!({"support":"unsupported","reasons":["unsupported_subattack"]}),
    )
    .unwrap();
    assert_eq!(unsupported.min_multiplier, None);
    assert_eq!(unsupported.max_multiplier, None);
    e.weapon_damage = Some(EnchantWeaponDamageDef {
        support: CatalogSupport::Supported,
        min_multiplier: Some(0.75),
        max_multiplier: Some(0.75),
        reasons: vec![],
    });
    assert_eq!(round_trip(&e)["weapon_damage"]["min_multiplier"], 0.75);
    e.weapon_damage.as_mut().unwrap().max_multiplier = Some(f64::NAN);
    assert!(serde_json::to_string(&e).is_err());
}

#[test]
fn unknown_enchant_and_large_sequence_survive_json() {
    let value = EnchantView {
        state: EnchantState::Missing,
        slots: None,
        applied: None,
        ids: vec![],
    };
    let json = serde_json::to_value(&value).unwrap();
    assert!(json["slots"].is_null());
    assert_eq!(json["state"], "missing");
    assert_eq!(
        serde_json::to_string(&Sequence(Timestamp(u64::MAX))).unwrap(),
        "\"18446744073709551615\""
    );
    round_trip(&value);
    for sequence in [0, 9_007_199_254_740_993, u64::MAX] {
        assert_eq!(
            round_trip(&Sequence(Timestamp(sequence))),
            sequence.to_string()
        );
    }
    for invalid in [
        "0",
        "9007199254740993",
        "null",
        "\"-1\"",
        "\"18446744073709551616\"",
    ] {
        assert!(serde_json::from_str::<Sequence>(invalid).is_err());
    }
}

#[test]
fn snapshot_preserves_partial_data_and_p0_capture_shape() {
    let snapshot = demo_snapshot();
    let json = round_trip(&snapshot);
    assert_eq!(json["capture"]["state"], "Stopped");
    assert_eq!(json["sequence"], "12");
    assert_eq!(json["at"], "5000000");
    assert_eq!(json["character"]["identity_verified"], false);
    assert_eq!(json["character"]["maxed"], Value::Null);
    assert_eq!(json["fights"][0]["max_hp"], Value::Null);
    assert_eq!(json["drops"][0]["bag_rank"], Value::Null);
    assert_eq!(json["drops"][0]["source_enemy"], Value::Null);
    assert!(json.get("catalog").is_none());

    let mut unknown = snapshot;
    unknown.session = None;
    unknown.character = None;
    unknown.map = None;
    unknown.run = None;
    unknown.area_kind = AreaKind::Unknown;
    unknown.fights.clear();
    unknown.drops.clear();
    unknown.issues = vec![CoverageIssue::Midstream, CoverageIssue::MissingCatalog];
    let json = round_trip(&unknown);
    for field in ["session", "character", "map", "run"] {
        assert_eq!(
            json.get(field),
            Some(&Value::Null),
            "{field} must be present"
        );
    }
    assert_eq!(json["area_kind"], "unknown");
}

#[test]
fn realm_target_and_unquantified_sources_remain_representable() {
    let mut fight = demo_snapshot().fights.remove(0);
    fight.run_id = None;
    fight.boss_type = None;
    fight.first_hit = Some(Timestamp(2_000_000));
    fight.last_hit = fight.first_hit;
    let player = &mut fight.players[0];
    player.name = None;
    player.class_id = None;
    player.biggest_hit = None;
    player.dps = None;
    player.share = None;
    player.rank = None;
    player.sources[0].item_type = None;
    player.sources[0].biggest_hit = None;
    player.sources[0].estimated = true;
    player.sources[0].reasons = vec!["synthetic_missing_projectile".into()];
    fight.issues.push(CoverageIssue::MissingProjectile);
    let json = round_trip(&fight);
    assert_eq!(json["run_id"], Value::Null);
    assert_eq!(json["boss_type"], Value::Null);
    assert_eq!(json["target_type"], 300);
    assert_eq!(json["players"][0]["dps"], Value::Null);
    assert_eq!(json["players"][0]["sources"][0]["item_type"], Value::Null);
}

#[test]
fn completion_and_death_facts_survive_run_revisions() {
    let mut run = demo_run_story().run;
    run.died_at = Some(Timestamp(5_500_000));
    run.evidence.extend([
        EvidenceAt {
            at: Timestamp(5_500_000),
            evidence: Evidence::DeathPacket,
        },
        EvidenceAt {
            at: Timestamp(5_600_000),
            evidence: Evidence::CompletionDialogue {
                rule: "synthetic_final_boss".into(),
            },
        },
        EvidenceAt {
            at: Timestamp(6_000_000),
            evidence: Evidence::CompletionCounter {
                before: 7,
                after: 8,
                delay_micros: 1_000_000,
            },
        },
    ]);
    let json = round_trip(&EngineEvent::RunUpdated(run.clone()));
    assert_eq!(json["run_updated"]["outcome"], "cleared");
    assert_eq!(json["run_updated"]["completed_at"], "5000000");
    assert_eq!(json["run_updated"]["died_at"], "5500000");
    assert_eq!(
        json["run_updated"]["evidence"][4]["evidence"],
        json!({
            "completion_counter": { "before": 7, "after": 8, "delay_micros": 1_000_000 }
        })
    );
    // The DTO must also carry death-first outcome plus a later completion fact.
    run.outcome = Some(Outcome::Died);
    round_trip(&run);
}

#[test]
fn late_drop_revision_keeps_per_slot_enchants_and_duplicate_item_types() {
    let mut drop = demo_snapshot().drops.remove(0);
    drop.contents.clear();
    drop.items.clear();
    drop.contents_known = false;
    let initial = round_trip(&EngineEvent::BagDropped(drop.clone()));
    assert_eq!(initial["bag_dropped"]["contents_known"], false);

    drop.contents = vec![
        DropItem {
            taken_observed_at: None,
            slot: 0,
            item_type: 200,
            quantity: 1,
            enchant: EnchantView {
                state: EnchantState::RecordedEmpty,
                slots: Some(2),
                applied: Some(0),
                ids: vec![],
            },
        },
        DropItem {
            taken_observed_at: None,
            slot: 1,
            item_type: 200,
            quantity: 1,
            enchant: EnchantView {
                state: EnchantState::Recorded,
                slots: Some(2),
                applied: Some(1),
                ids: vec![987654],
            },
        },
    ];
    drop.items = vec![200, 200];
    drop.contents_known = true;
    drop.source_enemy = Some(300);
    drop.source_estimated = true;
    drop.source_reason = Some("synthetic_nearby_enemy".into());
    let json = round_trip(&EngineEvent::DropUpdated(drop));
    assert_eq!(json["drop_updated"]["items"], json!([200, 200]));
    assert_eq!(json["drop_updated"]["enchant"], Value::Null);
    assert_eq!(
        json["drop_updated"]["contents"][0]["enchant"]["state"],
        "recorded_empty"
    );
    assert_eq!(
        json["drop_updated"]["contents"][1]["enchant"]["ids"],
        json!([987654])
    );
}

#[test]
fn every_event_is_externally_tagged_and_enveloped_with_decimal_strings() {
    let snapshot = demo_snapshot();
    let run = snapshot.run.unwrap();
    let fight = snapshot.fights[0].clone();
    let drop = snapshot.drops[0].clone();
    let events = [
        (
            "capture_state_changed",
            EngineEvent::CaptureStateChanged(snapshot.capture),
        ),
        (
            "map_entered",
            EngineEvent::MapEntered {
                realm_instance_key: None,
                at: snapshot.at,
                name: "Synthetic Realm".into(),
                kind: AreaKind::Realm,
            },
        ),
        (
            "session_updated",
            EngineEvent::SessionUpdated(snapshot.session.unwrap()),
        ),
        ("run_started", EngineEvent::RunStarted(run.clone())),
        ("run_updated", EngineEvent::RunUpdated(run.clone())),
        ("run_ended", EngineEvent::RunEnded(run)),
        ("fight_started", EngineEvent::FightStarted(fight.clone())),
        ("fight_updated", EngineEvent::FightUpdated(fight.clone())),
        ("fight_ended", EngineEvent::FightEnded(fight)),
        ("bag_dropped", EngineEvent::BagDropped(drop.clone())),
        ("drop_updated", EngineEvent::DropUpdated(drop)),
        (
            "player_died",
            EngineEvent::PlayerDied {
                at: snapshot.at,
                character_id: None,
                run_id: None,
            },
        ),
        (
            "character_updated",
            EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        ),
        (
            "character_merged",
            EngineEvent::CharacterMerged {
                provisional_id: "demo-provisional".into(),
                verified_id: "demo-verified".into(),
            },
        ),
        (
            "fame_changed",
            EngineEvent::FameChanged {
                at: snapshot.at,
                character_id: "demo-character".into(),
                fame: 1251,
            },
        ),
        (
            "alert_fired",
            EngineEvent::AlertFired(AlertView {
                details: None,
                presentation: None,
                id: "demo-alert".into(),
                at: snapshot.at,
                kind: AlertKind::WhiteBag,
                run_id: None,
                explanation: "Synthetic white bag observation".into(),
            }),
        ),
    ];
    for (tag, event) in events {
        let json = round_trip(&EventEnvelope {
            fight_chart_omitted: false,
            stream_id: snapshot.stream_id.clone(),
            sequence: Sequence(Timestamp(u64::MAX)),
            at: Timestamp(u64::MAX),
            event,
        });
        assert_eq!(json["event"].as_object().unwrap().len(), 1);
        assert!(json["event"].get(tag).is_some(), "{tag}");
        assert_eq!(json["sequence"], u64::MAX.to_string());
        assert_eq!(json["at"], u64::MAX.to_string());
        assert!(EngineEvent::decl().contains(&format!("\"{tag}\"")));
    }
}

#[test]
fn catalog_round_trip_includes_every_definition_and_unknown_asset_fields() {
    let mut catalog = demo_catalog();
    catalog.items[0].scaling = Some(ScalingDef {
        stat_id: 20,
        minimum: 50,
        damage_per_stat: 0.25,
        shots: 2,
        origin_item: None,
    });
    catalog.enchants.push(EnchantDef {
        regeneration: None,
        id: 987654,
        name: "Synthetic enchant".into(),
        description: "Contract fixture".into(),
        stat_damage_multiplier: Some(1.25),
        weapon_damage: None,
    });
    catalog.sprites.push(SpriteRef {
        key: "synthetic".into(),
        sheet: "synthetic.png".into(),
        x: 0,
        y: 0,
        width: 8,
        height: 8,
        trim: None,
    });
    catalog.tiles.push(TileDef {
        tile_type: 1,
        min_damage: 10,
        max_damage: 20,
    });
    let json = round_trip(&catalog);
    assert_eq!(json["format_version"], 1);
    assert_eq!(json["items"][0]["sprite_key"], Value::Null);
    assert_eq!(json["items"][0]["scaling"]["origin_item"], Value::Null);
    assert_eq!(json["dungeons"][0]["completion_index"], Value::Null);
    assert_eq!(json["bags"][0]["rank"], Value::Null);
    assert_eq!(json["crucible_bonuses"], json!([]));
    catalog.crucible_bonuses.push(CrucibleDef {
        id: "synthetic-bonus".into(),
        damage_multiplier: 1.25,
    });
    round_trip(&catalog);
}

#[test]
fn queries_preserve_unknown_metrics_ranges_cursors_and_durable_watermark() {
    let story = demo_run_story();
    assert_eq!(story.fights[0].total_damage, 300.0);
    assert_eq!(story.fights[0].local_dps, None);
    assert_eq!(
        demo_home_summary().top_fight.unwrap().players[0].dps,
        Some(150.0)
    );
    assert_eq!(story.drops.len(), 1);
    assert_eq!(
        story
            .timeline
            .iter()
            .filter(|entry| entry.kind == TimelineKind::Completion)
            .count(),
        1
    );
    assert_eq!(round_trip(&story)["comparison"], Value::Null);
    let home = round_trip(&demo_home_summary());
    assert_eq!(home["fame_delta"], Value::Null);
    assert_eq!(home["best_drop"], Value::Null);
    assert_eq!(round_trip(&demo_run_page())["next"], Value::Null);
    round_trip(&ClearComparison {
        previous_clears: 2,
        mean_micros: 6_000_000.0,
        change_fraction: -0.25,
    });
    let range = TimeRange {
        start: Timestamp(0),
        end: Timestamp(u64::MAX),
    };
    assert_eq!(
        round_trip(&HomeScope::Today {
            range: range.clone()
        }),
        json!({"today": {"range": {"start": "0", "end": u64::MAX.to_string()}}})
    );
    assert_eq!(
        round_trip(&HomeScope::Session {
            id: "demo-session".into()
        }),
        json!({"session": {"id": "demo-session"}})
    );
    round_trip(&RunFilter {
        character_id: None,
        dungeon: None,
        outcome: Some(Outcome::Unknown),
        range: Some(range),
        sort: RunSort::DurationDescending,
        cursor: Some(RunCursor {
            sort_value: u64::MAX.to_string(),
            id: "demo-run".into(),
        }),
        limit: 50,
    });
    let mut status = StoreStatus {
        state: StoreState::Backpressured,
        queued: 2,
        durable: None,
        stream_id: None,
        message: "Synthetic unsaved state".into(),
    };
    assert_eq!(round_trip(&status)["durable"], Value::Null);
    status.durable = Some(Sequence(Timestamp(u64::MAX)));
    status.stream_id = Some("demo-stream".into());
    assert_eq!(round_trip(&status)["durable"], u64::MAX.to_string());
}

#[test]
fn unit_enum_discriminants_agree_with_generated_typescript() {
    macro_rules! check {
        ($ty:ty; $($variant:expr => $tag:literal),+ $(,)?) => {
            $(assert_eq!(round_trip(&$variant), $tag);
            assert!(<$ty>::decl().contains(concat!("\"", $tag, "\"")));)+
        };
    }
    check!(Outcome; Outcome::Cleared => "cleared", Outcome::Died => "died", Outcome::Left => "left", Outcome::Unknown => "unknown");
    check!(Evidence; Evidence::Victory => "victory", Evidence::DeathPacket => "death_packet", Evidence::ObservedExit => "observed_exit", Evidence::CaptureGap => "capture_gap", Evidence::AppClosed => "app_closed");
    check!(CoverageIssue; CoverageIssue::Midstream => "midstream", CoverageIssue::DecodeFailure => "decode_failure", CoverageIssue::StreamGap => "stream_gap", CoverageIssue::MissingIdentity => "missing_identity", CoverageIssue::MissingCatalog => "missing_catalog", CoverageIssue::MissingStats => "missing_stats", CoverageIssue::MissingProjectile => "missing_projectile", CoverageIssue::AmbiguousHit => "ambiguous_hit", CoverageIssue::ResourceLimit => "resource_limit", CoverageIssue::ClockRegression => "clock_regression");
    check!(AreaKind; AreaKind::Nexus => "nexus", AreaKind::Realm => "realm", AreaKind::Dungeon => "dungeon", AreaKind::OtherHub => "other_hub", AreaKind::Unknown => "unknown");
    check!(SourceKind; SourceKind::Weapon => "weapon", SourceKind::Ability => "ability", SourceKind::Summon => "summon", SourceKind::ItemEffect => "item_effect", SourceKind::Other => "other", SourceKind::Unknown => "unknown");
    check!(EnchantState; EnchantState::Missing => "missing", EnchantState::Invalid => "invalid", EnchantState::RecordedEmpty => "recorded_empty", EnchantState::Recorded => "recorded");
    check!(FightEnd; FightEnd::MapChanged => "map_changed", FightEnd::CaptureStopped => "capture_stopped", FightEnd::SessionIdle => "session_idle", FightEnd::AppClosed => "app_closed", FightEnd::ResourceLimit => "resource_limit");
    check!(AlertKind; AlertKind::WhiteBag => "white_bag", AlertKind::PersonalBest => "personal_best", AlertKind::KeyPopped => "key_popped", AlertKind::FlawlessRun => "flawless_run", AlertKind::Death => "death");
    check!(RunSort; RunSort::Newest => "newest", RunSort::Oldest => "oldest", RunSort::DurationAscending => "duration_ascending", RunSort::DurationDescending => "duration_descending");
    check!(TimelineKind; TimelineKind::Entered => "entered", TimelineKind::Fight => "fight", TimelineKind::Drop => "drop", TimelineKind::Completion => "completion", TimelineKind::Death => "death", TimelineKind::Exit => "exit", TimelineKind::CaptureIssue => "capture_issue");
    check!(StoreState; StoreState::Ready => "ready", StoreState::Backpressured => "backpressured", StoreState::Failed => "failed", StoreState::Closing => "closing", StoreState::Closed => "closed");
    assert_eq!(Timestamp::decl(), "type Timestamp = string;");
    assert_eq!(Sequence::decl(), "type Sequence = Timestamp;");
    assert!(PlayerDamage::decl().contains("dps: number | null"));
    assert!(CharacterView::decl().contains("maxed: number | null"));
}

#[test]
fn nonfinite_numbers_are_rejected_instead_of_becoming_null() {
    macro_rules! reject {
        ($base:expr, $field:ident, $bad:expr) => {{
            let mut value = $base;
            value.$field = $bad;
            assert!(serde_json::to_string(&value).is_err(), stringify!($field));
        }};
    }
    let fight = demo_snapshot().fights.remove(0);
    let player = &fight.players[0];
    let source = &player.sources[0];
    let bucket = &fight.buckets[0];
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        reject!(fight.clone(), total_damage, bad);
        reject!(fight.clone(), unattributed_damage, bad);
        reject!(player.clone(), damage, bad);
        reject!(player.clone(), biggest_hit, Some(bad));
        reject!(player.clone(), dps, Some(bad));
        reject!(player.clone(), share, Some(bad));
        reject!(source.clone(), damage, bad);
        reject!(source.clone(), biggest_hit, Some(bad));
        reject!(bucket.clone(), total, bad);
        reject!(bucket.clone(), local, Some(bad));
        let comparison = ClearComparison {
            previous_clears: 1,
            mean_micros: 1.0,
            change_fraction: 0.0,
        };
        reject!(comparison.clone(), mean_micros, bad);
        reject!(comparison, change_fraction, bad);
        assert!(serde_json::to_string(&CrucibleDef {
            id: "synthetic".into(),
            damage_multiplier: bad
        })
        .is_err());
    }
    let scaling = ScalingDef {
        stat_id: 20,
        minimum: 0,
        damage_per_stat: 0.5,
        shots: 1,
        origin_item: None,
    };
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        reject!(scaling.clone(), damage_per_stat, bad);
    }
    // Valid JSON f64 values may overflow f32 during deserialization.
    let mut json = serde_json::to_value(scaling).unwrap();
    json["damage_per_stat"] = json!(1e39);
    assert!(serde_json::from_value::<ScalingDef>(json).is_err());
}

#[test]
fn exported_fixture_is_the_typed_synthetic_fixture() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/p1-demo.json")).unwrap();
    assert_eq!(
        fixture,
        json!({
            "snapshot": demo_snapshot(),
            "catalog": demo_catalog(),
            "home_summary": demo_home_summary(),
            "run_page": demo_run_page(),
            "run_story": demo_run_story(),
        })
    );
}

#[test]
fn retention_and_session_coverage_are_additive_contracts() {
    let snapshot = demo_snapshot();
    let mut session = serde_json::to_value(snapshot.session.unwrap()).unwrap();
    session.as_object_mut().unwrap().remove("issues");
    assert!(serde_json::from_value::<PlaySessionView>(session)
        .unwrap()
        .issues
        .is_empty());
    let mut item = serde_json::to_value(&snapshot.drops[0].contents[0]).unwrap();
    item.as_object_mut().unwrap().remove("taken_observed_at");
    let mut item: DropItem = serde_json::from_value(item).unwrap();
    assert_eq!(item.taken_observed_at, None);
    item.taken_observed_at = Some(Timestamp(9_007_199_254_740_993));
    assert_eq!(round_trip(&item)["taken_observed_at"], "9007199254740993");
}

#[test]
fn legacy_fight_payload_has_no_live_quest_or_local_recency_claim() {
    let mut value = serde_json::to_value(demo_snapshot().fights.remove(0)).unwrap();
    value.as_object_mut().unwrap().remove("is_current_quest");
    value.as_object_mut().unwrap().remove("local_last_hit");
    value
        .as_object_mut()
        .unwrap()
        .remove("pending_local_reconciliation");
    let fight: FightView = serde_json::from_value(value).unwrap();
    assert!(!fight.is_current_quest);
    assert!(fight.local_last_hit.is_none());
    assert!(!fight.pending_local_reconciliation);
}

#[test]
fn fight_dps_basis_is_explicit_and_legacy_json_stays_legacy() {
    let mut value = serde_json::to_value(demo_snapshot().fights.remove(0)).unwrap();
    assert_eq!(value["dps_basis"], "player_active_window_v1");
    value.as_object_mut().unwrap().remove("dps_basis");
    let legacy: FightView = serde_json::from_value(value).unwrap();
    assert_eq!(round_trip(&legacy)["dps_basis"], "whole_target_v0");
}

#[test]
fn player_drilldown_contracts_round_trip_populated_values() {
    let snapshot = demo_snapshot();
    let character = snapshot.character.unwrap();
    let fight = &snapshot.fights[0];
    let profile = PlayerProfileView {
        maxed_gated: true,
        scope_id: "synthetic-scope".into(),
        run_id: Some("demo-run".into()),
        player_id: "synthetic-player".into(),
        object_id: 1,
        is_local: true,
        name: Some("Synthetic Knight".into()),
        class_id: Some(100),
        level: Some(20),
        stars: Some(5),
        fame: Some(1250),
        skin_id: Some(101),
        guild: Some("Synthetic Guild".into()),
        stats: character.stats,
        maxed: Some(8),
        maxed_stats: vec![0, 3, 20, 21, 22, 28, 26, 27],
        equipment: character.equipment,
        first_seen: Timestamp(9_007_199_254_740_993),
        last_seen: Timestamp(u64::MAX),
    };
    assert_eq!(round_trip(&profile)["maxed_gated"], true);
    let mut old = round_trip(&profile);
    old.as_object_mut().unwrap().remove("maxed_gated");
    assert!(
        !serde_json::from_value::<PlayerProfileView>(old)
            .unwrap()
            .maxed_gated
    );
    assert_eq!(round_trip(&profile)["first_seen"], "9007199254740993");
    assert_eq!(
        round_trip(&profile)["maxed_stats"],
        json!([0, 3, 20, 21, 22, 28, 26, 27])
    );
    assert!(PlayerProfileView::decl().contains("maxed_stats: Array<number>"));
    let mut legacy = serde_json::to_value(&profile).unwrap();
    legacy.as_object_mut().unwrap().remove("maxed_stats");
    assert!(serde_json::from_value::<PlayerProfileView>(legacy)
        .unwrap()
        .maxed_stats
        .is_empty());
    let bucket = ScopeBucket {
        from: Timestamp(1),
        until: Timestamp(2),
        party_total: 300.5,
        party_active: 2,
    };
    round_trip(&bucket);
    let update = ScopeTimelineUpdate {
        scope_id: profile.scope_id.clone(),
        run_id: profile.run_id.clone(),
        buckets: vec![bucket.clone()],
        players: vec![PlayerSeries {
            player_id: profile.player_id.clone(),
            object_id: 1,
            is_local: true,
            points: vec![300.5],
        }],
        full: true,
        compacted: true,
        disabled: false,
    };
    round_trip(&update);
    assert_eq!(
        round_trip(&EngineEvent::PlayerObserved(profile.clone())),
        json!({"player_observed": profile})
    );
    assert_eq!(
        round_trip(&EngineEvent::ScopeTimelineUpdated(update.clone())),
        json!({"scope_timeline_updated": update})
    );
    let gear = GearChange {
        at: Timestamp(2),
        equipment: profile.equipment.clone(),
    };
    round_trip(&gear);
    let timeline = PlayerTimeline {
        buckets: vec![bucket],
        points: vec![300.5],
        compacted: true,
        disabled: false,
    };
    round_trip(&timeline);
    let death = PlayerDeathAt {
        at: Timestamp(3),
        fight_id: Some(fight.id.clone()),
    };
    round_trip(&death);
    let roster = RosterEntry {
        player_id: profile.player_id.clone(),
        name: profile.name.clone(),
        class_id: profile.class_id,
        is_local: true,
        damage: Some(300.5),
    };
    round_trip(&roster);
    let party = PartySummary {
        total_damage: 300.5,
        average_dps: Some(150.25),
        top_player_id: Some(profile.player_id.clone()),
        top_name: profile.name.clone(),
        top_dps: Some(150.25),
        players_with_damage: 1,
    };
    round_trip(&party);
    let player_fight = RunPlayerFight {
        fight_id: fight.id.clone(),
        dps_basis: fight.dps_basis,
        object_id: Some(2),
        target_type: Some(300),
        target_name: Some("Synthetic Boss".into()),
        boss_type: Some(300),
        started: Timestamp(1),
        ended: Some(Timestamp(3)),
        killed: true,
        player_count: 1,
        damage: Some(300.5),
        dps: Some(150.25),
        rank: Some(1),
        share: Some(1.0),
        deaths: Some(1),
    };
    round_trip(&player_fight);
    let view = RunPlayerView {
        run: snapshot.run.unwrap(),
        scope_id: profile.scope_id.clone(),
        player_id: profile.player_id.clone(),
        player: Some(RunPlayerTotal {
            player_id: profile.player_id.clone(),
            name: profile.name.clone(),
            class_id: profile.class_id,
            is_local: true,
            damage: 300.5,
            hits: 2,
            deaths: 1,
            share: Some(1.0),
            rank: Some(1),
            dps: Some(150.25),
        }),
        party,
        sources: fight.players[0].sources.clone(),
        fights: vec![player_fight],
        profile: Some(profile.clone()),
        gear_log: vec![gear.clone()],
        timeline: Some(timeline.clone()),
        deaths: vec![death.clone()],
        departed_at: Some(Timestamp(4)),
        roster: vec![roster],
        partial: true,
    };
    round_trip(&view);
    round_trip(&PlayerContextView {
        profile: Some(profile),
        gear_log: vec![gear],
        timeline: Some(timeline),
        deaths: vec![death],
    });
}

#[test]
fn player_drilldown_legacy_fields_default_to_unknown() {
    let mut value = serde_json::to_value(demo_snapshot().fights.remove(0)).unwrap();
    value.as_object_mut().unwrap().remove("scope_id");
    assert_eq!(
        serde_json::from_value::<FightView>(value).unwrap().scope_id,
        None
    );
    let death: InstanceEventKind = serde_json::from_value(
        json!({"type": "player_death", "player_name": "Synthetic Knight", "is_local": false}),
    )
    .unwrap();
    let InstanceEventKind::PlayerDeath { player_id, .. } = death else {
        panic!("expected death")
    };
    assert_eq!(player_id, None);
}

#[test]
fn item_detail_page_decl() {
    assert!(ItemDetail::decl().contains("slot_label: string | null"));
    assert!(EnchantInfo::decl().contains("description: string | null"));
}

#[test]
fn party_member_provenance_defaults_and_round_trips() {
    let mut value = json!({
        "player_id": "synthetic", "equipment": [], "is_local": true,
        "flags": [], "runs_together": 0
    });
    let legacy: PartyMember = serde_json::from_value(value.clone()).unwrap();
    assert!(!legacy.maxed_gated);
    value["maxed_gated"] = json!(true);
    let marked: PartyMember = serde_json::from_value(value).unwrap();
    assert!(marked.maxed_gated);
    assert_eq!(round_trip(&marked)["maxed_gated"], true);
    assert!(PartyMember::decl().contains("maxed_gated?: boolean"));
    assert!(PlayerProfileView::decl().contains("maxed_gated?: boolean"));
}

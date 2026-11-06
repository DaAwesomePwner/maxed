use maxed_types::*;

fn message(channel: ChatChannel, sender: &str, text: &str) -> ChatMessageView {
    ChatMessageView {
        id: "live-1:chat:1".into(),
        session_id: "s".into(),
        run_id: None,
        map: Some("Nexus".into()),
        at: Timestamp(1_700_000_000_000_000),
        channel,
        direction: None,
        sender_kind: ChatSenderKind::Player,
        sender: sender.into(),
        counterpart: None,
        is_local: false,
        stars: Some(70),
        supporter: false,
        text: text.into(),
        truncated: false,
    }
}

#[test]
fn chat_event_serializes_with_snake_case_tags() {
    let event = EngineEvent::Chat(message(ChatChannel::Guild, "SynthAlpha", "hi"));
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["chat"]["channel"], "guild");
    assert_eq!(json["chat"]["sender_kind"], "player");
    let back: EngineEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn chat_settings_default_and_old_files() {
    let settings = ChatSettings::default();
    assert!(
        settings.store.public
            && settings.store.guild
            && settings.store.party
            && settings.store.whisper
    );
    assert_eq!(settings.retention_days, 90);
    assert!(settings.filter_spam);
    let disabled: ChatSettings = serde_json::from_str(r#"{"filter_spam":false}"#).unwrap();
    assert!(!disabled.filter_spam);
    assert_eq!(
        serde_json::to_value(&disabled).unwrap()["filter_spam"],
        false
    );
    let parsed: ChatSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed, settings);
    assert!(serde_json::from_str::<ChatSettings>(r#"{"unknown":1}"#).is_err());
}

#[test]
fn chat_settings_validation_bounds() {
    let mut s = ChatSettings::default();
    assert!(s.valid());
    s.retention_days = 0;
    assert!(!s.valid());
    s.retention_days = 3650;
    assert!(s.valid());
    s.retention_days = 3651;
    assert!(!s.valid());
    s.retention_days = 90;
    s.hidden_senders = vec!["x".repeat(33)];
    assert!(!s.valid());
    s.hidden_senders = vec![String::new()];
    assert!(!s.valid());
    s.hidden_senders = vec!["SynthBravo".into(); 257];
    assert!(!s.valid());
    s.hidden_senders = vec![];
    s.hidden_keywords = vec!["k".repeat(65)];
    assert!(!s.valid());
    s.hidden_keywords = vec!["a\u{0}b".into()];
    assert!(!s.valid());
}

#[test]
fn storage_toggles_cover_unknown_channel() {
    let mut store = ChatStorage::default();
    assert!(store.stores(ChatChannel::Unknown));
    store = ChatStorage {
        public: false,
        guild: false,
        party: false,
        whisper: false,
    };
    assert!(!store.stores(ChatChannel::Unknown));
    store.party = true;
    assert!(store.stores(ChatChannel::Unknown));
    assert!(!store.stores(ChatChannel::Guild));
}

#[test]
fn hide_lists_are_ascii_case_insensitive() {
    let s = ChatSettings {
        hidden_senders: vec!["synthalpha".into()],
        hidden_keywords: vec!["CHEAP GOLD".into()],
        ..ChatSettings::default()
    };
    assert!(s.hides(&message(ChatChannel::Public, "SynthAlpha", "hello")));
    assert!(s.hides(&message(
        ChatChannel::Public,
        "SynthBravo",
        "buy cheap gold now"
    )));
    assert!(!s.hides(&message(ChatChannel::Public, "SynthBravo", "hello")));
}

#[test]
fn event_tags_match_store_kind_strings() {
    assert_eq!(InstanceEventTag::PlayerDeath.tag(), "player_death");
    assert_eq!(InstanceEventTag::PlayerLeft.tag(), "player_left");
    assert_eq!(InstanceEventTag::PortalOpened.tag(), "portal_opened");
    assert_eq!(
        InstanceEventTag::RealmEventKilled.tag(),
        "realm_event_killed"
    );
    assert_eq!(ChatChannel::Whisper.tag(), "whisper");
}

#[test]
fn chat_alert_preferences_default_for_old_settings() {
    let old = r#"{"white_bag":"banner","personal_best":"banner","flawless_run":"off","muted":false,"windows_notifications":false,"volume_percent":70}"#;
    let parsed: AlertPreferences = serde_json::from_str(old).unwrap();
    assert_eq!(parsed.whisper, AlertLoudness::Toast);
    assert_eq!(parsed.guild_message, AlertLoudness::QuietLogOnly);
    assert_eq!(parsed.party_message, AlertLoudness::QuietLogOnly);
    assert_eq!(parsed.loudness(&AlertKind::Whisper), AlertLoudness::Toast);
    let disabled = AlertPreferences::disabled();
    assert_eq!(disabled.loudness(&AlertKind::Whisper), AlertLoudness::Off);
    assert_eq!(
        disabled.loudness(&AlertKind::GuildMessage),
        AlertLoudness::Off
    );
}

#[test]
fn app_settings_without_chat_field_still_parse() {
    let old = r#"{"version":1,"adapter_id":null,"assets_path":null,"save_recordings":false,"recording_limit_bytes":1000000000,"update_channel":"stable"}"#;
    let parsed: AppSettings = serde_json::from_str(old).unwrap();
    assert_eq!(parsed.chat, ChatSettings::default());
}

#[test]
fn chat_alert_details_round_trip() {
    let details = AlertDetails::Chat {
        channel: ChatChannel::Whisper,
        sender: "SynthAlpha".into(),
        excerpt: "need a carry?".into(),
    };
    let json = serde_json::to_value(&details).unwrap();
    assert_eq!(json["type"], "chat");
    assert_eq!(
        serde_json::from_value::<AlertDetails>(json).unwrap(),
        details
    );
}

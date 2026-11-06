use crate::common;
use common::chat::*;
use maxed_store::HiddenLists;
use maxed_types::*;

fn filter() -> ChatFilter {
    ChatFilter {
        channel: None,
        sender: None,
        query: String::new(),
        run_id: None,
        show_npc: false,
        show_hidden: false,
        cursor: None,
        limit: 2,
    }
}

#[test]
fn overview_counts_stored_lines_including_npcs() {
    let (_dir, reader) = seeded();
    let overview = reader
        .chat_overview(ChatOverviewRequest { run_id: None })
        .unwrap();
    assert_eq!(
        overview,
        ChatOverview {
            lines: 4,
            by_channel: ChannelCounts {
                public: 2,
                guild: 1,
                party: 0,
                whisper: 1,
                unknown: 0
            },
            first_at: Some(Timestamp(1_000_000)),
            latest_at: Some(Timestamp(4_000_000)),
        }
    );
}

#[test]
fn summary_requests_decode_legacy_defaults() {
    let overview: ChatOverviewRequest = serde_json::from_str("{}").unwrap();
    assert_eq!(overview.run_id, None);
    let partners: WhisperPartnerFilter = serde_json::from_str(r#"{"limit":200}"#).unwrap();
    assert!(partners.query.is_empty());
    assert!(!partners.show_hidden);
}

#[test]
fn whisper_partner_bounds_and_hidden_names() {
    let (_dir, reader) = seeded();
    let hidden = HiddenLists {
        senders: vec!["synthcharlie".into()],
        keywords: vec![],
    };
    let filter = WhisperPartnerFilter {
        query: String::new(),
        show_hidden: false,
        limit: 200,
    };
    let empty = reader.whisper_partners(filter.clone(), &hidden).unwrap();
    assert_eq!(empty, WhisperPartners::default());
    let visible = reader
        .whisper_partners(
            WhisperPartnerFilter {
                show_hidden: true,
                ..filter.clone()
            },
            &hidden,
        )
        .unwrap();
    assert_eq!(visible.people, 1);
    assert_eq!(visible.lines, 1);
    assert_eq!(visible.rows[0].name, "SynthCharlie");
    for limit in [0, 201] {
        assert!(reader
            .whisper_partners(
                WhisperPartnerFilter {
                    limit,
                    ..filter.clone()
                },
                &hidden
            )
            .is_err());
    }
}

#[test]
fn overview_run_scope_and_empty_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut events = common::story_events();
    for event in &mut events {
        event.stream_id = "synthetic-run-seed".into();
    }
    maxed_store::apply_batch(&mut db, &events).unwrap();
    let run = maxed_types::demo::demo_run_story().run.id;
    let mut scoped = message(1, ChatChannel::Party, "Vexlin", "scoped", 1);
    scoped.run_id = Some(run.clone());
    apply_chat(
        &mut db,
        &[
            (scoped, None),
            (
                message(2, ChatChannel::Unknown, "Quenby", "outside", 2),
                None,
            ),
        ],
    );
    let reader = maxed_store::open_reader(&path).unwrap();
    let scope = reader
        .chat_overview(ChatOverviewRequest { run_id: Some(run) })
        .unwrap();
    assert_eq!(scope.lines, 1);
    assert_eq!(scope.by_channel.party, 1);
    assert_eq!(scope.first_at, Some(Timestamp(1)));
    assert_eq!(scope.latest_at, Some(Timestamp(1)));
    assert_eq!(
        reader
            .chat_overview(ChatOverviewRequest { run_id: None })
            .unwrap()
            .by_channel
            .unknown,
        1
    );
    assert_eq!(
        reader
            .chat_overview(ChatOverviewRequest {
                run_id: Some("missing".into())
            })
            .unwrap(),
        ChatOverview::default()
    );
    for run_id in ["".into(), "x".repeat(257), "bad\nrun".into()] {
        assert!(reader
            .chat_overview(ChatOverviewRequest {
                run_id: Some(run_id)
            })
            .is_err());
    }
}

#[test]
fn sender_summary_uses_newest_metadata_and_counts_both_whisper_directions_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let first = message(1, ChatChannel::Guild, "Vexlin", "first", 1);
    let incoming = message(2, ChatChannel::Whisper, "Vexlin", "incoming", 2);
    let mut outgoing = message(3, ChatChannel::Whisper, "Quenby", "outgoing", 3);
    outgoing.direction = Some(WhisperDirection::Outgoing);
    outgoing.counterpart = Some("vExLiN".into());
    outgoing.is_local = true;
    let mut newest = message(4, ChatChannel::Public, "VEXLIN", "latest", 4);
    newest.stars = Some(72);
    newest.supporter = true;
    apply_chat(
        &mut db,
        &[
            (first, None),
            (incoming, None),
            (outgoing, None),
            (newest, None),
        ],
    );
    let reader = maxed_store::open_reader(&path).unwrap();
    let summary = reader
        .chat_sender(ChatSenderRequest {
            name: "vexlin".into(),
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        summary,
        ChatSenderSummary {
            name: "VEXLIN".into(),
            lines: 3,
            first_at: Timestamp(1),
            last_at: Timestamp(4),
            channels: vec![
                ChatChannel::Public,
                ChatChannel::Guild,
                ChatChannel::Whisper
            ],
            stars: Some(72),
            supporter: true,
            whispers: 2,
        }
    );
    assert!(reader
        .chat_sender(ChatSenderRequest {
            name: "Kestrel".into()
        })
        .unwrap()
        .is_none());
    for name in [
        String::new(),
        "bad\nname".into(),
        "é".repeat(17),
        "x".repeat(33),
    ] {
        assert!(matches!(
            reader.chat_sender(ChatSenderRequest { name }),
            Err(maxed_store::StoreError::Limit)
        ));
    }
}

#[test]
fn whisper_partner_naming_search_order_and_multibyte_excerpt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut messages = vec![];
    for (i, direction, local, sender, counterpart) in [
        (
            1,
            Some(WhisperDirection::Incoming),
            false,
            "Vexlin",
            Some("Quenby"),
        ),
        (
            2,
            Some(WhisperDirection::Outgoing),
            true,
            "Quenby",
            Some("VEXLIN"),
        ),
        (
            3,
            Some(WhisperDirection::Unknown),
            false,
            "Morrow",
            Some("Quenby"),
        ),
        (4, None, true, "Quenby", Some("Kestrel")),
        (5, Some(WhisperDirection::Outgoing), true, "Quenby", None),
        (6, None, false, "", None),
    ] {
        let mut m = message(i, ChatChannel::Whisper, sender, &"界".repeat(200), i);
        m.direction = direction;
        m.is_local = local;
        m.counterpart = counterpart.map(str::to_owned);
        messages.push((m, None));
    }
    apply_chat(&mut db, &messages);
    let reader = maxed_store::open_reader(&path).unwrap();
    let filter = WhisperPartnerFilter {
        query: String::new(),
        show_hidden: false,
        limit: 200,
    };
    let partners = reader
        .whisper_partners(filter.clone(), &HiddenLists::default())
        .unwrap();
    assert_eq!((partners.people, partners.lines), (3, 4));
    assert_eq!(
        partners
            .rows
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        ["Kestrel", "Morrow", "VEXLIN"]
    );
    assert_eq!(partners.rows[0].last_direction, WhisperDirection::Unknown);
    assert_eq!(partners.rows[2].last_direction, WhisperDirection::Outgoing);
    assert_eq!(partners.rows[2].lines, 2);
    assert_eq!(partners.rows[0].last_text, "界".repeat(120));
    let limited = reader
        .whisper_partners(
            WhisperPartnerFilter {
                limit: 1,
                ..filter.clone()
            },
            &HiddenLists::default(),
        )
        .unwrap();
    assert_eq!(limited.rows.len(), 1);
    assert_eq!((limited.people, limited.lines), (3, 4));
    for (query, expected) in [("vex", 1), ("界", 3), ("Quenby", 0), ("no_match", 0)] {
        assert_eq!(
            reader
                .whisper_partners(
                    WhisperPartnerFilter {
                        query: query.into(),
                        ..filter.clone()
                    },
                    &HiddenLists::default()
                )
                .unwrap()
                .people,
            expected
        );
    }
    for query in ["x".repeat(129), "界".repeat(171), "bad\0query".into()] {
        assert!(reader
            .whisper_partners(
                WhisperPartnerFilter {
                    query,
                    ..filter.clone()
                },
                &HiddenLists::default()
            )
            .is_err());
    }
}

#[test]
fn whisper_partner_response_and_decode_obey_byte_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let messages: Vec<_> = (0..200)
        .map(|i| {
            (
                message(
                    i + 1,
                    ChatChannel::Whisper,
                    &format!("Vexlin{i}"),
                    &"🦀".repeat(256),
                    i,
                ),
                None,
            )
        })
        .collect();
    apply_chat(&mut db, &messages);
    let reader = maxed_store::open_reader(&path).unwrap();
    let filter = WhisperPartnerFilter {
        query: String::new(),
        show_hidden: false,
        limit: 200,
    };
    let partners = reader
        .whisper_partners(filter.clone(), &HiddenLists::default())
        .unwrap();
    assert_eq!(partners.rows.len(), 200);
    assert_eq!(partners.lines, 200);
    assert!(partners
        .rows
        .iter()
        .all(|r| r.last_text.chars().count() == 120));
    assert!(serde_json::to_vec(&partners).unwrap().len() <= 1024 * 1024);
    // Simulate oversized persisted JSON: reject it before decoding.
    db.execute(
        "UPDATE chat_messages SET json=json_set(json,'$.text',?1) WHERE id=?2",
        rusqlite::params!["x".repeat(1024 * 1024), messages[199].0.id],
    )
    .unwrap();
    assert!(matches!(
        reader.whisper_partners(filter, &HiddenLists::default()),
        Err(maxed_store::StoreError::Limit)
    ));
    assert!(matches!(
        reader.chat_sender(ChatSenderRequest {
            name: "Vexlin199".into()
        }),
        Err(maxed_store::StoreError::Limit)
    ));
}

fn seeded() -> (tempfile::TempDir, maxed_store::StoreReader) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut npc = message(4, ChatChannel::Public, "SynthDelta", "I am back", 4_000_000);
    npc.sender_kind = ChatSenderKind::Npc;
    apply_chat(
        &mut db,
        &[
            (
                message(
                    1,
                    ChatChannel::Public,
                    "SynthAlpha",
                    "selling cheap gold",
                    1_000_000,
                ),
                None,
            ),
            (
                message(
                    2,
                    ChatChannel::Guild,
                    "SynthBravo",
                    "who wants lost halls",
                    2_000_000,
                ),
                None,
            ),
            (
                message(
                    3,
                    ChatChannel::Whisper,
                    "SynthCharlie",
                    "hey there",
                    3_000_000,
                ),
                None,
            ),
            (npc, None),
        ],
    );
    drop(db);
    let reader = maxed_store::open_reader(&path).unwrap();
    (dir, reader)
}

fn texts(page: &ChatPage) -> Vec<&str> {
    page.rows.iter().map(|r| r.message.text.as_str()).collect()
}

#[test]
fn pages_newest_first_and_hides_npc_by_default() {
    let (_dir, reader) = seeded();
    let page = reader.list_chat(filter(), &HiddenLists::default()).unwrap();
    assert_eq!(texts(&page), ["hey there", "who wants lost halls"]);
    let next = reader
        .list_chat(
            ChatFilter {
                cursor: page.next.clone(),
                ..filter()
            },
            &HiddenLists::default(),
        )
        .unwrap();
    assert_eq!(texts(&next), ["selling cheap gold"]);
    assert!(next.next.is_none());
    let npc = reader
        .list_chat(
            ChatFilter {
                show_npc: true,
                limit: 10,
                ..filter()
            },
            &HiddenLists::default(),
        )
        .unwrap();
    assert_eq!(npc.rows.len(), 4);
}

#[test]
fn channel_sender_and_search_filters() {
    let (_dir, reader) = seeded();
    let none = HiddenLists::default();
    let guild = reader
        .list_chat(
            ChatFilter {
                channel: Some(ChatChannel::Guild),
                ..filter()
            },
            &none,
        )
        .unwrap();
    assert_eq!(texts(&guild), ["who wants lost halls"]);
    let sender = reader
        .list_chat(
            ChatFilter {
                sender: Some("synthcharlie".into()),
                ..filter()
            },
            &none,
        )
        .unwrap();
    assert_eq!(texts(&sender), ["hey there"]);
    let search = reader
        .list_chat(
            ChatFilter {
                query: "los".into(),
                ..filter()
            },
            &none,
        )
        .unwrap();
    assert_eq!(texts(&search), ["who wants lost halls"]);
}

#[test]
fn search_input_with_fts_syntax_never_errors() {
    let (_dir, reader) = seeded();
    for query in [
        "\"",
        "*",
        "NEAR(",
        "-gold",
        "gold\" OR \"x",
        "((",
        "😀",
        "   ",
        "AND",
    ] {
        let page = reader.list_chat(
            ChatFilter {
                query: query.into(),
                limit: 10,
                ..filter()
            },
            &HiddenLists::default(),
        );
        assert!(page.is_ok(), "{query}");
    }
    let punctuation = reader
        .list_chat(
            ChatFilter {
                query: "\"*()".into(),
                limit: 10,
                ..filter()
            },
            &HiddenLists::default(),
        )
        .unwrap();
    assert_eq!(
        punctuation.rows.len(),
        3,
        "punctuation-only input does not filter"
    );
}

#[test]
fn hide_lists_exclude_or_mark_rows() {
    let (_dir, reader) = seeded();
    let hidden = HiddenLists {
        senders: vec!["SYNTHBRAVO".into()],
        keywords: vec!["Cheap Gold".into()],
    };
    let page = reader
        .list_chat(
            ChatFilter {
                limit: 10,
                ..filter()
            },
            &hidden,
        )
        .unwrap();
    assert_eq!(texts(&page), ["hey there"]);
    let all = reader
        .list_chat(
            ChatFilter {
                limit: 10,
                show_hidden: true,
                ..filter()
            },
            &hidden,
        )
        .unwrap();
    assert_eq!(all.rows.iter().filter(|r| r.hidden).count(), 2);
}

#[test]
fn rejects_out_of_bounds_filters() {
    let (_dir, reader) = seeded();
    let none = HiddenLists::default();
    assert!(reader
        .list_chat(
            ChatFilter {
                limit: 0,
                ..filter()
            },
            &none
        )
        .is_err());
    assert!(reader
        .list_chat(
            ChatFilter {
                limit: 101,
                ..filter()
            },
            &none
        )
        .is_err());
    assert!(reader
        .list_chat(
            ChatFilter {
                query: "x".repeat(129),
                ..filter()
            },
            &none
        )
        .is_err());
}

#[test]
fn events_reader_filters_by_kind_and_dungeon() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let session = session();
    let event = |n: u64, kind: InstanceEventKind| {
        common::envelope(
            n,
            EngineEvent::InstanceEvent(InstanceEventView {
                id: format!("test-stream:instance:1:{n}"),
                session_id: session.id.clone(),
                run_id: None,
                map: Some("Realm".into()),
                at: Timestamp(n * 1_000_000),
                kind,
                provisional: false,
            }),
        )
    };
    maxed_store::apply_batch(
        &mut db,
        &[
            common::envelope(1, EngineEvent::SessionUpdated(session.clone())),
            event(
                2,
                InstanceEventKind::PortalOpened {
                    opener_is_local: None,
                    opener: Some("SynthAlpha".into()),
                    portal_type: None,
                    dungeon: Some("Lost Halls".into()),
                },
            ),
            event(
                3,
                InstanceEventKind::PortalOpened {
                    opener_is_local: None,
                    opener: Some("SynthBravo".into()),
                    portal_type: None,
                    dungeon: Some("Shatters".into()),
                },
            ),
            event(
                4,
                InstanceEventKind::PlayerLeft {
                    object_id: 9,
                    player_name: Some("SynthCharlie".into()),
                    class_id: None,
                },
            ),
        ],
    )
    .unwrap();
    drop(db);
    let reader = maxed_store::open_reader(&path).unwrap();
    let base = EventFilter {
        range: None,
        query: String::new(),
        opener_local: None,
        kinds: vec![],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 10,
    };
    assert_eq!(reader.list_events(base.clone()).unwrap().rows.len(), 3);
    let first = reader
        .list_events(EventFilter {
            limit: 2,
            ..base.clone()
        })
        .unwrap();
    assert_eq!(
        first.rows.iter().map(|r| r.at).collect::<Vec<_>>(),
        [Timestamp(4_000_000), Timestamp(3_000_000)]
    );
    assert!(first.next.is_some());
    let last = reader
        .list_events(EventFilter {
            limit: 2,
            cursor: first.next,
            ..base.clone()
        })
        .unwrap();
    assert_eq!(last.rows.len(), 1);
    assert_eq!(last.rows[0].at, Timestamp(2_000_000));
    assert!(last.next.is_none());
    let keys = reader
        .list_events(EventFilter {
            range: None,
            query: String::new(),
            opener_local: None,
            kinds: vec![InstanceEventTag::PortalOpened],
            dungeon: Some("Lost Halls".into()),
            ..base
        })
        .unwrap();
    assert_eq!(keys.rows.len(), 1);
}

#[test]
fn chat_cursor_breaks_timestamp_ties_and_sender_matches_counterpart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut outgoing = message(3, ChatChannel::Whisper, "SynthAlpha", "outgoing", 1);
    outgoing.counterpart = Some("SynthBravo".into());
    outgoing.direction = Some(WhisperDirection::Outgoing);
    apply_chat(
        &mut db,
        &[
            (
                message(1, ChatChannel::Public, "SynthAlpha", "first", 1),
                None,
            ),
            (
                message(2, ChatChannel::Public, "SynthAlpha", "second", 1),
                None,
            ),
            (outgoing, None),
        ],
    );
    let reader = maxed_store::open_reader(&path).unwrap();
    let hidden = HiddenLists::default();
    let page = reader.list_chat(filter(), &hidden).unwrap();
    assert_eq!(texts(&page), ["outgoing", "second"]);
    let next = reader
        .list_chat(
            ChatFilter {
                cursor: page.next,
                ..filter()
            },
            &hidden,
        )
        .unwrap();
    assert_eq!(texts(&next), ["first"]);
    assert!(next.next.is_none());
    let counterpart = reader
        .list_chat(
            ChatFilter {
                sender: Some("synthbravo".into()),
                ..filter()
            },
            &hidden,
        )
        .unwrap();
    assert_eq!(texts(&counterpart), ["outgoing"]);
}

#[test]
fn validates_query_cursor_and_event_bounds() {
    let (_dir, reader) = seeded();
    let hidden = HiddenLists::default();
    for query in ["\0".into(), "😀".repeat(129)] {
        assert!(reader
            .list_chat(ChatFilter { query, ..filter() }, &hidden)
            .is_err());
    }
    assert!(reader
        .list_chat(
            ChatFilter {
                query: "😀".repeat(128),
                limit: 100,
                ..filter()
            },
            &hidden
        )
        .is_ok());
    for cursor in [
        ChatCursor {
            at: Timestamp(1),
            id: String::new(),
        },
        ChatCursor {
            at: Timestamp(u64::MAX),
            id: "valid".into(),
        },
        ChatCursor {
            at: Timestamp(1),
            id: "invalid\0".into(),
        },
    ] {
        assert!(reader
            .list_chat(
                ChatFilter {
                    cursor: Some(cursor),
                    ..filter()
                },
                &hidden
            )
            .is_err());
    }
    let base = EventFilter {
        range: None,
        query: String::new(),
        opener_local: None,
        kinds: vec![],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 100,
    };
    assert!(reader.list_events(base.clone()).unwrap().rows.is_empty());
    for limit in [0, 101] {
        assert!(reader
            .list_events(EventFilter {
                limit,
                ..base.clone()
            })
            .is_err());
    }
    for dungeon in [String::new(), "x".repeat(257)] {
        assert!(reader
            .list_events(EventFilter {
                dungeon: Some(dungeon),
                ..base.clone()
            })
            .is_err());
    }
    assert!(reader
        .list_events(EventFilter {
            range: None,
            query: String::new(),
            opener_local: None,
            kinds: vec![InstanceEventTag::PortalOpened; 5],
            ..base.clone()
        })
        .is_err());
    assert!(reader
        .list_events(EventFilter {
            run_id: Some(String::new()),
            ..base.clone()
        })
        .is_err());
    assert!(reader
        .list_events(EventFilter {
            cursor: Some(EventCursor {
                at: Timestamp(u64::MAX),
                id: "valid".into()
            }),
            ..base
        })
        .is_err());
}

#[test]
fn whisper_partners_hide_keywords_before_counting_and_selecting_latest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let visible = message(1, ChatChannel::Whisper, "Vexlin", "visible earlier line", 1);
    let mut hidden_latest = message(
        2,
        ChatChannel::Whisper,
        "Quenby",
        "some CHEAP gold offer",
        2,
    );
    hidden_latest.direction = Some(WhisperDirection::Outgoing);
    hidden_latest.is_local = true;
    hidden_latest.counterpart = Some("Vexlin".into());
    let hidden_only = message(3, ChatChannel::Whisper, "Morrow", "totally BlOcKeD", 3);
    apply_chat(
        &mut db,
        &[
            (visible.clone(), None),
            (hidden_latest.clone(), None),
            (hidden_only, None),
        ],
    );
    let reader = maxed_store::open_reader(&path).unwrap();
    let hidden = HiddenLists {
        senders: vec![],
        keywords: vec!["CHEAP GOLD".into(), "blocked".into()],
    };
    let partner_filter = WhisperPartnerFilter {
        query: String::new(),
        show_hidden: false,
        limit: 200,
    };
    let partners = reader
        .whisper_partners(partner_filter.clone(), &hidden)
        .unwrap();
    assert_eq!(
        (partners.people, partners.lines, partners.rows.len()),
        (1, 1, 1)
    );
    let partner = &partners.rows[0];
    assert_eq!(partner.name, "Vexlin");
    assert_eq!(partner.lines, 1);
    assert_eq!(partner.last_text, visible.text);
    assert_eq!(partner.last_at, visible.at);
    assert_eq!(partner.last_direction, WhisperDirection::Incoming);
    let thread = reader
        .list_chat(
            ChatFilter {
                channel: Some(ChatChannel::Whisper),
                limit: 100,
                ..filter()
            },
            &hidden,
        )
        .unwrap();
    assert_eq!(thread.rows.len(), partners.lines as usize);
    assert_eq!(thread.rows[0].message.text, partner.last_text);
    let shown = reader
        .whisper_partners(
            WhisperPartnerFilter {
                show_hidden: true,
                ..partner_filter
            },
            &hidden,
        )
        .unwrap();
    assert_eq!((shown.people, shown.lines, shown.rows.len()), (2, 3, 2));
    assert_eq!(shown.rows[0].name, "Morrow");
    let partner = shown.rows.iter().find(|row| row.name == "Vexlin").unwrap();
    assert_eq!(partner.lines, 2);
    assert_eq!(partner.last_text, hidden_latest.text);
    assert_eq!(partner.last_at, hidden_latest.at);
    assert_eq!(partner.last_direction, WhisperDirection::Outgoing);
}

#[test]
fn whisper_partner_name_query_escapes_literal_underscore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    apply_chat(
        &mut db,
        &[
            (
                message(
                    1,
                    ChatChannel::Whisper,
                    "Kest_rel",
                    "first synthetic line",
                    1,
                ),
                None,
            ),
            (
                message(
                    2,
                    ChatChannel::Whisper,
                    "Kestxrel",
                    "second synthetic line",
                    2,
                ),
                None,
            ),
        ],
    );
    let reader = maxed_store::open_reader(&path).unwrap();
    // The query has FTS terms, but neither text matches them. Only the escaped
    // partner-name LIKE can match; treating '_' as a wildcard adds Kestxrel.
    let partners = reader
        .whisper_partners(
            WhisperPartnerFilter {
                query: "Kest_rel".into(),
                show_hidden: false,
                limit: 200,
            },
            &HiddenLists::default(),
        )
        .unwrap();
    assert_eq!(
        (partners.people, partners.lines, partners.rows.len()),
        (1, 1, 1)
    );
    assert_eq!(partners.rows[0].name, "Kest_rel");
}

fn event_filter() -> EventFilter {
    EventFilter {
        range: None,
        query: String::new(),
        opener_local: None,
        kinds: vec![],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 100,
    }
}

struct EventsFixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    db: rusqlite::Connection,
}
impl EventsFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.sqlite");
        let mut db = rusqlite::Connection::open(&path).unwrap();
        maxed_store::migrate(&mut db).unwrap();
        for id in ["Vexlin-session", "Quenby-session"] {
            db.execute(
                "INSERT INTO play_sessions(id,started,last_traffic,json) VALUES (?1,0,0,'{}')",
                [id],
            )
            .unwrap();
        }
        Self {
            _dir: dir,
            path,
            db,
        }
    }
    fn event(&self, id: &str, at: u64, kind: InstanceEventKind, run: Option<&str>) {
        let tag = match &kind {
            InstanceEventKind::PortalOpened { .. } => "portal_opened",
            InstanceEventKind::PlayerLeft { .. } => "player_left",
            InstanceEventKind::PlayerDeath { .. } => "player_death",
            InstanceEventKind::RealmEventKilled { .. } => "realm_event_killed",
        };
        let event = InstanceEventView {
            id: id.into(),
            session_id: "Vexlin-session".into(),
            run_id: run.map(str::to_owned),
            map: None,
            at: Timestamp(at),
            kind,
            provisional: true,
        };
        self.db
            .execute(
                "INSERT INTO instance_events VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    id,
                    event.session_id,
                    run,
                    at as i64,
                    tag,
                    serde_json::to_string(&event).unwrap()
                ],
            )
            .unwrap();
    }
    fn portal(&self, id: &str, at: u64, dungeon: &str, local: Option<bool>) {
        self.event(
            id,
            at,
            InstanceEventKind::PortalOpened {
                opener: Some("Vexlin".into()),
                opener_is_local: local,
                dungeon: Some(dungeon.into()),
                portal_type: None,
            },
            None,
        );
    }
    fn run(&self, id: &str, at: u64, dungeon: &str, session: &str) {
        self.db
            .execute(
                "INSERT INTO runs(id,session_id,dungeon,started,outcome,complete_capture,json)
            VALUES (?1,?2,?3,?4,'cleared',1,'{}')",
                rusqlite::params![id, session, dungeon, at as i64],
            )
            .unwrap();
    }
    fn reader(&self) -> maxed_store::StoreReader {
        maxed_store::open_reader(&self.path).unwrap()
    }
}

#[test]
fn event_filters_decode_legacy_and_enforce_range_query_and_opener() {
    let legacy: EventFilter = serde_json::from_str(
        r#"{"kinds":[],"run_id":null,"dungeon":null,"cursor":null,"limit":100}"#,
    )
    .unwrap();
    assert_eq!(legacy, event_filter());
    let old: InstanceEventKind = serde_json::from_str(
        r#"{"type":"portal_opened","opener":"Vexlin","dungeon":"Vault","portal_type":null}"#,
    )
    .unwrap();
    assert!(matches!(
        old,
        InstanceEventKind::PortalOpened {
            opener_is_local: None,
            ..
        }
    ));
    let fixture = EventsFixture::new();
    fixture.portal("a", 10, "Vault", Some(true));
    fixture.portal("b", 20, "Vault", Some(false));
    fixture.portal("c", 30, "Vault", None);
    fixture.db.execute("UPDATE instance_events SET json=json_remove(json,'$.kind.opener_is_local') WHERE id='c'", []).unwrap();
    let reader = fixture.reader();
    let page = reader
        .list_events(EventFilter {
            range: Some(TimeRange {
                start: Timestamp(10),
                end: Timestamp(30),
            }),
            ..legacy.clone()
        })
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["b", "a"]
    );
    for (local, id) in [(true, "a"), (false, "b")] {
        let page = reader
            .list_events(EventFilter {
                opener_local: Some(local),
                ..legacy.clone()
            })
            .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].id, id);
        assert!(page.rows[0].provisional);
    }
    for invalid in [
        EventFilter {
            query: "x".repeat(129),
            ..legacy.clone()
        },
        EventFilter {
            query: "x\0".into(),
            ..legacy.clone()
        },
        EventFilter {
            query: "🦀".repeat(129),
            ..legacy.clone()
        },
        EventFilter {
            range: Some(TimeRange {
                start: Timestamp(20),
                end: Timestamp(20),
            }),
            ..legacy.clone()
        },
        EventFilter {
            range: Some(TimeRange {
                start: Timestamp(0),
                end: Timestamp(u64::MAX),
            }),
            ..legacy.clone()
        },
    ] {
        assert!(reader.list_events(invalid.clone()).is_err());
        assert!(reader
            .event_summary(EventSummaryRequest { filter: invalid })
            .is_err());
    }
    assert!(reader
        .list_events(EventFilter {
            query: "🦀".repeat(128),
            ..legacy
        })
        .is_ok());
}

#[test]
fn event_search_matches_name_prefixes_and_ignores_punctuation() {
    let f = EventsFixture::new();
    f.run("scope", 0, "Scope", "Vexlin-session");
    f.event(
        "death",
        10,
        InstanceEventKind::PlayerDeath {
            player_id: None,
            player_name: "Vexlin%".into(),
            killed_by: Some("Quenby_".into()),
            level: None,
            grave_type: None,
            object_id: None,
            class_id: None,
            is_local: false,
        },
        Some("scope"),
    );
    f.event(
        "left",
        20,
        InstanceEventKind::PlayerLeft {
            player_name: Some("Morrow".into()),
            object_id: 1,
            class_id: None,
        },
        None,
    );
    f.event(
        "realm",
        30,
        InstanceEventKind::RealmEventKilled {
            boss_key: "Kestrel".into(),
            killer: Some("Vexlin\\Quenby".into()),
            object_type: None,
        },
        None,
    );
    f.portal("portal", 40, "Vault", Some(true));
    let reader = f.reader();
    let summary = reader
        .event_summary(EventSummaryRequest {
            filter: event_filter(),
        })
        .unwrap();
    assert_eq!(
        summary.by_kind,
        KindCounts {
            player_death: 1,
            player_left: 1,
            portal_opened: 1,
            realm_event_killed: 1
        }
    );
    for (query, expected) in [
        ("%", vec!["portal", "realm", "left", "death"]),
        ("_", vec!["portal", "realm", "left", "death"]),
        ("\\", vec!["portal", "realm", "left", "death"]),
        ("mOrRoW", vec!["left"]),
        ("orr", vec![]),
        ("Vex", vec!["portal", "realm", "death"]),
        ("xlin", vec![]),
        ("kestrel", vec!["realm"]),
        ("Vexlin", vec!["portal", "realm", "death"]),
    ] {
        let filter = EventFilter {
            query: query.into(),
            ..event_filter()
        };
        let page = reader.list_events(filter.clone()).unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            reader
                .event_summary(EventSummaryRequest { filter })
                .unwrap(),
            summary
        );
    }
    let pops = reader
        .list_events(EventFilter {
            query: "%".into(),
            kinds: vec![InstanceEventTag::PortalOpened],
            ..event_filter()
        })
        .unwrap();
    assert_eq!(
        pops.rows
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        vec!["portal"]
    );
    let summary = reader
        .event_summary(EventSummaryRequest {
            filter: EventFilter {
                run_id: Some("scope".into()),
                query: "no match".into(),
                ..event_filter()
            },
        })
        .unwrap();
    assert_eq!(summary.total, 1);
    assert_eq!(summary.by_kind.player_death, 1);
}

#[test]
fn portal_run_matches_obey_session_dungeon_window_and_intervening_pops() {
    const MINUTE: u64 = 60_000_000;
    let f = EventsFixture::new();
    f.portal("three", 0, "Three", Some(true));
    f.run("three-run", 3 * MINUTE, "Three", "Vexlin-session");
    f.run("three-later", 4 * MINUTE, "Three", "Vexlin-session");
    f.portal("eleven", 0, "Eleven", Some(true));
    f.run("eleven-run", 11 * MINUTE, "Eleven", "Vexlin-session");
    f.portal("session", 0, "Session", Some(true));
    f.run("other-session", MINUTE, "Session", "Quenby-session");
    f.portal("early", 0, "Double", Some(true));
    f.portal("late", MINUTE, "Double", Some(false));
    f.run("double-run", 3 * MINUTE, "Double", "Vexlin-session");
    f.portal("inclusive", 0, "Ten", None);
    f.run("ten-run", 10 * MINUTE, "Ten", "Vexlin-session");
    f.portal("same-time", MINUTE, "Same", None);
    f.run("same-run", MINUTE, "Same", "Vexlin-session");
    f.event(
        "left",
        0,
        InstanceEventKind::PlayerLeft {
            object_id: 1,
            player_name: None,
            class_id: None,
        },
        None,
    );
    let reader = f.reader();
    let page = reader.list_events(event_filter()).unwrap();
    let pairs: std::collections::BTreeMap<_, _> = page
        .entered_runs
        .iter()
        .map(|r| (r.event_id.as_str(), r.run_id.as_str()))
        .collect();
    assert_eq!(
        pairs,
        std::collections::BTreeMap::from([
            ("three", "three-run"),
            ("late", "double-run"),
            ("inclusive", "ten-run"),
            ("same-time", "same-run")
        ])
    );
    assert!(page
        .entered_runs
        .iter()
        .all(|r| r.outcome.as_deref() == Some("cleared")));
    let filtered = reader
        .list_events(EventFilter {
            opener_local: Some(true),
            ..event_filter()
        })
        .unwrap();
    assert_eq!(
        filtered.entered_runs.len(),
        1,
        "a filtered-out later pop still blocks an earlier match"
    );
    assert_eq!(filtered.entered_runs[0].event_id, "three");
}

#[test]
fn event_summary_scope_ranking_local_counts_and_global_flagged_since() {
    let f = EventsFixture::new();
    let reader = f.reader();
    assert_eq!(
        reader
            .event_summary(EventSummaryRequest {
                filter: event_filter()
            })
            .unwrap(),
        EventSummary::default()
    );
    f.portal("old", 1, "Old", Some(false));
    f.portal("a", 10, "Vault", Some(true));
    f.portal("b", 20, "Vault", Some(false));
    f.portal("c", 30, "Vault", Some(true));
    f.portal("d", 25, "Abbey", Some(true));
    f.portal("e", 26, "Abbey", None);
    let filter = EventFilter {
        range: Some(TimeRange {
            start: Timestamp(10),
            end: Timestamp(31),
        }),
        kinds: vec![InstanceEventTag::PlayerLeft; 5],
        dungeon: Some("Ignored".into()),
        query: "no match".into(),
        opener_local: Some(false),
        limit: 0,
        cursor: Some(EventCursor {
            at: Timestamp(u64::MAX),
            id: String::new(),
        }),
        ..event_filter()
    };
    let summary = reader
        .event_summary(EventSummaryRequest { filter })
        .unwrap();
    assert_eq!(summary.total, 5);
    assert_eq!(summary.by_kind.portal_opened, 5);
    assert_eq!(summary.local_pops, 3);
    assert_eq!(summary.flagged_since, Some(Timestamp(1)));
    assert_eq!(summary.first_at, Some(Timestamp(10)));
    assert_eq!(
        summary
            .dungeons
            .iter()
            .map(|p| (p.dungeon.as_str(), p.pops))
            .collect::<Vec<_>>(),
        [("Vault", 3), ("Abbey", 2)]
    );
    assert_eq!(
        summary.last_local_pop,
        Some(DungeonPops {
            dungeon: "Vault".into(),
            pops: 2,
            last_at: Timestamp(30)
        })
    );
    for n in 0..260 {
        f.portal(&format!("rank-{n}"), 100, &format!("Dungeon {n:03}"), None);
    }
    let summary = reader
        .event_summary(EventSummaryRequest {
            filter: EventFilter {
                range: Some(TimeRange {
                    start: Timestamp(100),
                    end: Timestamp(101),
                }),
                ..event_filter()
            },
        })
        .unwrap();
    assert_eq!(summary.total, 260);
    assert_eq!(summary.dungeons.len(), 256);
    assert_eq!(summary.dungeons[0].dungeon, "Dungeon 000");
    assert_eq!(summary.dungeons[255].dungeon, "Dungeon 255");
    assert_eq!(summary.local_pops, 0);
    assert_eq!(summary.last_local_pop, None);
    assert_eq!(summary.flagged_since, Some(Timestamp(1)));
}

#[test]
fn combined_portal_aggregates_match_separate_queries_before_ranking_cap() {
    let f = EventsFixture::new();
    for n in 0..300 {
        f.portal(
            &format!("aggregate-{n}"),
            n + 1,
            &format!("Dungeon {n:03}"),
            [Some(true), Some(false), None][n as usize % 3],
        );
    }
    f.event(
        "unknown-local",
        1000,
        InstanceEventKind::PortalOpened {
            opener: Some("Vexlin".into()),
            opener_is_local: Some(true),
            dungeon: None,
            portal_type: None,
        },
        None,
    );
    let reader = f.reader();
    for (start, end) in [(0, 1000), (0, 1001), (150, 250), (1001, 2000)] {
        let summary = reader
            .event_summary(EventSummaryRequest {
                filter: EventFilter {
                    range: Some(TimeRange {
                        start: Timestamp(start),
                        end: Timestamp(end),
                    }),
                    ..event_filter()
                },
            })
            .unwrap();
        let args = rusqlite::params![start as i64, end as i64];
        // Pre-rewrite statements are an independent equivalence oracle. The
        // latest local dungeon can be outside the 256 ranked rows or unknown.
        let mut statement = f.db.prepare("SELECT json_extract(json,'$.kind.dungeon') dungeon,count(*),max(at)
            FROM instance_events WHERE kind='portal_opened' AND at>=?1 AND at<?2 AND dungeon IS NOT NULL
            GROUP BY dungeon ORDER BY count(*) DESC,dungeon ASC LIMIT 256").unwrap();
        let ranked = statement
            .query_map(args, |row| {
                Ok(DungeonPops {
                    dungeon: row.get(0)?,
                    pops: row.get(1)?,
                    last_at: Timestamp(row.get::<_, i64>(2)? as u64),
                })
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(summary.dungeons, ranked);
        let mut statement = f.db.prepare("SELECT json_extract(json,'$.kind.dungeon') dungeon,count(*),max(at),sum(count(*)) OVER ()
            FROM instance_events WHERE kind='portal_opened' AND at>=?1 AND at<?2
            AND json_extract(json,'$.kind.opener_is_local')=1
            GROUP BY dungeon ORDER BY max(at) DESC,dungeon ASC LIMIT 1").unwrap();
        let mut rows = statement.query(args).unwrap();
        let (local, latest) = if let Some(row) = rows.next().unwrap() {
            let dungeon: Option<String> = row.get(0).unwrap();
            let pops = row.get(1).unwrap();
            let last_at = Timestamp(row.get::<_, i64>(2).unwrap() as u64);
            (
                row.get::<_, u32>(3).unwrap(),
                dungeon.map(|dungeon| DungeonPops {
                    dungeon,
                    pops,
                    last_at,
                }),
            )
        } else {
            (0, None)
        };
        assert_eq!(summary.local_pops, local);
        assert_eq!(summary.last_local_pop, latest);
    }
}

#[test]
fn searched_event_pages_merge_kinds_without_duplicates_and_preserve_ties() {
    let fixture = EventsFixture::new();
    for n in 0..180 {
        let id = format!("event-{n:03}");
        if n % 2 == 0 {
            fixture.portal(&id, n / 3, "Synthetic dungeon", Some(true));
        } else {
            fixture.event(
                &id,
                n / 3,
                InstanceEventKind::PlayerLeft {
                    player_name: Some("Vexlin".into()),
                    class_id: None,
                    object_id: 1,
                },
                None,
            );
        }
    }
    let reader = fixture.reader();
    for kinds in [
        vec![],
        vec![
            InstanceEventTag::PortalOpened,
            InstanceEventTag::PlayerLeft,
            InstanceEventTag::PortalOpened,
        ],
    ] {
        let mut filter = event_filter();
        filter.query = "Vex".into();
        filter.kinds = kinds;
        filter.limit = 17;
        let mut ids = Vec::new();
        loop {
            let page = reader.list_events(filter.clone()).unwrap();
            ids.extend(page.rows.into_iter().map(|row| row.id));
            filter.cursor = page.next;
            if filter.cursor.is_none() {
                break;
            }
        }
        let expected = (0..180)
            .rev()
            .map(|n| format!("event-{n:03}"))
            .collect::<Vec<_>>();
        assert_eq!(ids, expected);
    }
}

#[test]
fn alias_named_portals_do_not_match_canonical_run_names() {
    let fixture = EventsFixture::new();
    fixture.portal("alias", 10, "Synthetic alias", Some(true));
    fixture.run(
        "canonical-run",
        20,
        "Synthetic canonical dungeon",
        "Vexlin-session",
    );
    let page = fixture.reader().list_events(event_filter()).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert!(page.entered_runs.is_empty());
    assert!(
        matches!(&page.rows[0].kind, InstanceEventKind::PortalOpened { dungeon: Some(name), .. } if name == "Synthetic alias")
    );
}

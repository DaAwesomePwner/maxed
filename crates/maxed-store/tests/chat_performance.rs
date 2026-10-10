mod common;
use common::chat::*;
use maxed_store::HiddenLists;
use maxed_types::*;
use std::time::Instant;

#[test]
#[ignore = "benchmark: cargo test --release --locked -p maxed-store --test chat_performance -- --ignored --nocapture"]
fn five_hundred_thousand_messages() {
    let (dir, run_id) = seeded_benchmark();
    let path = dir.path().join("db");
    let reader = maxed_store::open_reader(&path).unwrap();
    let filter = ChatFilter {
        channel: None,
        sender: None,
        query: String::new(),
        run_id: None,
        show_npc: false,
        show_hidden: false,
        cursor: None,
        limit: 100,
    };
    let t = Instant::now();
    reader
        .list_chat(filter.clone(), &HiddenLists::default())
        .unwrap();
    println!("first page: {:?}", t.elapsed());
    let t = Instant::now();
    reader
        .list_chat(
            ChatFilter {
                query: "halls".into(),
                ..filter
            },
            &HiddenLists::default(),
        )
        .unwrap();
    let search_time = t.elapsed();
    println!("search page: {search_time:?}");
    let t = Instant::now();
    let overview = reader
        .chat_overview(ChatOverviewRequest { run_id: None })
        .unwrap();
    let all_time = t.elapsed();
    assert_eq!(overview.lines, 500_000);
    let t = Instant::now();
    let scoped = reader
        .chat_overview(ChatOverviewRequest {
            run_id: Some(run_id.clone()),
        })
        .unwrap();
    let run_time = t.elapsed();
    assert_eq!(scoped.lines, 50_000);
    let t = Instant::now();
    let sender = reader
        .chat_sender(ChatSenderRequest {
            name: "Kestrel".into(),
        })
        .unwrap()
        .unwrap();
    let sender_time = t.elapsed();
    assert!(sender.lines > 30_000);
    println!(
        "busy sender lines: {}; whisper rows in store: {}",
        sender.lines, overview.by_channel.whisper
    );
    let t = Instant::now();
    let regular_sender = reader
        .chat_sender(ChatSenderRequest {
            name: "Morrow".into(),
        })
        .unwrap()
        .unwrap();
    let regular_sender_time = t.elapsed();
    assert_eq!(regular_sender.lines, 500);
    println!("regular sender lines: {}", regular_sender.lines);
    let t = Instant::now();
    let partners = reader
        .whisper_partners(
            WhisperPartnerFilter {
                query: String::new(),
                show_hidden: false,
                limit: 200,
            },
            &HiddenLists::default(),
        )
        .unwrap();
    let partner_time = t.elapsed();
    assert_eq!(partners.rows.len(), 200);
    assert_eq!(partners.people, 500);
    for (label, elapsed) in [
        ("overview all-time", all_time),
        ("overview run", run_time),
        ("sender busy", sender_time),
        ("sender 500 lines", regular_sender_time),
        ("whisper partners 200", partner_time),
    ] {
        println!("{label}: {elapsed:?}; same-run FTS gate: {search_time:?}; within gate: {}; target <=250ms: {}",
            elapsed <= search_time, elapsed <= std::time::Duration::from_millis(250));
    }
    let plans = rusqlite::Connection::open(&path).unwrap();
    for (label, sql, args) in [
        ("overview all-time", "SELECT channel,count(*),min(at),max(at) FROM chat_messages INDEXED BY chat_channel WHERE 1=1 GROUP BY channel", vec![]),
        ("overview run", "SELECT channel,count(*),min(at),max(at) FROM chat_messages INDEXED BY chat_run WHERE run_id=?1 GROUP BY channel", vec![run_id]),
        ("sender latest", "SELECT length(CAST(json AS BLOB)),json FROM chat_messages WHERE sender=?1 ORDER BY at DESC,id DESC LIMIT 1", vec!["Kestrel".into()]),
        ("sender counts", "SELECT count(*),min(at) FROM chat_messages WHERE sender=?1", vec!["Kestrel".into()]),
        ("sender channels", "SELECT DISTINCT channel FROM chat_messages WHERE sender=?1", vec!["Kestrel".into()]),
        ("sender whispers", "SELECT count(*) FROM chat_messages WHERE rowid IN (SELECT rowid FROM chat_messages INDEXED BY chat_sender WHERE sender=?1 AND channel='whisper' UNION SELECT rowid FROM chat_messages INDEXED BY chat_counterpart WHERE counterpart=?1 AND channel='whisper')", vec!["Kestrel".into()]),
    ] { explain(&plans, label, sql, &args); }
    let sql = include_str!("../src/whisper_partners.sql")
        .replace("/* message predicates */", "1=1")
        .replace(
            "/* partner predicates */",
            "partner IS NOT NULL AND length(partner)>0",
        )
        .replace(":limit", "200");
    explain(&plans, "whisper partners", &sql, &[]);
    drop(plans);
    let t = Instant::now();
    while maxed_store::maintain_chat_file(
        &path,
        maxed_store::ChatMaintenance::Prune {
            before: Timestamp(1_250_000),
        },
    )
    .unwrap()
        > 0
    {}
    println!("prune 250k: {:?}", t.elapsed());
}

fn explain(db: &rusqlite::Connection, label: &str, sql: &str, args: &[String]) {
    let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let rows = statement
        .query_map(rusqlite::params_from_iter(args), |row| {
            row.get::<_, String>(3)
        })
        .unwrap();
    println!("EXPLAIN {label}:");
    for row in rows {
        println!("  {}", row.unwrap());
    }
}

fn seeded_benchmark() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut events = common::story_events();
    for event in &mut events {
        event.stream_id = "synthetic-benchmark-run".into();
    }
    maxed_store::apply_batch(&mut db, &events).unwrap();
    let run_id = demo::demo_run_story().run.id;
    let mut regular_sender_lines = 0;
    let rows: Vec<_> = (0..500_000u64)
        .map(|i| {
            let channel = if i % 7 == 0 {
                ChatChannel::Whisper
            } else {
                ChatChannel::Public
            };
            let mut m = message(
                i + 1,
                channel,
                &format!("SynthUser{}", i % 500),
                &format!("synthetic message number {i} about lost halls"),
                1_000_000 + i,
            );
            if channel == ChatChannel::Public && regular_sender_lines < 500 {
                m.sender = "Morrow".into();
                regular_sender_lines += 1;
            }
            if i < 50_000 {
                m.run_id = Some(run_id.clone());
            }
            if channel == ChatChannel::Whisper && i % 2 == 0 {
                m.counterpart = Some(m.sender.clone());
                m.sender = "Kestrel".into();
                m.direction = Some(WhisperDirection::Outgoing);
                m.is_local = true;
            }
            (m, None)
        })
        .collect();
    apply_chat(&mut db, &rows);
    drop(db);
    (dir, run_id)
}

#[test]
#[ignore = "benchmark: cargo test --release --locked -p maxed-store --test chat_performance whisper_partners_hidden_keywords -- --ignored --nocapture"]
fn whisper_partners_hidden_keywords() {
    let (dir, _) = seeded_benchmark();
    let reader = maxed_store::open_reader(&dir.path().join("db")).unwrap();
    let filter = WhisperPartnerFilter {
        query: String::new(),
        show_hidden: false,
        limit: 200,
    };
    let mut baseline = None;
    for count in [0, MAX_HIDDEN_KEYWORDS] {
        // All terms miss, so every whisper must check the full keyword list.
        let hidden = HiddenLists {
            senders: vec![],
            keywords: (0..count)
                .map(|i| format!("hidden synthetic keyword {i}"))
                .collect(),
        };
        let started = Instant::now();
        let partners = reader.whisper_partners(filter.clone(), &hidden).unwrap();
        println!("whisper partners, {count} hidden keywords: {:?}; store: 500000 messages; whisper lines: {}; people: {}; returned rows: {}",
            started.elapsed(), partners.lines, partners.people, partners.rows.len());
        assert_eq!(partners.lines, 71_429);
        assert_eq!(partners.people, 500);
        assert_eq!(partners.rows.len(), 200);
        if let Some(expected) = &baseline {
            assert_eq!(&partners, expected);
        } else {
            baseline = Some(partners);
        }
    }
}

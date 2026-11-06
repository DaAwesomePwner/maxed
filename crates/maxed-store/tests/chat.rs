use crate::common;
use common::chat::*;
use maxed_store::{AlertContext, ChatContext};
use maxed_types::*;
use rusqlite::Connection;

#[test]
fn stores_chat_and_upserts_by_id() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let m = message(1, ChatChannel::Guild, "SynthAlpha", "hello", 10_000_000);
    apply_chat(&mut db, &[(m.clone(), None)]);
    assert_eq!(count(&db), 1);
    let (channel, sender): (String, String) = db
        .query_row("SELECT channel,sender FROM chat_messages", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((channel.as_str(), sender.as_str()), ("guild", "SynthAlpha"));
    apply_chat(&mut db, &[(m.clone(), None)]);
    let mut updated = m;
    updated.text = "updated".into();
    maxed_store::apply_batch(
        &mut db,
        &[common::envelope(3, EngineEvent::Chat(updated.clone()))],
    )
    .unwrap();
    assert_eq!(count(&db), 1);
    let stored: String = db
        .query_row("SELECT json FROM chat_messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<ChatMessageView>(&stored).unwrap(),
        updated
    );
}

#[test]
fn switched_off_channel_is_not_stored_but_sequence_continues() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let off = Some(ChatContext {
        store: false,
        hidden: false,
        alertable: false,
    });
    let on = Some(ChatContext {
        store: true,
        hidden: false,
        alertable: false,
    });
    apply_chat(
        &mut db,
        &[
            (
                message(1, ChatChannel::Guild, "SynthAlpha", "before", 10_000_000),
                on,
            ),
            (
                message(2, ChatChannel::Guild, "SynthAlpha", "during", 11_000_000),
                off,
            ),
            (
                message(3, ChatChannel::Guild, "SynthAlpha", "after", 12_000_000),
                on,
            ),
        ],
    );
    let texts: Vec<String> = db
        .prepare("SELECT text FROM chat_messages ORDER BY at")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(texts, ["before", "after"]);
    let head: i64 = common::receipts::head(&db, "test-stream").unwrap();
    assert_eq!(head, 4);
}

#[test]
fn oversize_text_is_rejected() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut m = message(1, ChatChannel::Public, "SynthAlpha", "", 10_000_000);
    m.text = "x".repeat(MAX_CHAT_TEXT_BYTES + 1);
    let events = vec![
        (
            common::envelope(1, EngineEvent::SessionUpdated(session())),
            AlertContext::default(),
        ),
        (
            common::envelope(2, EngineEvent::Chat(m)),
            AlertContext::default(),
        ),
    ];
    assert!(maxed_store::apply_contexts_for_tests(&mut db, &events).is_err());
}

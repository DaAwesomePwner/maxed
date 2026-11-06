use crate::common;
use common::chat::*;
use maxed_store::ChatContext;
use maxed_types::*;
use rusqlite::Connection;

fn alerts(db: &Connection) -> Vec<(String, String)> {
    db.prepare("SELECT kind,json FROM alerts ORDER BY at")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn live(db: &mut Connection, m: ChatMessageView, chat: ChatContext) {
    // Live contexts carry preferences; the default preferences enable whisper toasts.
    let session = session();
    let ctx = |c: Option<ChatContext>| maxed_store::AlertContext {
        live: true,
        preferences: AlertPreferences::default(),
        chat: c,
        ..maxed_store::AlertContext::default()
    };
    maxed_store::apply_contexts_for_tests(
        db,
        &[
            (
                common::envelope(1, EngineEvent::SessionUpdated(session)),
                ctx(None),
            ),
            (common::envelope(2, EngineEvent::Chat(m)), ctx(Some(chat))),
        ],
    )
    .unwrap();
}

const ALERTABLE: ChatContext = ChatContext {
    store: true,
    hidden: false,
    alertable: true,
};

#[test]
fn incoming_whisper_creates_alert_with_sender_and_excerpt() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    live(
        &mut db,
        message(
            1,
            ChatChannel::Whisper,
            "SynthAlpha",
            "need a carry?",
            10_000_000,
        ),
        ALERTABLE,
    );
    let rows = alerts(&db);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "whisper");
    let view: AlertView = serde_json::from_str(&rows[0].1).unwrap();
    assert_eq!(view.explanation, "Whisper from SynthAlpha: need a carry?");
    assert_eq!(
        view.details,
        Some(AlertDetails::Chat {
            channel: ChatChannel::Whisper,
            sender: "SynthAlpha".into(),
            excerpt: "need a carry?".into()
        })
    );
}

#[test]
fn hidden_unalertable_and_public_messages_never_alert() {
    for (m, ctx) in [
        (
            message(1, ChatChannel::Whisper, "SynthAlpha", "x", 1),
            ChatContext {
                hidden: true,
                ..ALERTABLE
            },
        ),
        (
            message(1, ChatChannel::Whisper, "SynthAlpha", "x", 1),
            ChatContext {
                alertable: false,
                ..ALERTABLE
            },
        ),
        (
            message(1, ChatChannel::Public, "SynthAlpha", "x", 1),
            ALERTABLE,
        ),
    ] {
        let mut db = Connection::open_in_memory().unwrap();
        maxed_store::migrate(&mut db).unwrap();
        live(&mut db, m, ctx);
        assert!(alerts(&db).is_empty());
    }
}

#[test]
fn channel_switched_off_still_alerts() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    live(
        &mut db,
        message(1, ChatChannel::Whisper, "SynthAlpha", "x", 10_000_000),
        ChatContext {
            store: false,
            ..ALERTABLE
        },
    );
    assert_eq!(count(&db), 0);
    assert_eq!(alerts(&db).len(), 1);
}

#[test]
fn demo_never_alerts() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    apply_chat(
        &mut db,
        &[(
            message(1, ChatChannel::Whisper, "SynthAlpha", "x", 1),
            Some(ALERTABLE),
        )],
    );
    assert!(alerts(&db).is_empty());
}

#[test]
fn guild_and_party_alerts_keep_unicode_excerpts_and_default_to_quiet() {
    for (channel, kind, prefix) in [
        (
            ChatChannel::Guild,
            AlertKind::GuildMessage,
            "Guild message from",
        ),
        (
            ChatChannel::Party,
            AlertKind::PartyMessage,
            "Party message from",
        ),
    ] {
        let mut db = Connection::open_in_memory().unwrap();
        maxed_store::migrate(&mut db).unwrap();
        live(
            &mut db,
            message(1, channel, "SynthBravo", &"é".repeat(121), 10_000_000),
            ALERTABLE,
        );
        let rows = alerts(&db);
        assert_eq!(rows.len(), 1);
        let view: AlertView = serde_json::from_str(&rows[0].1).unwrap();
        assert_eq!(view.kind, kind);
        assert_eq!(view.presentation, Some(AlertLoudness::QuietLogOnly));
        let excerpt = format!("{}…", "é".repeat(119));
        assert_eq!(view.explanation, format!("{prefix} SynthBravo: {excerpt}"));
        assert_eq!(
            view.details,
            Some(AlertDetails::Chat {
                channel,
                sender: "SynthBravo".into(),
                excerpt,
            })
        );
    }
}

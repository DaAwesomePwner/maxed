use maxed_store::{AlertContext, ChatContext};
use maxed_types::*;
use rusqlite::Connection;

pub fn session() -> PlaySessionView {
    super::story_events()
        .into_iter()
        .find_map(|e| match e.event {
            EngineEvent::SessionUpdated(s) => Some(s),
            _ => None,
        })
        .unwrap()
}

pub fn message(n: u64, channel: ChatChannel, sender: &str, text: &str, at: u64) -> ChatMessageView {
    ChatMessageView {
        id: format!("test-stream:chat:{n}"),
        session_id: session().id,
        run_id: None,
        map: Some("Nexus".into()),
        at: Timestamp(at),
        channel,
        direction: (channel == ChatChannel::Whisper).then_some(WhisperDirection::Incoming),
        sender_kind: ChatSenderKind::Player,
        sender: sender.into(),
        counterpart: (channel == ChatChannel::Whisper).then(|| sender.to_owned()),
        is_local: false,
        stars: None,
        supporter: false,
        text: text.into(),
        truncated: false,
    }
}

/// Session first, then one envelope per message, applied with the given chat context.
pub fn apply_chat(db: &mut Connection, messages: &[(ChatMessageView, Option<ChatContext>)]) {
    let mut events = vec![(
        super::envelope(1, EngineEvent::SessionUpdated(session())),
        AlertContext::default(),
    )];
    for (i, (m, chat)) in messages.iter().enumerate() {
        events.push((
            super::envelope(i as u64 + 2, EngineEvent::Chat(m.clone())),
            AlertContext {
                chat: *chat,
                ..AlertContext::default()
            },
        ));
    }
    maxed_store::apply_contexts_for_tests(db, &events).unwrap();
}

pub fn count(db: &Connection) -> i64 {
    db.query_row("SELECT count(*) FROM chat_messages", [], |r| r.get(0))
        .unwrap()
}

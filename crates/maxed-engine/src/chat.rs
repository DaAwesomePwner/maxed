//! TEXT packet classification for the Logs tab. Channel rules follow
//! RealmShark's ChatGUI: `*Guild*`, `*Party*`, other non-empty recipients are
//! whispers. They are not yet verified against Maxed's own live capture.
use maxed_types::{ChatChannel, ChatSenderKind, WhisperDirection, MAX_CHAT_TEXT_BYTES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    pub channel: ChatChannel,
    pub direction: Option<WhisperDirection>,
    pub sender_kind: ChatSenderKind,
    pub sender: String,
    pub counterpart: Option<String>,
    pub is_local: bool,
    pub text: String,
    pub truncated: bool,
}

pub use crate::names::display_name;

fn cut(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_owned(), false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

pub fn classify(name: &str, recipient: &str, text: &str, local: Option<&str>) -> Classified {
    let recipient = recipient.trim();
    let channel = if recipient.is_empty() {
        ChatChannel::Public
    } else if recipient.contains("*Guild*") {
        ChatChannel::Guild
    } else if recipient.contains("*Party*") {
        ChatChannel::Party
    } else if recipient.contains('*') {
        ChatChannel::Unknown
    } else {
        ChatChannel::Whisper
    };
    let (sender_kind, sender) = if let Some(npc) = name.strip_prefix('#') {
        (ChatSenderKind::Npc, npc.to_owned())
    } else if name.is_empty() || (name.len() >= 2 && name.starts_with('*') && name.ends_with('*')) {
        (ChatSenderKind::System, name.to_owned())
    } else {
        (ChatSenderKind::Player, display_name(name).to_owned())
    };
    let local = local.map(display_name).filter(|l| !l.is_empty());
    let is_me = |candidate: &str| local.is_some_and(|l| l.eq_ignore_ascii_case(candidate));
    let is_local = sender_kind == ChatSenderKind::Player && is_me(&sender);
    let (direction, counterpart) = if channel == ChatChannel::Whisper {
        let target = display_name(recipient).to_owned();
        if is_me(&target) {
            (Some(WhisperDirection::Incoming), Some(sender.clone()))
        } else if is_local {
            (Some(WhisperDirection::Outgoing), Some(target))
        } else {
            (Some(WhisperDirection::Unknown), Some(target))
        }
    } else {
        (None, None)
    };
    let (text, truncated) = cut(text, MAX_CHAT_TEXT_BYTES);
    Classified {
        channel,
        direction,
        sender_kind,
        sender,
        counterpart,
        is_local,
        text,
        truncated,
    }
}

impl crate::Engine {
    fn local_name(&self) -> Option<&str> {
        self.local
            .as_ref()
            .and_then(|local| self.entities.get(&local.object))
            .and_then(|entity| entity.name.as_deref())
            .filter(|name| !name.is_empty())
    }

    pub(crate) fn emit_chat(
        &mut self,
        p: &maxed_protocol::defs::Text,
        events: &mut Vec<maxed_types::EngineEvent>,
    ) {
        let c = classify(&p.name, &p.recipient, &p.text, self.local_name());
        self.chat_emitted += 1;
        events.push(maxed_types::EngineEvent::Chat(
            maxed_types::ChatMessageView {
                id: format!("{}:chat:{:012}", self.config.stream_id, self.chat_emitted),
                session_id: self.session_id(),
                run_id: self.run.as_ref().map(|r| r.view.id.clone()),
                map: self.map.as_deref().map(|map| cut(map, 256).0),
                at: crate::clock::millis(self.now),
                channel: c.channel,
                direction: c.direction,
                sender_kind: c.sender_kind,
                sender: cut(&c.sender, 256).0,
                counterpart: c.counterpart.as_deref().map(|name| cut(name, 256).0),
                is_local: c.is_local,
                stars: u8::try_from(p.numStars).ok(),
                supporter: p.isSupporter,
                text: c.text,
                truncated: c.truncated,
            },
        ));
    }

    /// Chat alerts need a live stream and a known local identity; otherwise a
    /// message from you can't be ruled out, matching the Key Popped rule.
    pub fn chat_alertable(&self) -> bool {
        self.config.stream_id.starts_with("live-") && self.local_name().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::{ChatChannel as C, ChatSenderKind as K, WhisperDirection as D};

    #[test]
    fn channels_follow_recipient_markers() {
        let me = Some("SynthMe");
        assert_eq!(classify("SynthAlpha", "", "hi", me).channel, C::Public);
        assert_eq!(classify("SynthAlpha", "  ", "hi", me).channel, C::Public);
        assert_eq!(
            classify("SynthAlpha", "*Guild*", "hi", me).channel,
            C::Guild
        );
        assert_eq!(
            classify("SynthAlpha", "*Party*", "hi", me).channel,
            C::Party
        );
        assert_eq!(
            classify("SynthAlpha", "SynthMe", "hi", me).channel,
            C::Whisper
        );
        assert_eq!(
            classify("SynthAlpha", "*Mystery*", "hi", me).channel,
            C::Unknown
        );
    }

    #[test]
    fn whisper_direction_uses_local_identity() {
        let incoming = classify("SynthAlpha,7", "synthme", "hi", Some("SynthMe"));
        assert_eq!(incoming.direction, Some(D::Incoming));
        assert_eq!(incoming.sender, "SynthAlpha");
        assert_eq!(incoming.counterpart.as_deref(), Some("SynthAlpha"));
        assert!(!incoming.is_local);
        let outgoing = classify("SynthMe", "SynthBravo,3", "hi", Some("SynthMe"));
        assert_eq!(outgoing.direction, Some(D::Outgoing));
        assert_eq!(outgoing.counterpart.as_deref(), Some("SynthBravo"));
        assert!(outgoing.is_local);
        let unknown = classify("SynthAlpha", "SynthMe", "hi", None);
        assert_eq!(unknown.direction, Some(D::Unknown));
        assert_eq!(unknown.counterpart.as_deref(), Some("SynthMe"));
        let neither = classify("SynthAlpha", "SynthBravo", "hi", Some("SynthMe"));
        assert_eq!(neither.direction, Some(D::Unknown));
        assert_eq!(
            classify("SynthAlpha", "*Guild*", "hi", Some("SynthMe")).direction,
            None
        );
    }

    #[test]
    fn sender_kinds() {
        let npc = classify("#Oryx the Mad God", "", "x", None);
        assert_eq!(
            (npc.sender_kind, npc.sender.as_str()),
            (K::Npc, "Oryx the Mad God")
        );
        assert_eq!(classify("", "", "x", None).sender_kind, K::System);
        assert_eq!(classify("*Help*", "", "x", None).sender_kind, K::System);
        assert_eq!(classify("SynthAlpha", "", "x", None).sender_kind, K::Player);
        assert!(classify("SynthMe", "", "x", Some("synthme")).is_local);
    }

    #[test]
    fn text_is_cut_at_a_character_boundary() {
        let long = "😀".repeat(300); // 1200 bytes, 4 bytes per char
        let c = classify("SynthAlpha", "", &long, None);
        assert!(c.truncated);
        assert_eq!(c.text.len(), 1024);
        assert!(c.text.chars().all(|ch| ch == '😀'));
        let short = classify("SynthAlpha", "", "hi", None);
        assert!(!short.truncated);
        let odd = "a".to_string() + &"😀".repeat(300); // 1201 bytes; boundary at 1021
        let c = classify("SynthAlpha", "", &odd, None);
        assert_eq!(c.text.len(), 1021);
    }
}

//! Pure evidence rules. Persistence owns prior records and occurrence identity.
use maxed_types::*;

#[derive(Clone, Debug, PartialEq)]
pub struct PersonalBestCandidate {
    pub fight_id: String,
    pub character_id: String,
    pub boss_type: u16,
    pub at: Timestamp,
    pub dps: f64,
}
pub fn personal_best_candidate(
    fight: &FightView,
    character: &CharacterView,
) -> Option<PersonalBestCandidate> {
    if !crate::personal_best_eligible(fight, character)
        || fight.ended.is_none()
        || !matches!(
            fight.end_reason,
            Some(FightEnd::MapChanged | FightEnd::Killed)
        )
        || fight.first_hit? >= fight.last_hit?
    {
        return None;
    }
    let mut locals = fight.players.iter().filter(|p| p.is_local);
    let local = locals.next()?;
    if locals.next().is_some()
        || local.player_id != character.id
        || !local.damage.is_finite()
        || local.damage <= 0.
        || !local.dps?.is_finite()
        || local.dps? <= 0.
    {
        return None;
    }
    Some(PersonalBestCandidate {
        fight_id: fight.id.clone(),
        character_id: character.id.clone(),
        boss_type: fight.boss_type?,
        at: fight.last_hit?,
        dps: local.dps?,
    })
}
pub fn beats_previous(candidate: &PersonalBestCandidate, previous: Option<f64>) -> bool {
    previous
        .is_some_and(|p| p.is_finite() && p > 0. && candidate.dps.is_finite() && candidate.dps > p)
}
pub fn is_white_bag(drop: &DropView) -> bool {
    drop.bag_rank == Some(255)
}

impl crate::Engine {
    /// Resolve only catalog text; identity stays inside the engine.
    pub fn key_pop_dungeon(&self, event: &EngineEvent) -> Option<String> {
        if !self.config.stream_id.starts_with("live-") {
            return None;
        }
        let EngineEvent::InstanceEvent(view) = event else {
            return None;
        };
        if view.provisional {
            return None;
        }
        let InstanceEventKind::PortalOpened {
            opener: Some(opener),
            dungeon: Some(dungeon),
            ..
        } = &view.kind
        else {
            return None;
        };
        let local = self
            .local
            .as_ref()
            .and_then(|local| self.entities.get(&local.object))
            .and_then(|entity| entity.name.as_deref())
            .map(crate::names::display_name)
            .filter(|identity| !identity.is_empty())?;
        // Without a known local identity an own pop can't be ruled out, so stay silent.
        if local.eq_ignore_ascii_case(crate::names::display_name(opener)) {
            return None;
        }
        self.catalog
            .dungeons
            .iter()
            .find(|d| {
                d.kind == AreaKind::Dungeon && (&d.name == dungeon || d.aliases.contains(dungeon))
            })
            .map(|d| d.name.clone())
    }
}

/// The alert a stored chat message may raise. Only other players' messages
/// alert; whispers only when known to be incoming.
pub fn chat_alert_kind(m: &ChatMessageView) -> Option<AlertKind> {
    if m.sender_kind != ChatSenderKind::Player || m.is_local {
        return None;
    }
    match m.channel {
        ChatChannel::Whisper if m.direction == Some(WhisperDirection::Incoming) => {
            Some(AlertKind::Whisper)
        }
        ChatChannel::Guild => Some(AlertKind::GuildMessage),
        ChatChannel::Party => Some(AlertKind::PartyMessage),
        _ => None,
    }
}

/// At most 120 characters, ending in an ellipsis when cut.
pub fn chat_excerpt(text: &str) -> String {
    if text.chars().count() <= 120 {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(119).collect();
    out.push('…');
    out
}

//! Shared wire grammar for live notices and privacy-preserving fixture scrubbing.
pub struct OryxAnnouncement {
    pub boss_key: String,
    pub killer: Option<String>,
    pub(crate) killer_spans: Vec<std::ops::Range<usize>>,
}

/// Accept one localization object or two copies with the same full key and raw killer.
/// Unknown fields and escaped structural/localization keys are rejected.
pub fn parse_oryx_announcement(text: &str) -> Option<OryxAnnouncement> {
    if text.len() > 16_384 {
        return None;
    }
    let mut p = Json { text, at: 0 };
    let mut spans = Vec::new();
    let mut previous = None;
    for _ in 0..2 {
        p.take(b'{')?;
        let mut key = None;
        let mut killer = None;
        let mut killer_wire = None;
        loop {
            let field = p.literal_string()?;
            p.take(b':')?;
            match field.as_str() {
                "k" if key.is_none() => {
                    key = Some(p.literal_string()?);
                }
                "t" if killer.is_none() => {
                    p.take(b'{')?;
                    if p.literal_string()? != "KILLER" {
                        return None;
                    }
                    p.take(b':')?;
                    let (value, span) = p.string()?;
                    if value.len() > 256 {
                        return None;
                    }
                    killer = Some(value);
                    // Equal wire spans stay equal after length-preserving redaction.
                    killer_wire = Some(&text[span.clone()]);
                    spans.push(span);
                    if !p.end_object()? {
                        return None;
                    }
                }
                _ => return None,
            }
            if p.end_object()? {
                break;
            }
        }
        let key = key?;
        let tail = key.strip_prefix("stringlist.")?;
        let (boss, number) = if let Some(pair) = tail.rsplit_once(".death.") {
            if killer.is_some() {
                return None;
            }
            pair
        } else {
            killer.as_ref()?;
            tail.rsplit_once(".killed.")?
        };
        if boss.is_empty()
            || boss.len() > 256
            || !boss.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || number.is_empty()
            || !number.bytes().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        let boss_key = boss.to_owned();
        let object = (key, killer, killer_wire);
        if previous.as_ref().is_some_and(|old| old != &object) {
            return None;
        }
        p.ws();
        if p.at == text.len() {
            return Some(OryxAnnouncement {
                boss_key,
                killer: object.1,
                killer_spans: spans,
            });
        }
        previous = Some(object);
    }
    None
}

struct Json<'a> {
    text: &'a str,
    at: usize,
}
impl Json<'_> {
    fn ws(&mut self) {
        while self
            .text
            .as_bytes()
            .get(self.at)
            .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.at += 1;
        }
    }
    fn take(&mut self, c: u8) -> Option<()> {
        self.ws();
        if self.text.as_bytes().get(self.at) != Some(&c) {
            return None;
        }
        self.at += 1;
        Some(())
    }
    fn end_object(&mut self) -> Option<bool> {
        self.ws();
        if self.text.as_bytes().get(self.at) == Some(&b'}') {
            self.at += 1;
            return Some(true);
        }
        self.take(b',')?;
        self.ws();
        if self.text.as_bytes().get(self.at) == Some(&b'}') {
            self.at += 1;
            return Some(true);
        }
        Some(false)
    }
    fn string(&mut self) -> Option<(String, std::ops::Range<usize>)> {
        self.ws();
        let start = self.at;
        let mut stream =
            serde_json::Deserializer::from_str(&self.text[start..]).into_iter::<String>();
        let value = stream.next()?.ok()?;
        self.at += stream.byte_offset();
        Some((value, start + 1..self.at - 1))
    }

    fn literal_string(&mut self) -> Option<String> {
        let (value, span) = self.string()?;
        // Structural text is kept byte-for-byte, so escaped spellings must not
        // hide decoded secrets from verification of those retained chunks.
        (self.text[span] == value).then_some(value)
    }
}

/// The exact key-pop template, with one bounded JSON string identity. The span
/// includes quotes so the scrubber can pad escaped spellings outside the string.
pub struct PortalAnnouncement {
    pub player: String,
    pub(crate) player_span: std::ops::Range<usize>,
}
pub fn parse_portal_announcement(text: &str) -> Option<PortalAnnouncement> {
    if text.len() > 16_384 {
        return None;
    }
    let mut p = Json { text, at: 0 };
    p.take(b'{')?;
    let mut key = None;
    let mut player = None;
    loop {
        let field = p.literal_string()?;
        p.take(b':')?;
        match field.as_str() {
            "k" if key.is_none() => key = Some(p.literal_string()?),
            "t" if player.is_none() => {
                p.take(b'{')?;
                if p.literal_string()? != "player" {
                    return None;
                }
                p.take(b':')?;
                let (value, span) = p.string()?;
                if value.len() > 256 {
                    return None;
                }
                player = Some(PortalAnnouncement {
                    player: value,
                    player_span: span.start - 1..span.end + 1,
                });
                if !p.end_object()? {
                    return None;
                }
            }
            _ => return None,
        }
        if p.end_object()? {
            break;
        }
    }
    p.ws();
    if key.as_deref()? != "s.dungeon_opened_by" || p.at != text.len() {
        return None;
    }
    player
}

pub(crate) struct DeathAnnouncement {
    pub player_span: std::ops::Range<usize>,
    pub enemy_span: std::ops::Range<usize>,
    pub level_span: std::ops::Range<usize>,
}

/// Scrub only the exact death template. Game data stays bounded and typed;
/// duplicate/extra fields, escaped structure, nesting and trailing objects fail closed.
pub(crate) fn parse_death_announcement(text: &str) -> Option<DeathAnnouncement> {
    if text.len() > 16_384 {
        return None;
    }
    let mut p = Json { text, at: 0 };
    p.take(b'{')?;
    let mut key = None;
    let mut fields_seen = false;
    let (mut player_span, mut enemy_span, mut level_span) = (None, None, None);
    loop {
        let field = p.literal_string()?;
        p.take(b':')?;
        match field.as_str() {
            "k" if key.is_none() => key = Some(p.literal_string()?),
            "t" if !fields_seen => {
                fields_seen = true;
                p.take(b'{')?;
                loop {
                    let field = p.literal_string()?;
                    p.take(b':')?;
                    let (value, span) = p.string()?;
                    if value.is_empty() || value.len() > 256 {
                        return None;
                    }
                    let span = span.start - 1..span.end + 1;
                    match field.as_str() {
                        "player" if player_span.is_none() => player_span = Some(span),
                        "enemy" if enemy_span.is_none() => enemy_span = Some(span),
                        "level"
                            if level_span.is_none()
                                && value.bytes().all(|c| c.is_ascii_digit())
                                && value.parse::<u16>().is_ok() =>
                        {
                            level_span = Some(span)
                        }
                        _ => return None,
                    }
                    if p.end_object()? {
                        break;
                    }
                }
            }
            _ => return None,
        }
        if p.end_object()? {
            break;
        }
    }
    p.ws();
    if key.as_deref()? != "s.death" || p.at != text.len() {
        return None;
    }
    Some(DeathAnnouncement {
        player_span: player_span?,
        enemy_span: enemy_span?,
        level_span: level_span?,
    })
}

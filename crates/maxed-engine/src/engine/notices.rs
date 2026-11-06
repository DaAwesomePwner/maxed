//! Parse only the three authorized templates. Raw text is never retained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Notice {
    PlayerDeath {
        player: String,
        level: Option<u16>,
        enemy: String,
    },
    PortalOpened {
        player: String,
    },
    RealmEventKilled {
        boss_key: String,
        killer: String,
    },
}
pub(crate) const INSTANCE_EVENT_LIMIT: usize = 4096;
pub(crate) struct ObservedNotice {
    pub at: maxed_types::Timestamp,
    pub picture_type: Option<i32>,
    pub notice: Notice,
}
#[derive(Clone, Copy)]
pub(crate) struct LootKill {
    pub tick: u64,
    pub object_type: u16,
    pub pos: (f32, f32),
}
#[derive(Default)]
pub(crate) struct Notices {
    pub observed_types: std::collections::BTreeSet<u16>,
    pub entries: Vec<ObservedNotice>,
    pub dropped: u32,
    pub published: usize,
    pub emitted: usize,
    pub departures:
        std::collections::BTreeMap<(i32, u32), (maxed_types::Timestamp, maxed_types::PlayerDamage)>,
    pub dead: std::collections::BTreeSet<(i32, u32)>,
    pub loot_kills: std::collections::BTreeMap<(i32, u32), LootKill>,
}
impl Notices {
    pub fn push(&mut self, notice: Notice, at: maxed_types::Timestamp, picture_type: Option<i32>) {
        if self.entries.len() == INSTANCE_EVENT_LIMIT || self.emitted == INSTANCE_EVENT_LIMIT {
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.entries.push(ObservedNotice {
                at,
                picture_type,
                notice,
            });
        }
    }
}
pub(crate) fn parse(text: &str) -> Option<Notice> {
    if text.len() > 16_384 {
        return None;
    }
    let mut notice = if let Some(notice) = maxed_protocol::notices::parse_oryx_announcement(text) {
        Notice::RealmEventKilled {
            boss_key: notice.boss_key,
            killer: notice.killer.unwrap_or_default(),
        }
    } else if let Some(notice) = maxed_protocol::notices::parse_portal_announcement(text) {
        Notice::PortalOpened {
            player: notice.player,
        }
    } else {
        let mut p = Parser(text);
        let notice = parse_one(&mut p)?;
        p.0.trim().is_empty().then_some(notice)?
    };
    // Validate the full raw fields before normalizing names for display and matching.
    let name = match &mut notice {
        Notice::PlayerDeath { player, .. } | Notice::PortalOpened { player } => player,
        Notice::RealmEventKilled { killer, .. } => killer,
    };
    name.truncate(crate::names::display_name(name).len());
    Some(notice)
}

fn parse_one(p: &mut Parser<'_>) -> Option<Notice> {
    p.take('{')?;
    let mut key = None;
    let mut fields = None;
    loop {
        let name = p.string()?;
        p.take(':')?;
        match name.as_str() {
            "k" if key.is_none() => key = Some(p.string()?),
            "t" if fields.is_none() => fields = Some(p.fields()?),
            _ => return None,
        }
        if p.end_object()? {
            break;
        }
    }
    let key = key?;
    let mut fields = fields.unwrap_or_default();
    let notice = match key.as_str() {
        "s.death" if fields.len() == 3 => Notice::PlayerDeath {
            player: fields.remove("player")?,
            level: fields.remove("level")?.parse().ok(),
            enemy: fields.remove("enemy")?,
        },
        _ => return None,
    };
    let within_limit = match &notice {
        Notice::PlayerDeath { player, enemy, .. } => player.len() <= 256 && enemy.len() <= 256,
        Notice::PortalOpened { player } => player.len() <= 256,
        Notice::RealmEventKilled { boss_key, killer } => {
            boss_key.len() <= 256 && killer.len() <= 256
        }
    };
    within_limit.then_some(notice)
}

// A bounded, non-recursive parser for {"k":string,"t":{string:string,...}}.
// Unlike global comma replacement, escaped quotes and commas inside names
// remain intact. No arbitrary JSON or additional fields are accepted.
struct Parser<'a>(&'a str);
impl Parser<'_> {
    fn ws(&mut self) {
        self.0 = self.0.trim_start_matches([' ', '\t', '\r', '\n']);
    }
    fn take(&mut self, c: char) -> Option<()> {
        self.ws();
        self.0 = self.0.strip_prefix(c)?;
        Some(())
    }
    fn next(&mut self) -> Option<char> {
        let c = self.0.chars().next()?;
        self.0 = &self.0[c.len_utf8()..];
        Some(c)
    }
    fn hex(&mut self) -> Option<u16> {
        let mut n = 0;
        for _ in 0..4 {
            n = n * 16 + self.next()?.to_digit(16)? as u16;
        }
        Some(n)
    }
    fn string(&mut self) -> Option<String> {
        self.take('"')?;
        let mut out = String::new();
        loop {
            match self.next()? {
                '"' => return Some(out),
                '\\' => {
                    let escaped = match self.next()? {
                        '"' => '"',
                        '\\' => '\\',
                        '/' => '/',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'u' => {
                            let high = self.hex()?;
                            let code = if (0xd800..=0xdbff).contains(&high) {
                                if self.next()? != '\\' || self.next()? != 'u' {
                                    return None;
                                }
                                let low = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return None;
                                }
                                0x10000
                                    + ((u32::from(high) - 0xd800) << 10)
                                    + (u32::from(low) - 0xdc00)
                            } else {
                                u32::from(high)
                            };
                            char::from_u32(code)?
                        }
                        _ => return None,
                    };
                    out.push(escaped);
                }
                c if c < '\u{20}' => return None,
                c => out.push(c),
            }
        }
    }
    fn end_object(&mut self) -> Option<bool> {
        self.ws();
        if self.0.starts_with('}') {
            self.take('}')?;
            return Some(true);
        }
        self.take(',')?;
        self.ws();
        if self.0.starts_with('}') {
            self.take('}')?;
            return Some(true);
        }
        Some(false)
    }
    fn fields(&mut self) -> Option<std::collections::BTreeMap<String, String>> {
        self.take('{')?;
        let mut fields = std::collections::BTreeMap::new();
        loop {
            let key = self.string()?;
            self.take(':')?;
            let value = self.string()?;
            if fields.insert(key, value).is_some() || fields.len() > 3 {
                return None;
            }
            if self.end_object()? {
                return Some(fields);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notice_names_are_stripped_after_raw_length_validation() {
        for length in [256, 257] {
            let name = format!("SynthAlpha,{}", "x".repeat(length - "SynthAlpha,".len()));
            for message in [
                serde_json::json!({"k":"s.death","t":{"player":name,"level":"20","enemy":"Synthetic Boss"}}),
                serde_json::json!({"k":"s.dungeon_opened_by","t":{"player":name}}),
                serde_json::json!({"k":"stringlist.Synthetic_Boss.killed.0","t":{"KILLER":name}}),
            ] {
                let notice = parse(&message.to_string());
                if length == 257 {
                    assert_eq!(notice, None);
                    continue;
                }
                let name = match notice.unwrap() {
                    Notice::PlayerDeath { player, .. } | Notice::PortalOpened { player } => player,
                    Notice::RealmEventKilled { killer, .. } => killer,
                };
                assert_eq!(name, "SynthAlpha");
            }
        }
    }

    #[test]
    fn death_template_with_trailing_comma_parses() {
        assert_eq!(
            parse(
                r#"{"k":"s.death","t":{"player":"SynthAlpha","level":"20","enemy":"Synthetic Boss",}}"#
            ),
            Some(Notice::PlayerDeath {
                player: "SynthAlpha".into(),
                level: Some(20),
                enemy: "Synthetic Boss".into()
            })
        );
        assert_eq!(
            parse(r#"{"t":{"player":"SynthBravo"},"k":"s.dungeon_opened_by"}"#),
            Some(Notice::PortalOpened {
                player: "SynthBravo".into()
            })
        );
        assert_eq!(
            parse(r#"{"k":"stringlist.Synthetic_Boss.killed.0","t":{"KILLER":"SynthAlpha"}}"#),
            Some(Notice::RealmEventKilled {
                boss_key: "Synthetic_Boss".into(),
                killer: "SynthAlpha".into()
            })
        );
    }
    #[test]
    fn unknown_templates_are_ignored() {
        for text in [
            "hello",
            r#"{"k":"s.minion_food"}"#,
            r#"{"k":"stringlist.Boss.taunt.0","t":{"KILLER":"SynthAlpha"}}"#,
            r#"{"k":"s.death","t":{"player":"SynthAlpha"}}"#,
            r#"{"k":"s.dungeon_opened_by","t":{"player":"SynthAlpha","extra":"x"}}"#,
            r#"{"k":"s.dungeon_opened_by","k":"s.death","t":{"player":"SynthAlpha"}}"#,
        ] {
            assert_eq!(parse(text), None);
        }
        assert!(parse(r#"{"k":"s.dungeon_opened_by","t":{"player":"SynthAlpha"}}"#).is_some());
    }
    #[test]
    fn escaped_strings_truncation_and_wrong_shapes_are_safe() {
        let valid =
            r#"{"k":"s.dungeon_opened_by","t":{"player":"Synth\u0041lpha\uD83D\uDE00\" ,}",}}"#;
        assert_eq!(
            parse(valid),
            Some(Notice::PortalOpened {
                player: "SynthAlpha😀\" ".into()
            })
        );
        for i in 0..valid.len() {
            assert_eq!(parse(&valid[..i]), None);
        }
        for malformed in [
            r#"{"k":"s.dungeon_opened_by","t":{"player":"SynthAlpha","player":"SynthBravo"}}"#,
            r#"{"k":"s.dungeon_opened_by","t":{"player":"\uD800"}}"#,
            r#"{"k":"s.dungeon_opened_by","t":{"player":"\uDC00"}}"#,
            r#"{"k":"s.dungeon_opened_by","t":{"player":42}}"#,
            r#"{"k":"s.dungeon_opened_by","t":{"player":{"player":"SynthAlpha"}}}"#,
            r#"{"k":"stringlist.Synthetic_Boss.killed.x","t":{"KILLER":"SynthAlpha"}}"#,
        ] {
            assert_eq!(parse(malformed), None);
        }
        for length in [256, 257] {
            let value = "x".repeat(length);
            for message in [
                serde_json::json!({"k":"s.death","t":{"player":value,"level":"20","enemy":"Synthetic Boss"}}),
                serde_json::json!({"k":"s.death","t":{"player":"SynthAlpha","level":"20","enemy":value}}),
                serde_json::json!({"k":"s.dungeon_opened_by","t":{"player":value}}),
                serde_json::json!({"k":"stringlist.Synthetic_Boss.killed.0","t":{"KILLER":value}}),
                serde_json::json!({"k":format!("stringlist.{value}.killed.0"),"t":{"KILLER":"SynthAlpha"}}),
            ] {
                assert_eq!(parse(&message.to_string()).is_some(), length <= 256);
            }
        }
        for count in [128, 129] {
            let escaped = r"\u00e9".repeat(count);
            let message = format!(r#"{{"k":"s.dungeon_opened_by","t":{{"player":"{escaped}"}}}}"#);
            assert_eq!(parse(&message).is_some(), count == 128);
        }
    }
}

#[cfg(test)]
mod death_tests {
    use super::*;
    #[test]
    fn engine_and_scrubber_accept_the_same_oryx_grammar() {
        fn scrubber_accepts(text: &str) -> bool {
            fn string(body: &mut Vec<u8>, value: &str) {
                body.extend_from_slice(&(value.len() as u16).to_be_bytes());
                body.extend_from_slice(value.as_bytes());
            }
            let mut body = Vec::new();
            string(&mut body, "#Oryx the Mad God");
            body.extend_from_slice(&7i32.to_be_bytes());
            body.extend_from_slice(&5i16.to_be_bytes());
            body.push(3);
            string(&mut body, "");
            string(&mut body, text);
            string(&mut body, "");
            body.push(0);
            body.extend_from_slice(&0i32.to_be_bytes());
            maxed_protocol::privacy::inventory(44, maxed_types::Direction::ServerToClient, &body)
                .unwrap()
                .treatments
                .iter()
                .any(|field| field.name == "oryxAnnouncement")
        }
        let mut failures = Vec::new();
        for (text, accepted) in [
            (
                r#"{"k":"stringlist.Boss.killed.0","t":{"KILLER":"Synthetic"}}{"k":"stringlist.Boss.killed.0","t":{"KILLER":"\u0053ynthetic"}}"#,
                false,
            ),
            (
                r#"{"k":"stringlist.Boss.killed.0","t":{"KILLER":"\u0053ynthetic"}}{"k":"stringlist.Boss.killed.0","t":{"KILLER":"\u0053ynthetic"}}"#,
                true,
            ),
            (r#"{"k":"stringlist.Boss.death.0"}"#, true),
            (r#"{"k":"stringlist.The_Boss.death.0",}"#, true),
            (
                r#"{"k":"stringlist.Boss.killed.0","t":{"KILLER":"Synthetic",},}"#,
                true,
            ),
            (
                r#"{"t":{"KILLER":"Synthetic\\\" ,}"},"k":"stringlist.Boss.killed.0"}"#,
                true,
            ),
            (
                r#"{"k":"stringlist.Boss.killed.0","t":{"KILLER":"\u0053ynthetic"}}"#,
                true,
            ),
            (
                r#"{"k":"stringlist.Boss.death.0"}{"k":"stringlist.Boss.death.0",}"#,
                true,
            ),
            (
                r#"{"k":"stringlist.Boss.death.0"}{"k":"stringlist.Boss.death.1"}"#,
                false,
            ),
            (r#"{"k":"stringlist.Boss.death.0","t":{}}"#, false),
            (r#"{"k":"stringlist.\u0042oss.death.0"}"#, false),
            (r#"{"k":"stringlist.Boss.killed.0"}"#, false),
            (
                r#"{"k":"stringlist.Boss.death.0","t":{"KILLER":"Synthetic"}}"#,
                false,
            ),
            (
                r#"{"k":"stringlist.Boss.killed.0","t":{"KILLER":"Synthetic","extra":"x"}}"#,
                false,
            ),
            (r#"{"k":"stringlist.Boss.death.0","extra":"x"}"#, false),
            (
                r#"{"k":"stringlist.Boss.death.0","k":"stringlist.Boss.death.0"}"#,
                false,
            ),
            (r#"{"k":"stringlist.Boss.death.x"}"#, false),
            (r#"{"k":"stringlist..death.0"}"#, false),
            (r#"{"k":"stringlist.Boss.death.0"}junk"#, false),
        ] {
            let engine = parse(text).is_some();
            let scrubber = scrubber_accepts(text);
            if engine != accepted || scrubber != accepted {
                failures.push((text, accepted, engine, scrubber));
            }
        }
        assert!(
            failures.is_empty(),
            "(input, expected, engine, scrubber): {failures:?}"
        );
    }
    #[test]
    fn death_keys_and_identical_concatenated_objects() {
        for key in ["Goblin_Patriarch", "The_Plague_Doctor", "LH_Lost_Sentry"] {
            let text = format!(r#"{{"k":"stringlist.{key}.death.0"}}"#);
            for value in [text.clone(), format!("{text}{text}")] {
                assert!(
                    matches!(parse(&value), Some(Notice::RealmEventKilled { boss_key, .. }) if boss_key == key)
                );
            }
        }
    }
    #[test]
    fn duplicated_killed_objects_and_death_limits() {
        let text = r#"{"k":"stringlist.Hermit_God.killed.2","t":{"KILLER":"SyntheticKiller"}}"#;
        assert_eq!(parse(&format!("{text}{text}")), parse(text));
        assert!(parse(&format!("{text}{text}{text}")).is_none());
        for length in [256, 257] {
            let key = "X".repeat(length);
            assert_eq!(
                parse(&format!(r#"{{"k":"stringlist.{key}.death.0"}}"#)).is_some(),
                length == 256
            );
        }
    }
    #[test]
    fn malformed_death_announcements_are_rejected() {
        for text in [
            r#"{"k":"stringlist..death.0"}"#,
            r#"{"k":"stringlist.The_Boss.death.x"}"#,
            r#"{"k":"stringlist.The_Boss.death."}"#,
            r#"{"k":"stringlist.The.Boss.death.0"}"#,
            r#"{"k":"stringlist.The_Boss.death.0","extra":"x"}"#,
            r#"{"k":"stringlist.The_Boss.death.0","t":{"player":"Synthetic"}}"#,
            r#"{"k":"stringlist.The_Boss.death.0"}{"k":"stringlist.Other.death.0"}"#,
            r#"{"k":"stringlist.The_Boss.death.0"}junk"#,
            r#"{"k":"stringlist.The_Boss.killed.0"}"#,
        ] {
            assert_eq!(parse(text), None);
        }
    }
}

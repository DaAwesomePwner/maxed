//! Checked big-endian field reading, ported from RealmShark (see NOTICE.md).
use crate::ProtocolError;

pub struct Reader<'a> {
    input: &'a [u8],
    offset: usize,
    privacy: Option<crate::privacy::Inventory>,
    field_name: &'static str,
    objects: Option<&'a crate::privacy::ObjectContext>,
    object_id: Option<i32>,
    object_type: Option<u16>,
}

impl<'a> Reader<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            offset: 0,
            privacy: None,
            field_name: "field",
            objects: None,
            object_id: None,
            object_type: None,
        }
    }

    pub(crate) fn for_privacy(
        input: &'a [u8],
        objects: Option<&'a crate::privacy::ObjectContext>,
    ) -> Self {
        Self {
            input,
            offset: 0,
            privacy: Some(Default::default()),
            field_name: "field",
            objects,
            object_id: None,
            object_type: None,
        }
    }

    pub(crate) fn finish_privacy(self) -> Result<crate::privacy::Inventory, ProtocolError> {
        let inventory = self.privacy.ok_or(ProtocolError::Definition)?;
        if self.offset != self.input.len() {
            return Err(ProtocolError::Definition);
        }
        Ok(inventory)
    }

    pub(crate) fn evidence(
        &mut self,
        field: &'static str,
        stat_id: Option<u8>,
        reason: crate::privacy::EvidenceReason,
    ) {
        if let Some(inventory) = &mut self.privacy {
            inventory.evidence.push(crate::privacy::FieldEvidence {
                field,
                stat_id,
                reason,
            });
        }
    }

    pub(crate) fn object_type(&mut self, object_type: Option<u16>) {
        self.object_type = object_type;
    }
    pub(crate) fn object_id(&mut self, object_id: i32) {
        self.object_id = Some(object_id);
    }

    pub(crate) fn stat_string(
        &mut self,
        id: u8,
        policy: Option<(&'static str, crate::privacy::FieldPolicy)>,
    ) -> Result<String, ProtocolError> {
        use crate::privacy::{EvidenceReason, FieldPolicy, SecretKind};
        let (name, mut policy) =
            policy.unwrap_or(("stringStatValue", FieldPolicy::FillUnclassified));
        let mut fill_reason = EvidenceReason::UnclassifiedStringStat;
        if id == 31 {
            let (name_policy, reason) = self.object_id.map_or(
                (
                    FieldPolicy::FillUnclassified,
                    Some(EvidenceReason::UnknownObjectName),
                ),
                |id| {
                    self.objects
                        .unwrap_or(&crate::privacy::ObjectContext::default())
                        .name_policy(id, self.object_type)
                },
            );
            policy = name_policy;
            if let Some(reason) = reason {
                fill_reason = reason;
            }
        }
        self.field_name(name);
        match policy {
            FieldPolicy::Keep => self.public_string(),
            FieldPolicy::Pseudonym => self.secret_string(SecretKind::Identity),
            FieldPolicy::Fill => self.secret_string(SecretKind::Credential),
            FieldPolicy::FillUnclassified => {
                self.evidence("stringStatValue", Some(id), fill_reason);
                self.string()
            }
        }
    }

    fn field(
        &mut self,
        kind: Option<crate::privacy::SecretKind>,
        binary: bool,
    ) -> Result<&'a [u8], ProtocolError> {
        let len = usize::from(self.u16()?);
        let offset = self.position();
        let value = self.take(len)?;
        if let Some(inventory) = &mut self.privacy {
            inventory.fields.push((offset, len));
            let kind = kind.unwrap_or(crate::privacy::SecretKind::Unclassified);
            inventory
                .spans
                .push(crate::privacy::SensitiveSpan { offset, len, kind });
            if binary {
                inventory.binary.push(offset);
            }
            use crate::privacy::{FieldPolicy, FieldTreatment, SecretKind};
            inventory.treatments.push(FieldTreatment {
                offset,
                len,
                name: self.field_name,
                public_literal: false,
                policy: match kind {
                    SecretKind::Identity => FieldPolicy::Pseudonym,
                    SecretKind::Unclassified => FieldPolicy::FillUnclassified,
                    _ => FieldPolicy::Fill,
                },
            });
        }
        Ok(value)
    }

    pub(crate) fn secret_string(
        &mut self,
        kind: crate::privacy::SecretKind,
    ) -> Result<String, ProtocolError> {
        Ok(java_utf8_lossy(self.field(Some(kind), false)?))
    }

    pub(crate) fn secret_bytes(&mut self) -> Result<Vec<u8>, ProtocolError> {
        Ok(self
            .field(Some(crate::privacy::SecretKind::Credential), true)?
            .to_vec())
    }

    pub(crate) fn field_name(&mut self, name: &'static str) {
        self.field_name = name;
    }

    pub(crate) fn public_if(
        &mut self,
        allowed: impl FnOnce(&str) -> bool,
        literal: bool,
    ) -> Result<String, ProtocolError> {
        let value = self.string()?;
        // The predicate only selects a privacy treatment; plain decoding skips it.
        if self.privacy.is_some() && allowed(&value) {
            if let Some(inventory) = &mut self.privacy {
                let field = inventory
                    .treatments
                    .last_mut()
                    .ok_or(ProtocolError::Definition)?;
                if std::str::from_utf8(&self.input[field.offset..field.offset + field.len]).is_ok()
                {
                    field.policy = crate::privacy::FieldPolicy::Keep;
                    field.public_literal = literal;
                    inventory.spans.pop();
                }
            }
        }
        Ok(value)
    }

    pub(crate) fn public_string(&mut self) -> Result<String, ProtocolError> {
        self.public_if(|_| true, false)
    }

    pub(crate) fn public_literal(&mut self, allowed: &[&str]) -> Result<String, ProtocolError> {
        self.public_if(|s| allowed.contains(&s), true)
    }

    pub(crate) fn npc_text(&mut self, name: &str) -> Result<String, ProtocolError> {
        let value = self.public_if(|s| crate::privacy::NPC_DIALOGUE.contains(&(name, s)), true)?;
        if name != "#Oryx the Mad God" || self.privacy.is_none() {
            return Ok(value);
        }
        self.oryx_text_spans(&value)?;
        Ok(value)
    }

    pub(crate) fn notice_text(&mut self, death: bool) -> Result<String, ProtocolError> {
        use crate::privacy::{FieldPolicy, FieldTreatment, SecretKind, SensitiveSpan};
        let value = self.string()?;
        let spans = if death {
            crate::notices::parse_death_announcement(&value).map(|notice| {
                vec![
                    (notice.player_span, "deathPlayer", FieldPolicy::Pseudonym),
                    (notice.enemy_span, "deathEnemy", FieldPolicy::Keep),
                    (notice.level_span, "deathLevel", FieldPolicy::Keep),
                ]
            })
        } else {
            crate::notices::parse_portal_announcement(&value)
                .map(|notice| vec![(notice.player_span, "portalPlayer", FieldPolicy::Pseudonym)])
        };
        if let (Some(mut spans), Some(inventory)) = (spans, &mut self.privacy) {
            let field = inventory
                .treatments
                .last()
                .ok_or(ProtocolError::Definition)?;
            if std::str::from_utf8(&self.input[field.offset..field.offset + field.len]).is_err() {
                return Ok(value);
            }
            let field = inventory
                .treatments
                .pop()
                .ok_or(ProtocolError::Definition)?;
            inventory.spans.pop();
            spans.sort_by_key(|(span, _, _)| span.start);
            let mut cursor = 0;
            let mut ranges = Vec::new();
            let announcement = if death {
                "deathAnnouncement"
            } else {
                "portalAnnouncement"
            };
            for (span, name, policy) in spans {
                ranges.push((cursor..span.start, announcement, FieldPolicy::Keep));
                ranges.push((span.clone(), name, policy));
                cursor = span.end;
            }
            ranges.push((cursor..field.len, announcement, FieldPolicy::Keep));
            for (range, name, policy) in ranges {
                if policy == FieldPolicy::Pseudonym {
                    inventory.spans.push(SensitiveSpan {
                        offset: field.offset + range.start,
                        len: range.len(),
                        kind: SecretKind::Identity,
                    });
                }
                inventory.treatments.push(FieldTreatment {
                    offset: field.offset + range.start,
                    len: range.len(),
                    name,
                    policy,
                    // Game-data/chat coincidences are informational. Identity
                    // and credential collisions still fail verification.
                    public_literal: matches!(name, "deathEnemy" | "deathLevel"),
                });
            }
        }
        Ok(value)
    }

    fn oryx_text_spans(&mut self, value: &str) -> Result<(), ProtocolError> {
        use crate::privacy::{FieldPolicy, FieldTreatment, SecretKind, SensitiveSpan};
        let Some(spans) = crate::privacy::oryx_killer_spans(value) else {
            return Ok(());
        };
        if let Some(inventory) = &mut self.privacy {
            let field = inventory
                .treatments
                .pop()
                .ok_or(ProtocolError::Definition)?;
            if std::str::from_utf8(&self.input[field.offset..field.offset + field.len]).is_err() {
                inventory.treatments.push(field);
                return Ok(());
            }
            if field.policy != FieldPolicy::Keep {
                inventory.spans.pop();
            }
            let mut cursor = 0;
            for span in spans {
                inventory.treatments.push(FieldTreatment {
                    offset: field.offset + cursor,
                    len: span.start - cursor,
                    name: "oryxAnnouncement",
                    policy: FieldPolicy::Keep,
                    public_literal: false,
                });
                inventory.spans.push(SensitiveSpan {
                    offset: field.offset + span.start,
                    len: span.len(),
                    kind: SecretKind::PlayerText,
                });
                inventory.treatments.push(FieldTreatment {
                    offset: field.offset + span.start,
                    len: span.len(),
                    name: "KILLER",
                    policy: FieldPolicy::Fill,
                    public_literal: false,
                });
                cursor = span.end;
            }
            inventory.treatments.push(FieldTreatment {
                offset: field.offset + cursor,
                len: field.len - cursor,
                name: "oryxAnnouncement",
                policy: FieldPolicy::Keep,
                public_literal: false,
            });
        }
        Ok(())
    }

    pub(crate) fn text_sender(&mut self) -> Result<String, ProtocolError> {
        let value = self.public_if(|s| s.starts_with('#'), false)?;
        if !value.starts_with('#') {
            if let Some(inventory) = &mut self.privacy {
                inventory
                    .spans
                    .last_mut()
                    .ok_or(ProtocolError::Definition)?
                    .kind = crate::privacy::SecretKind::Identity;
                inventory
                    .treatments
                    .last_mut()
                    .ok_or(ProtocolError::Definition)?
                    .policy = crate::privacy::FieldPolicy::Pseudonym;
            }
        }
        Ok(value)
    }

    pub fn position(&self) -> usize {
        self.offset
    }

    pub fn remaining(&self) -> usize {
        self.input.len() - self.offset
    }

    pub fn take(&mut self, len: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self.offset.checked_add(len).ok_or(ProtocolError::Length)?;
        let bytes = self
            .input
            .get(self.offset..end)
            .ok_or(ProtocolError::Truncated)?;
        self.offset = end;
        Ok(bytes)
    }

    pub fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    pub fn i8(&mut self) -> Result<i8, ProtocolError> {
        Ok(self.u8()? as i8)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ProtocolError::Truncated)
    }

    pub fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub fn i16(&mut self) -> Result<i16, ProtocolError> {
        Ok(i16::from_be_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32, ProtocolError> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32, ProtocolError> {
        Ok(f32::from_be_bytes(self.array()?))
    }

    pub fn bool(&mut self) -> Result<bool, ProtocolError> {
        Ok(self.u8()? != 0)
    }

    /// Replace malformed sequences as required by the canonical parity contract.
    pub fn string(&mut self) -> Result<String, ProtocolError> {
        Ok(java_utf8_lossy(self.field(None, false)?))
    }

    pub fn bytes(&mut self) -> Result<Vec<u8>, ProtocolError> {
        Ok(self.field(None, true)?.to_vec())
    }

    pub fn list<T>(
        &mut self,
        count: i32,
        min_bytes: usize,
        mut read: impl FnMut(&mut Self) -> Result<T, ProtocolError>,
    ) -> Result<Vec<T>, ProtocolError> {
        let count = usize::try_from(count).map_err(|_| ProtocolError::Length)?;
        if min_bytes == 0 || count > self.remaining() / min_bytes {
            return Err(ProtocolError::Length);
        }
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| ProtocolError::Length)?;
        for _ in 0..count {
            result.push(read(self)?);
        }
        Ok(result)
    }

    pub fn ci(&mut self) -> Result<i32, ProtocolError> {
        let mut byte = self.u8()?;
        let negative = byte & 64 != 0;
        let mut value = u64::from(byte & 63);
        let mut shift = 6;
        while byte & 128 != 0 {
            if shift > 27 {
                return Err(ProtocolError::CompressedOverflow);
            }
            byte = self.u8()?;
            value |= u64::from(byte & 127) << shift;
            shift += 7;
        }
        let limit = if negative {
            2_147_483_648
        } else {
            2_147_483_647
        };
        if value > limit {
            return Err(ProtocolError::CompressedOverflow);
        }
        Ok(if negative {
            -(value as i64) as i32
        } else {
            value as i32
        })
    }
}

/// JDK malformed-input lengths agree with Rust except for UTF-8 encodings of
/// surrogate code points. Java consumes their otherwise-valid prefix together:
/// three bytes for a complete surrogate, two for a truncated/invalid third byte.
/// See OpenJDK 17 UTF_8.Decoder's malformedN and surrogate checks.
fn java_utf8_lossy(mut bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len());
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(valid) => {
                output.push_str(valid);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                // This prefix is certified valid; no replacement occurs here.
                output.push_str(&String::from_utf8_lossy(&bytes[..valid]));
                bytes = &bytes[valid..];
                let len = if bytes.first() == Some(&0xed)
                    && bytes.get(1).is_some_and(|b| (0xa0..=0xbf).contains(b))
                {
                    if bytes.get(2).is_some_and(|b| (0x80..=0xbf).contains(b)) {
                        3
                    } else {
                        2
                    }
                } else {
                    error.error_len().unwrap_or(bytes.len())
                };
                output.push('\u{fffd}');
                bytes = &bytes[len..];
            }
        }
    }
    output
}

#[cfg(test)]
mod privacy_tests {
    use super::*;
    use crate::privacy::{FieldPolicy, SecretKind};

    #[test]
    fn oryx_span_replacement_preserves_an_earlier_identity_after_public_if() {
        for public in [false, true] {
            let text = r#"{"k":"stringlist.Boss.killed.0","t":{"KILLER":"Synthetic"}}"#;
            let mut body = vec![0, 3, b'a', b'b', b'c'];
            body.extend_from_slice(&(text.len() as u16).to_be_bytes());
            body.extend_from_slice(text.as_bytes());
            let mut reader = Reader::for_privacy(&body, None);
            reader.secret_string(SecretKind::Identity).unwrap();
            let value = reader.public_if(|_| public, true).unwrap();
            reader.oryx_text_spans(&value).unwrap();
            let inventory = reader.finish_privacy().unwrap();
            assert_eq!(inventory.spans.len(), 2);
            assert_eq!(inventory.spans[0].kind, SecretKind::Identity);
            assert_eq!(inventory.spans[0].offset, 2);
            assert_eq!(inventory.spans[0].len, 3);
            assert_eq!(inventory.spans[1].kind, SecretKind::PlayerText);
        }
    }

    #[test]
    fn string_wire_kind_without_policy_still_has_a_fillable_extent() {
        let mut reader = Reader::for_privacy(&[0, 3, b'a', b'b', b'c'], None);
        assert_eq!(reader.stat_string(250, None).unwrap(), "abc");
        let inventory = reader.finish_privacy().unwrap();
        assert_eq!(inventory.spans[0].offset, 2);
        assert_eq!(inventory.spans[0].len, 3);
        assert_eq!(inventory.spans[0].kind, SecretKind::Unclassified);
        assert_eq!(inventory.evidence[0].stat_id, Some(250));
        assert_eq!(
            inventory.evidence[0].reason,
            crate::privacy::EvidenceReason::UnclassifiedStringStat
        );
        assert!(!inventory.evidence[0].reason.refuses());
    }

    #[test]
    fn ordinary_strings_and_blobs_get_named_same_length_filler_spans() {
        let body = [0, 3, b'a', b'b', b'c', 0, 2, 255, 0];
        let mut reader = Reader::for_privacy(&body, None);
        reader.field_name("unclassifiedString");
        assert_eq!(reader.string().unwrap(), "abc");
        reader.field_name("unclassifiedBlob");
        assert_eq!(reader.bytes().unwrap(), [255, 0]);
        let inventory = reader.finish_privacy().unwrap();
        assert_eq!(inventory.fields, [(2, 3), (7, 2)]);
        assert_eq!(inventory.binary, [7]);
        assert!(inventory
            .spans
            .iter()
            .all(|s| s.kind == SecretKind::Unclassified));
        assert!(inventory
            .treatments
            .iter()
            .all(|s| s.policy == FieldPolicy::FillUnclassified));
        assert_eq!(inventory.treatments[0].name, "unclassifiedString");
        assert_eq!(inventory.treatments[1].name, "unclassifiedBlob");
    }
}

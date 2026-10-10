use maxed_types::{EnchantState, EnchantView};

fn unknown(state: EnchantState) -> EnchantView {
    EnchantView {
        state,
        slots: None,
        applied: None,
        ids: vec![],
    }
}

/// Strict per-slot evidence, retaining unknown IDs and empty/locked entries.
/// RealmShark ParseEnchants.evidence / summarizeLegacy.
pub fn parse_enchant(value: Option<&str>) -> EnchantView {
    let Some(value) = value else {
        return unknown(EnchantState::Missing);
    };
    let mut result = EnchantView {
        state: EnchantState::RecordedEmpty,
        slots: Some(0),
        applied: Some(0),
        ids: vec![],
    };
    if value.is_empty() {
        return result;
    }
    let Some(bytes) = crate::encoding::decode(value, 4096) else {
        return unknown(EnchantState::Invalid);
    };
    let size = bytes.len().min(11);
    if !bytes.starts_with(&[0, 2, 4]) || (size - 3) % 2 != 0 {
        return unknown(EnchantState::Invalid);
    }
    let mut slots = 0;
    let mut applied = 0;
    for b in bytes[3..size].as_chunks::<2>().0 {
        let id = i32::from(i16::from_le_bytes([b[0], b[1]]));
        if id < -3 {
            return unknown(EnchantState::Invalid);
        }
        result.ids.push(id);
        if id == -3 {
            break;
        }
        if id != -2 {
            slots += 1;
        }
        if id >= 0 {
            applied += 1;
        }
    }
    result.slots = Some(slots);
    result.applied = Some(applied);
    if applied > 0 {
        result.state = EnchantState::Recorded;
    }
    result
}

pub(crate) fn slots(value: Option<&str>, count: usize) -> Vec<EnchantView> {
    match value {
        None => vec![parse_enchant(None); count],
        Some("") => vec![parse_enchant(Some("")); count],
        Some(v) => {
            let parts: Vec<_> = v.split(',').take(count).collect();
            (0..count)
                .map(|i| parse_enchant(parts.get(i).copied()))
                .collect()
        }
    }
}

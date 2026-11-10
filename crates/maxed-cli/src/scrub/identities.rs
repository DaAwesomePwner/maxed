use super::*;

fn field_named(body: &Body, offset: usize, name: &str) -> bool {
    body.inventory
        .treatments
        .iter()
        .any(|f| f.offset == offset && f.name == name)
}

fn player_field(body: &Body, offset: usize) -> bool {
    field_named(body, offset, "NAME_STAT") || json_identity(body, offset)
}

pub(super) fn display(value: &[u8]) -> &[u8] {
    value.split(|b| *b == b',').next().unwrap_or_default()
}

pub(super) fn redact(bodies: &mut [Body]) -> Result<u64, ScrubFailure> {
    let mut originals = BTreeSet::new();
    for body in bodies.iter() {
        for s in &body.inventory.spans {
            if s.kind == SecretKind::Identity {
                let identity = identity_value(body, s.offset, s.len)?;
                originals.insert(if player_field(body, s.offset) {
                    display(&identity).to_vec()
                } else {
                    identity
                });
            }
        }
    }
    // Decoded notice identities join the same sorted, length-preserving namespace
    // as NAME_STAT, even if no player object (or only a later one) was observed.
    let mut names = aliases(originals)?;
    // Empty display names are valid engine keys, including ",suffix".
    names.insert(Vec::new(), Vec::new());
    let mut player_aliases = BTreeMap::<Vec<u8>, BTreeSet<Vec<u8>>>::new();
    for body in bodies.iter() {
        for field in body
            .inventory
            .treatments
            .iter()
            .filter(|field| field.name == "NAME_STAT" && field.policy == FieldPolicy::Pseudonym)
        {
            let original = display(&body.plain[field.offset..field.offset + field.len]);
            if let Some(alias) = names.get(original) {
                player_aliases
                    .entry(original.to_ascii_lowercase())
                    .or_default()
                    .insert(alias.clone());
            }
        }
    }
    // A shared lowercase key does not imply exact death-name equality.
    // Merging case variants would invent matches (or remove ambiguity).
    // Refuse rather than encode source casing or split the engine's name key.
    if player_aliases.values().any(|aliases| aliases.len() != 1) {
        return refuse(ScrubReason::VerificationFailed);
    }
    let mut changed = 0;
    for body in bodies {
        if body.treatment != Treatment::Fields {
            let prefix = alignment_prefix(body);
            body.expected[prefix..].fill(0xff);
            changed += u64::from(prefix < body.plain.len());
            continue;
        }
        for s in &body.inventory.spans {
            if s.len == 0 {
                continue;
            }
            let replacement = match s.kind {
                SecretKind::Identity => {
                    let identity = identity_value(body, s.offset, s.len)?;
                    let key = if player_field(body, s.offset) {
                        display(&identity)
                    } else {
                        &identity
                    };
                    let alias = if json_identity(body, s.offset) {
                        match player_aliases.get(&key.to_ascii_lowercase()) {
                            // Death matching is exact after suffix removal. Use the
                            // independent alias for a case-only mismatch.
                            Some(_) if field_named(body, s.offset, "deathPlayer") => names.get(key),
                            Some(aliases) => aliases.first(),
                            None => names.get(key),
                        }
                    } else {
                        names.get(key)
                    }
                    .ok_or(maxed_protocol::ProtocolError::Definition)?;
                    let mut replacement = alias.clone();
                    if player_field(body, s.offset) {
                        // The engine discards all suffix text. Preserve only the
                        // comma layout, treating every suffix component as private.
                        replacement.extend(identity[key.len()..].iter().map(|b| {
                            if *b == b',' {
                                b','
                            } else {
                                b'x'
                            }
                        }));
                    }
                    if json_identity(body, s.offset) {
                        let mut encoded = serde_json::to_vec(
                            std::str::from_utf8(&replacement)
                                .map_err(|_| maxed_protocol::ProtocolError::Definition)?,
                        )
                        .map_err(|_| maxed_protocol::ProtocolError::Definition)?;
                        if encoded.len() > s.len {
                            return refuse(ScrubReason::VerificationFailed);
                        }
                        encoded.resize(s.len, b' ');
                        encoded
                    } else {
                        replacement
                    }
                }
                _ if body.inventory.binary.contains(&s.offset) => {
                    vec![0; s.len]
                }
                _ => vec![b'x'; s.len],
            };
            // Already-filler input is safe: verification checks replacement provenance.
            body.expected[s.offset..s.offset + s.len].copy_from_slice(&replacement);
            changed += 1;
        }
    }
    Ok(changed)
}

//! Session-independent configuration learned from CRUCIBLE_RESPONSE.
use crate::Engine;
use maxed_types::CrucibleDef;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_DEFINITIONS: usize = 4096;
const MAX_JSON_BYTES: usize = 65_535;
const MAX_DEPTH: usize = 32;

#[derive(Default)]
pub(crate) struct Definitions {
    values: BTreeMap<String, f64>,
    dirty: BTreeSet<String>,
}

impl Definitions {
    fn insert(&mut self, def: CrucibleDef, learned: bool) {
        if def.id.len() > MAX_JSON_BYTES
            || !def.damage_multiplier.is_finite()
            || def.damage_multiplier <= 0.
            || def.damage_multiplier > 100.
            || (self.values.len() >= MAX_DEFINITIONS && !self.values.contains_key(&def.id))
        {
            return;
        }
        if self.values.get(&def.id) == Some(&def.damage_multiplier) {
            return;
        }
        if learned {
            self.dirty.insert(def.id.clone());
        }
        self.values.insert(def.id, def.damage_multiplier);
    }

    pub(crate) fn learn(&mut self, jsons: &[String]) {
        // Match the protocol's count/string bounds even for synthetic callers.
        for json in jsons.iter().take(64) {
            if json.len() > MAX_JSON_BYTES {
                continue;
            }
            // serde_json also retains its default parser recursion limit.
            let Ok(value) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            let Some(entries) = value.get("array").and_then(Value::as_array) else {
                continue;
            };
            for entry in entries.iter().take(MAX_DEFINITIONS) {
                let Some(id) = entry.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if let Ok(amount) = first_bonus(entry, 0) {
                    self.insert(
                        CrucibleDef {
                            id: id.into(),
                            damage_multiplier: amount.unwrap_or(1.),
                        },
                        true,
                    );
                }
            }
        }
    }
}

// Search bonuses in array order. An invalid first type-5 bonus rejects the
// entry, rather than silently treating it as neutral or using a later bonus.
fn first_bonus(value: &Value, depth: usize) -> Result<Option<f64>, ()> {
    if depth > MAX_DEPTH {
        return Err(());
    }
    match value {
        Value::Object(object) => {
            if let Some(bonuses) = object.get("bonuses") {
                let bonuses = bonuses.as_array().ok_or(())?;
                for bonus in bonuses {
                    if bonus.get("type").and_then(Value::as_i64) == Some(5) {
                        return bonus
                            .get("amount")
                            .and_then(Value::as_f64)
                            .map(Some)
                            .ok_or(());
                    }
                }
            }
            for (key, child) in object {
                if key != "bonuses" {
                    if let Some(amount) = first_bonus(child, depth + 1)? {
                        return Ok(Some(amount));
                    }
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                if let Some(amount) = first_bonus(child, depth + 1)? {
                    return Ok(Some(amount));
                }
            }
        }
        _ => {}
    }
    Ok(None)
}

impl Engine {
    /// Seed persisted definitions without reporting them as newly learned.
    /// Invalid values and new IDs beyond the 4096-entry capacity are ignored.
    /// Intended for construction, before applying packets.
    pub fn seed_crucible_definitions(
        &mut self,
        definitions: impl IntoIterator<Item = CrucibleDef>,
    ) {
        for definition in definitions {
            self.crucible_definitions.insert(definition, false);
        }
    }

    /// Drain new or updated packet definitions, coalesced and sorted by ID.
    pub fn take_crucible_definitions(&mut self) -> Vec<CrucibleDef> {
        std::mem::take(&mut self.crucible_definitions.dirty)
            .into_iter()
            .map(|id| CrucibleDef {
                damage_multiplier: self.crucible_definitions.values[&id],
                id,
            })
            .collect()
    }

    pub(crate) fn crucible_multiplier(&self, id: Option<&str>) -> Option<f64> {
        let id = id?;
        if id.is_empty() {
            return Some(1.);
        }
        self.catalog
            .crucible_bonuses
            .iter()
            .find(|definition| definition.id == id)
            .map(|definition| definition.damage_multiplier)
            .or_else(|| self.crucible_definitions.values.get(id).copied())
    }
}

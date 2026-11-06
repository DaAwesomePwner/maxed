use crate::{identity, parse_enchant, world::Entity, Engine};
use maxed_types::*;

const STAT_BOOST_PAIRS: [(u8, u8); 8] = [
    (0, 46),
    (3, 47),
    (20, 48),
    (21, 49),
    (22, 50),
    (28, 53),
    (26, 51),
    (27, 52),
];

fn at_cap(class: &ClassDef, stat: &impl Fn(u8) -> Option<i32>, id: u8, boost: u8) -> Option<bool> {
    let cap = class.stat_caps.iter().find(|v| v.stat_id == id)?.value;
    Some(i64::from(stat(id)?) - i64::from(stat(boost)?) >= i64::from(cap))
}

pub const FLAWLESS_UNSUPPORTED: &str =
    "Flawless is unavailable: no approved captured-evidence definition exists.";

/// Personal-best alerts also require a prior baseline; this checks engine prerequisites.
pub fn personal_best_eligible(fight: &FightView, character: &CharacterView) -> bool {
    fight.dps_basis == DpsBasis::PlayerActiveWindowV1
        && fight.complete_capture
        && !fight.pending_local_reconciliation
        && fight.issues.is_empty()
        && fight.boss_type.is_some()
        && character.identity_verified
        && fight.players.iter().any(|p| {
            p.is_local
                && p.player_id == character.id
                && p.damage > 0.
                && p.sources.iter().all(|s| !s.estimated)
        })
}

pub(crate) fn equipment(entity: Option<&Entity>) -> Vec<EquipmentView> {
    (0..4)
        .map(|slot| EquipmentView {
            observed_empty: entity.and_then(|e| e.stat(8 + slot)).and_then(|value| {
                if value == -1 {
                    Some(true)
                } else {
                    u16::try_from(value).ok().map(|_| false)
                }
            }),
            slot,
            item_type: entity
                .and_then(|e| e.stat(8 + slot))
                .and_then(|i| u16::try_from(i).ok()),
            enchant: entity
                .and_then(|e| e.enchants.get(usize::from(slot)))
                .cloned()
                .unwrap_or_else(|| parse_enchant(None)),
        })
        .collect()
}

impl Engine {
    pub(crate) fn maxed(&self, class: Option<u16>, stat: impl Fn(u8) -> Option<i32>) -> Option<u8> {
        let class = self
            .catalog
            .classes
            .iter()
            .find(|c| Some(c.object_type) == class)?;
        // Entity.calculateBaseStats: base = observed value - observed boost.
        // Missing boosts/caps never imply zero boosts or an 8/8 character.
        let mut count = 0;
        for (s, b) in STAT_BOOST_PAIRS {
            if at_cap(class, &stat, s, b)? {
                count += 1;
            }
        }
        Some(count)
    }

    pub(crate) fn maxed_stats(
        &self,
        class: Option<u16>,
        stat: impl Fn(u8) -> Option<i32>,
    ) -> Vec<u8> {
        let Some(class) = self
            .catalog
            .classes
            .iter()
            .find(|c| Some(c.object_type) == class)
        else {
            return vec![];
        };
        STAT_BOOST_PAIRS
            .into_iter()
            .filter_map(|(id, boost)| (at_cap(class, &stat, id, boost) == Some(true)).then_some(id))
            .collect()
    }

    pub(crate) fn refresh_character(&mut self, events: &mut Vec<EngineEvent>) {
        let Some(local) = self.local.as_ref() else {
            return;
        };
        let previous = self.character.clone();
        let entity = self.entities.get(&local.object);
        let materialized = entity.is_some_and(|e| e.present);
        let verified = entity
            .and_then(|e| e.account)
            .map(|a| identity::character(&self.config.identity_key, &a, local.number));
        let id = verified
            .clone()
            .unwrap_or_else(|| local.provisional.clone());
        let equipment = equipment(entity);
        let class_id = entity.and_then(|e| e.object_type);
        let stat_panel = entity.map(|e| self.character_panel(e));
        // Local character and player profile use the shared placement gate.
        // Remote-player evaluation keeps its existing observation semantics.
        let maxed = stat_panel.as_ref().and_then(|panel| {
            panel.lines.iter().try_fold(0u8, |count, line| {
                line.at_cap.map(|at_cap| count + u8::from(at_cap))
            })
        });
        let mut issues = vec![];
        if verified.is_none() {
            issues.push(CoverageIssue::MissingIdentity);
        }
        if class_id.is_none() || maxed.is_none() {
            issues.push(CoverageIssue::MissingStats);
        }
        let mut next = CharacterView {
            calculation_revision: None,
            stat_panel,
            id: id.clone(),
            // Display only: identity still uses the account-stat HMAC and character number.
            name: entity
                .and_then(|e| e.name.as_deref())
                .map(|name| crate::names::display_name(name).to_owned()),
            class_id,
            level: entity
                .and_then(|e| e.stat(7))
                .and_then(|v| u8::try_from(v).ok()),
            maxed,
            fame: entity.and_then(|e| e.stat(57)),
            stats: entity
                .map(|e| {
                    e.stats
                        .iter()
                        .filter(|(k, _)| !matches!(k, 8..=19))
                        .map(|(&stat_id, &value)| StatView { stat_id, value })
                        .collect()
                })
                .unwrap_or_default(),
            equipment,
            observed_at: crate::clock::millis(self.now),
            identity_verified: verified.is_some(),
            died_at: previous
                .as_ref()
                .filter(|p| p.id == id)
                .and_then(|p| p.died_at),
            issues,
        };
        let character_event_index = events.len();
        if let Some(p) = &previous {
            if !p.identity_verified && verified.is_some() {
                events.push(EngineEvent::CharacterMerged {
                    provisional_id: p.id.clone(),
                    verified_id: id.clone(),
                });
                self.merge_player_identity(&p.id, &id);
                if let Some(fame) = self.last_fame.remove(&p.id) {
                    self.last_fame.entry(id.clone()).or_insert(fame);
                }
            }
            let mut compare = next.clone();
            compare.observed_at = p.observed_at;
            if &compare == p {
                next.observed_at = p.observed_at;
            }
        }
        if previous.as_ref() != Some(&next) {
            // AppHistory.fame owns per-character change detection; D-fame uses observed stat 57.
            if let Some(fame) = next.fame {
                if self.last_fame.get(&id) != Some(&fame) {
                    self.last_fame.insert(id.clone(), fame);
                    events.push(EngineEvent::FameChanged {
                        at: crate::clock::millis(self.now),
                        character_id: id.clone(),
                        fame,
                    });
                }
            }
            // Materialize the referenced identity before merge/fame events. Keep
            // the state transitions and change detection above unchanged.
            events.insert(
                character_event_index,
                EngineEvent::CharacterUpdated(next.clone()),
            );
            self.character = Some(next);
        }
        if self
            .local
            .as_ref()
            .is_none_or(|l| l.area != Some(self.map_ordinal))
        {
            return;
        }
        if self.area_kind == AreaKind::Realm && materialized {
            self.instance.observe_visit_fame(
                self.create_number,
                self.character.as_ref().and_then(|c| c.fame),
            );
        }
        if let Some(run) = self.run.as_mut().filter(|r| r.view.died_at.is_none()) {
            if run.view.character_id.as_ref() != Some(&id) {
                run.view.character_id = Some(id);
                events.push(EngineEvent::RunUpdated(run.view.clone()));
            }
        }
        self.accept_baseline(events);
    }
}

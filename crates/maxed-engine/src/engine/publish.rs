//! Convert bounded observations to public facts. No raw notification text survives parsing.
use super::{instance, notices, stat_panel};
use crate::{world::Entity, Engine};
use maxed_types::*;

impl From<&instance::AreaContext> for AreaContext {
    fn from(c: &instance::AreaContext) -> Self {
        Self {
            instance_cap: c.instance_cap,
            realm_name: c.realm_name.clone(),
            entered_from_realm: c.entered_from_realm,
            difficulty: c.difficulty,
            game_opened_epoch_s: c.game_opened_epoch_s,
            instance_age_micros: c.instance_age_micros,
            modifiers: c.modifiers.clone(),
            grade: c.grade.clone(),
        }
    }
}
impl Engine {
    pub(super) fn realm_event_object(&self, boss_key: &str) -> Option<&ObjectDef> {
        let name = boss_key.replace('_', " ");
        // XML IDs can differ from DisplayId (LH Lost Sentry, Big Barnacle).
        // Match the display-name family so an observed current-realm variant
        // wins over an unobserved legacy type without guessing ID prefixes.
        let alias_names: std::collections::BTreeSet<_> = self
            .catalog
            .objects
            .iter()
            .filter(|o| {
                o.event_boss == Some(true)
                    && o.realm_event_key
                        .as_deref()
                        .is_some_and(|key| key.eq_ignore_ascii_case(boss_key))
            })
            .map(|o| o.name.as_str())
            .collect();
        self.catalog
            .objects
            .iter()
            .filter(|o| {
                o.name.replace('_', " ").eq_ignore_ascii_case(&name)
                    || (o.event_boss == Some(true) && alias_names.contains(o.name.as_str()))
            })
            .min_by_key(|o| {
                (
                    !self.notices.observed_types.contains(&o.object_type),
                    o.event_boss != Some(true),
                    // A legacy alias may share the key but carry another
                    // display name; the exact display name wins when unobserved.
                    !o.name.replace('_', " ").eq_ignore_ascii_case(&name),
                    o.sprite_key
                        .as_deref()
                        .is_some_and(|s| s.starts_with("invisible")),
                    o.object_type,
                )
            })
    }
    pub(crate) fn target_kind(
        &self,
        key: crate::fights::FightKey,
        ty: Option<u16>,
    ) -> Option<TargetKind> {
        let o = ty
            .and_then(|t| self.object_index.get(&t))
            .map(|&i| &self.catalog.objects[i]);
        Some(if o.is_some_and(|o| o.is_boss) {
            TargetKind::Boss
        } else if o.is_some_and(|o| o.event_boss == Some(true)) {
            TargetKind::EventBoss
        } else if o.is_some_and(|o| o.is_miniboss) {
            TargetKind::Miniboss
        } else if self.is_quest(key) || self.fights.get(&key).is_some_and(|f| f.was_quest) {
            TargetKind::Quest
        } else if o.is_some() {
            TargetKind::Ordinary
        } else {
            return None;
        })
    }
    pub(crate) fn is_quest(&self, key: crate::fights::FightKey) -> bool {
        self.instance
            .quest
            .as_ref()
            .is_some_and(|q| q.object_id == key.0)
            && self.instance.quest_generation == Some(key.1)
    }
    pub(crate) fn refresh_targets(&mut self) {
        let quest = self.instance.quest.as_ref().and_then(|q| {
            self.instance
                .quest_generation
                .map(|generation| (q.object_id, generation))
        });
        if self.refreshed_quest != quest {
            self.target_dirty.extend(self.refreshed_quest);
            self.target_dirty.extend(quest);
            self.refreshed_quest = quest;
        }
        // New fights are classified at insertion. Only changed target types and
        // the old/new quest keys can change an existing classification.
        while let Some(key) = self.target_dirty.pop_first() {
            let Some(f) = self.fights.get(&key) else {
                continue;
            };
            #[cfg(test)]
            {
                self.target_refreshes += 1;
            }
            let kind = self.target_kind(key, f.base.target_type);
            let quest = self.is_quest(key);
            if let Some(f) = self.fights.get_mut(&key) {
                f.was_quest |= quest;
                if f.base.target_kind != kind || f.base.is_current_quest != quest {
                    f.changed();
                    f.base.target_kind = kind;
                    f.base.is_current_quest = quest;
                    self.dirty_fights.insert(key);
                }
            }
        }
    }
    pub(crate) fn character_panel(&self, e: &Entity) -> StatPanel {
        let caps = e
            .object_type
            .and_then(|ty| self.class_index.get(&ty).map(|&i| &self.catalog.classes[i]))
            .map(|c| c.stat_caps.iter().map(|s| (s.stat_id, s.value)).collect())
            .unwrap_or_default();
        let equipment: [Option<Vec<(u8, i32)>>; 4] = std::array::from_fn(|slot| {
            let ty = e.stat(8 + slot as u8)?;
            if ty == -1 {
                return Some(Vec::new());
            }
            self.item_index
                .get(&u16::try_from(ty).ok()?)
                .map(|&i| &self.catalog.items[i])?
                .stat_bonuses
                .as_ref()
                .map(|s| s.iter().map(|s| (s.stat_id, s.value)).collect())
        });
        let panel = stat_panel::panel(
            &e.stats,
            &caps,
            std::array::from_fn(|i| equipment[i].as_deref()),
        );
        StatPanel {
            lines: panel
                .lines
                .into_iter()
                .map(|s| StatLine {
                    split_evidence: s.split_evidence,
                    stat_id: s.stat_id,
                    total: s.total,
                    base: s.base,
                    cap: s.cap,
                    at_cap: s.at_cap,
                    boost: s.boost,
                    gear: s.gear,
                    exalt: s.exalt,
                    other: s.other,
                    current: s.current,
                })
                .collect(),
            conditions: panel
                .conditions
                .into_iter()
                .map(|c| ConditionView {
                    name: c.name.into(),
                    bit: c.bit,
                    secondary: c.secondary,
                    confirmed: c.confirmed,
                })
                .collect(),
            breakdown_provisional: panel.breakdown_provisional,
        }
    }
    fn emit_instance(
        &mut self,
        at: Timestamp,
        kind: InstanceEventKind,
        provisional: bool,
        events: &mut Vec<EngineEvent>,
    ) {
        if self.notices.emitted == notices::INSTANCE_EVENT_LIMIT {
            self.notices.dropped = self.notices.dropped.saturating_add(1);
            return;
        }
        self.notices.emitted += 1;
        events.push(EngineEvent::InstanceEvent(InstanceEventView {
            id: format!(
                "{}:instance:{}:{}",
                self.config.stream_id, self.map_ordinal, self.notices.emitted
            ),
            session_id: self.session_id(),
            run_id: self.run.as_ref().map(|r| r.view.id.clone()),
            map: self.map.clone(),
            at: crate::clock::millis(at),
            kind,
            provisional,
        }));
    }
    pub(crate) fn hold_departure(&mut self, id: i32) {
        if self.area_kind != AreaKind::Dungeon
            || !self.run.as_ref().is_some_and(|r| r.view.ended.is_none())
        {
            return;
        }
        let Some(e) = self.entities.get(&id).filter(|e| e.present) else {
            return;
        };
        let key = (id, e.generation);
        if self.notices.dead.contains(&key) {
            return;
        }
        if let Some(row) = self.player_row(id) {
            if self.notices.departures.len() < self.config.max_entities {
                self.notices
                    .departures
                    .entry(key)
                    .or_insert((self.now, row));
            }
        }
    }
    pub(crate) fn expire_departures(&mut self, events: &mut Vec<EngineEvent>) {
        if self.area_kind != AreaKind::Dungeon {
            self.notices.departures.clear();
            return;
        }
        // Live counts update at run end: out-of-view removals cannot be told
        // apart from departures without reliable positions. Re-entry and death
        // can cancel a pending departure throughout the run.
        if self.run.as_ref().is_some_and(|r| r.view.ended.is_some()) {
            let completed_at = self.run.as_ref().and_then(|r| r.view.completed_at);
            for (_, (at, row)) in std::mem::take(&mut self.notices.departures) {
                // Leaving with loot after the clear is not a run departure.
                if completed_at.is_some_and(|completed| crate::clock::millis(at) >= completed) {
                    continue;
                }
                self.emit_instance(
                    at,
                    InstanceEventKind::PlayerLeft {
                        object_id: row.object_id,
                        player_name: row.name,
                        class_id: row.class_id,
                    },
                    true,
                    events,
                );
            }
        }
    }
    pub(crate) fn publish_notices(&mut self, events: &mut Vec<EngineEvent>) {
        while self.notices.published < self.notices.entries.len() {
            let entry = &self.notices.entries[self.notices.published];
            let (notice, at, picture) = (entry.notice.clone(), entry.at, entry.picture_type);
            self.notices.published += 1;
            let (kind, provisional) = match notice {
                notices::Notice::PlayerDeath {
                    player,
                    level,
                    enemy,
                } => {
                    // Only a unique exact normalized name can establish an identity.
                    // Retained departure rows cover late notices throughout the run.
                    self.notices.dead.retain(|(id, generation)| {
                        self.entities
                            .get(id)
                            .is_some_and(|e| e.generation == *generation)
                    });
                    let mut candidates: std::collections::BTreeMap<_, _> = self
                        .entities
                        .iter()
                        .filter(|(id, e)| e.present && self.is_player(**id))
                        .filter_map(|(&id, e)| {
                            self.player_row(id)
                                .filter(|p| p.name.as_deref() == Some(&player))
                                .map(|p| ((id, e.generation), p))
                        })
                        .collect();
                    for (&key, (_, row)) in &self.notices.departures {
                        if row.name.as_deref() == Some(&player) {
                            candidates.entry(key).or_insert_with(|| row.clone());
                        }
                    }
                    let matched = if candidates.len() == 1 {
                        candidates.pop_first()
                    } else {
                        None
                    };
                    let row = matched.as_ref().map(|(_, p)| p);
                    let kind = InstanceEventKind::PlayerDeath {
                        player_id: row.map(|p| p.player_id.clone()),
                        player_name: player,
                        level,
                        killed_by: Some(enemy),
                        grave_type: picture,
                        object_id: row.map(|p| p.object_id),
                        class_id: row.and_then(|p| p.class_id),
                        is_local: row.is_some_and(|p| p.is_local),
                    };
                    if let Some((key, row)) = matched {
                        self.notices.departures.remove(&key);
                        let first = !self.notices.dead.contains(&key);
                        if self.notices.dead.len() < self.config.max_entities {
                            self.notices.dead.insert(key);
                        }
                        if first && !row.is_local {
                            for (key, f) in &mut self.fights {
                                if let Some(p) = f
                                    .base
                                    .players
                                    .iter_mut()
                                    .find(|p| p.player_id == row.player_id)
                                {
                                    p.deaths = p.deaths.saturating_add(1);
                                    f.changed();
                                    self.dirty_fights.insert(*key);
                                }
                            }
                        }
                    }
                    (kind, true)
                }
                notices::Notice::PortalOpened { player } => {
                    let portal_type = picture.and_then(|p| u16::try_from(p).ok());
                    let dungeon = portal_type
                        .and_then(|t| self.object_index.get(&t))
                        .and_then(|&i| self.catalog.objects[i].dungeon_name.clone());
                    (
                        InstanceEventKind::PortalOpened {
                            opener_is_local: self
                                .local
                                .as_ref()
                                .and_then(|local| self.entities.get(&local.object))
                                .and_then(|entity| entity.name.as_deref())
                                .map(crate::names::display_name)
                                .filter(|name| !name.is_empty() && !player.is_empty())
                                .map(|name| name.eq_ignore_ascii_case(&player)),
                            opener: Some(player),
                            portal_type,
                            dungeon,
                        },
                        false,
                    )
                }
                notices::Notice::RealmEventKilled { boss_key, killer } => {
                    let object_type = self.realm_event_object(&boss_key).map(|o| o.object_type);
                    (
                        InstanceEventKind::RealmEventKilled {
                            boss_key,
                            object_type,
                            killer: (!killer.is_empty()).then_some(killer),
                        },
                        true,
                    )
                }
            };
            self.emit_instance(at, kind, provisional, events);
        }
    }
}

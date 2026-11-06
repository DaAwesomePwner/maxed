use crate::{identity, Engine};
use maxed_protocol::defs::ObjectStatusData;
use maxed_types::*;
use std::collections::BTreeMap;

// Preserve only identity and retirement evidence once the full entity leaves
// memory. Restoring the generation also preserves changed-type identities.
pub(crate) struct RetiredEntity {
    generation: u32,
    object_type: Option<u16>,
    fight: bool,
    bag: bool,
}

pub(crate) struct Entity {
    pub generation: u32,
    pub bag_retired: bool,
    pub fight_retired: bool,
    pub object_type: Option<u16>,
    pub present: bool,
    pub stats: BTreeMap<u8, i32>,
    pub name: Option<String>,
    pub guild: Option<String>,
    pub account: Option<[u8; 32]>,
    pub enchants: Vec<EnchantView>,
    pub crucible: Option<f64>,
    pub blood_ritual: Option<f64>,
    pub pos: (f32, f32),
    pub summoner: Option<(i32, u32)>,
    pub last_seen: Timestamp,
    pub first_seen: Timestamp,
}
impl Entity {
    pub fn new(generation: u32, at: Timestamp) -> Self {
        Self {
            generation,
            bag_retired: false,
            fight_retired: false,
            object_type: None,
            present: true,
            stats: BTreeMap::new(),
            name: None,
            guild: None,
            account: None,
            enchants: vec![],
            crucible: None,
            blood_ritual: None,
            pos: (0., 0.),
            summoner: None,
            last_seen: at,
            first_seen: at,
        }
    }
    pub fn stat(&self, id: u8) -> Option<i32> {
        self.stats.get(&id).copied()
    }
}

impl Engine {
    pub(crate) fn is_player(&self, id: i32) -> bool {
        self.entities
            .get(&id)
            .and_then(|e| e.object_type)
            .and_then(|t| self.object_index.get(&t))
            .is_some_and(|&i| self.catalog.objects[i].is_player)
    }
    pub(crate) fn ensure_entity(&mut self, id: i32, events: &mut Vec<EngineEvent>) -> bool {
        if self.entities.contains_key(&id) {
            return true;
        }
        let evicted = if self.entities.len() >= self.config.max_entities {
            if let Some(&(_, old)) = self.inactive_entities.first() {
                Some(old)
            } else {
                self.mark(CoverageIssue::ResourceLimit, events);
                return false;
            }
        } else {
            None
        };
        // Reserve this tombstone before eviction can add another one and trim
        // the bounded table. Otherwise admission could discard its own evidence.
        let retired = self.retired_entities.remove(&id);
        if let Some(old) = evicted {
            self.forget_entity(old, events);
            self.mark(CoverageIssue::ResourceLimit, events);
        }
        let mut entity = Entity::new(self.entity_epoch, self.now);
        if let Some(retired) = retired {
            entity.generation = retired.generation;
            entity.object_type = retired.object_type;
            entity.fight_retired = retired.fight;
            entity.bag_retired = retired.bag;
        }
        self.entities.insert(id, entity);
        true
    }
    pub(crate) fn clear_entities(&mut self) {
        self.instance = Default::default();
        // A transport change clears entities before MAPINFO ends the old run.
        // Keep its pending departures and event IDs until run finalization.
        if !self.run.as_ref().is_some_and(|r| r.view.ended.is_none()) {
            self.notices = Default::default();
        }
        self.kill_detector = Default::default();
        self.observed_kills.clear();
        self.entities.clear();
        self.inactive_entities.clear();
        self.referenced_entities.clear();
        self.retired_entities.clear();
        self.summons.clear();
        self.generation = self.generation.saturating_add(1);
        self.entity_epoch = self.generation;
    }
    pub(crate) fn protect_entity(&mut self, id: i32) {
        if let Some(e) = self.entities.get(&id) {
            self.inactive_entities.remove(&(e.last_seen, id));
            self.referenced_entities.insert(id);
        }
    }
    pub(crate) fn release_entity(&mut self, key: (i32, u32)) {
        if self
            .entities
            .get(&key.0)
            .is_some_and(|e| e.generation == key.1)
            && !self.is_player(key.0)
            && !self.fights.contains_key(&key)
            && !self.drops.contains_key(&key)
        {
            self.referenced_entities.remove(&key.0);
            self.schedule_inactive(key.0);
        }
    }
    fn schedule_inactive(&mut self, id: i32) {
        if let Some(e) = self.entities.get(&id) {
            if !e.present
                && !self.referenced_entities.contains(&id)
                && e.summoner.is_none()
                && !self.summons.contains_key(&id)
                && !self.local.as_ref().is_some_and(|l| l.object == id)
            {
                self.inactive_entities.insert((e.last_seen, id));
            }
        }
    }
    pub(crate) fn prune_entities(&mut self, events: &mut Vec<EngineEvent>) {
        while let Some(&(at, id)) = self.inactive_entities.first() {
            if self.now.0.saturating_sub(at.0) < 30_000_000 {
                break;
            }
            self.forget_entity(id, events);
        }
    }
    pub(crate) fn forget_entity(&mut self, id: i32, events: &mut Vec<EngineEvent>) {
        self.remove_entity(id, events);
        if let Some(e) = self.entities.remove(&id) {
            self.inactive_entities.remove(&(e.last_seen, id));
            self.referenced_entities.remove(&id);
            if e.fight_retired || e.bag_retired {
                self.retired_entities.insert(
                    id,
                    RetiredEntity {
                        generation: e.generation,
                        object_type: e.object_type,
                        fight: e.fight_retired,
                        bag: e.bag_retired,
                    },
                );
                while self.retired_entities.len() > self.config.max_entities {
                    self.retired_entities.pop_first();
                }
            }
        }
    }
    pub(crate) fn set_summoner(&mut self, owner: i32, summoner: i32) {
        self.unlink_summoner(owner);
        let Some(generation) = self.entities.get(&summoner).map(|e| e.generation) else {
            return;
        };
        if owner == summoner {
            return;
        }
        if let Some(e) = self.entities.get_mut(&owner) {
            e.summoner = Some((summoner, generation));
            self.inactive_entities.remove(&(e.last_seen, owner));
            self.summons.entry(summoner).or_default().insert(owner);
            let parent = &self.entities[&summoner];
            self.inactive_entities.remove(&(parent.last_seen, summoner));
        }
    }
    fn unlink_summoner(&mut self, id: i32) {
        if let Some((parent, _)) = self.entities.get_mut(&id).and_then(|e| e.summoner.take()) {
            if let Some(children) = self.summons.get_mut(&parent) {
                children.remove(&id);
                if children.is_empty() {
                    self.summons.remove(&parent);
                }
            }
            self.schedule_inactive(parent);
        }
    }
    // TomatoData.entityUpdate/updateNewTick and Entity.updateStats: merge only observed deltas.
    pub(crate) fn merge_entity(
        &mut self,
        status: ObjectStatusData,
        ty: Option<u16>,
        events: &mut Vec<EngineEvent>,
    ) {
        let id = status.objectId;
        if let Some(ty) = ty {
            // Map-scoped and bounded by the u16 object-type space, even after eviction.
            self.notices.observed_types.insert(ty);
        }
        let mut replacement_generation = None;
        let old_type = self
            .entities
            .get(&id)
            .and_then(|e| e.object_type)
            .or_else(|| self.retired_entities.get(&id).and_then(|e| e.object_type));
        // UPDATE drops are view exits. Only a different observed type proves a new object.
        if ty.is_some() && old_type.is_some() && old_type != ty {
            self.generation = self.generation.saturating_add(1);
            replacement_generation = Some(self.generation);
            self.forget_entity(id, events);
            for (_, (_, reason)) in self.removed_shots.range_mut((id, 0)..=(id, u16::MAX)) {
                *reason = crate::ReconstructionReason::StaleOwnerGeneration;
            }
            self.retired_entities.remove(&id);
        }
        if !self.ensure_entity(id, events) {
            return;
        }
        let is_bag = ty
            .or(self.entities[&id].object_type)
            .is_some_and(crate::drops::known_bag);
        let is_player = ty
            .or(self.entities[&id].object_type)
            .and_then(|t| self.object_index.get(&t))
            .is_some_and(|&i| self.catalog.objects[i].is_player);
        if is_player {
            if let Some(scope) = &mut self.scope {
                scope.touched.insert(id);
            }
        }
        let entity = self.entities.get_mut(&id).expect("entity inserted");
        self.inactive_entities.remove(&(entity.last_seen, id));
        if let Some(generation) = replacement_generation {
            entity.generation = generation;
        }
        entity.present = true;
        entity.last_seen = self.now;
        if let Some(t) = ty {
            entity.object_type = Some(t);
            // A typed re-entry replaces combat state, unlike incremental NEWTICK.
            // Only secondary condition (96) has fixture evidence for omitted zero.
            for id in [21, 29, 96] {
                entity.stats.remove(&id);
            }
            entity.stats.insert(96, 0);
        }
        if status.pos.x.is_finite() && status.pos.y.is_finite() {
            entity.pos = (status.pos.x, status.pos.y);
        }
        let fame_observed = status
            .stats
            .iter()
            .rev()
            .find(|s| s.statTypeNum == 57 && s.stringStatValue.is_none())
            .map(|s| s.statValue);
        let hp_observed = status
            .stats
            .iter()
            .rev()
            .find(|s| s.statTypeNum == 1 && s.stringStatValue.is_none())
            .map(|s| s.statValue);
        let has_enchant = status.stats.iter().any(|s| s.statTypeNum == 80);
        for stat in status.stats {
            if !has_enchant
                && (8..16).contains(&stat.statTypeNum)
                && entity.stat(stat.statTypeNum) != Some(stat.statValue)
            {
                if let Some(enchant) = entity.enchants.get_mut(usize::from(stat.statTypeNum - 8)) {
                    *enchant = crate::parse_enchant(None);
                }
            }
            match stat.statTypeNum {
                31 => {
                    entity.name = stat.stringStatValue.filter(|s| s.len() <= 256);
                }
                38 => {
                    entity.account = stat
                        .stringStatValue
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|s| identity::account(&self.config.identity_key, s));
                }
                62 if is_player => {
                    entity.guild =
                        stat.stringStatValue
                            .filter(|s| !s.is_empty())
                            .map(|mut guild| {
                                let mut end = guild.len().min(64);
                                while !guild.is_char_boundary(end) {
                                    end -= 1;
                                }
                                guild.truncate(end);
                                guild
                            });
                }
                80 => {
                    entity.enchants = crate::enchants::slots(
                        stat.stringStatValue.as_deref(),
                        if is_bag || entity.object_type.is_none() {
                            8
                        } else {
                            4
                        },
                    );
                }
                128 | 155 => {
                    let multiplier = stat.stringStatValue.as_deref().and_then(|s| {
                        if s.is_empty() {
                            // An observed empty channel means no active bonus.
                            // Missing text or an unknown active ID remains uncertain.
                            Some(1.0)
                        } else {
                            self.catalog
                                .crucible_bonuses
                                .iter()
                                .find(|c| c.id == s)
                                .map(|c| c.damage_multiplier)
                        }
                    });
                    if stat.statTypeNum == 128 {
                        entity.crucible = multiplier;
                    } else {
                        entity.blood_ritual = multiplier;
                    }
                }
                _ if stat.stringStatValue.is_none() => {
                    entity.stats.insert(stat.statTypeNum, stat.statValue);
                }
                _ => {} // Never retain arbitrary string stats, chat, or account data.
            }
        }
        let generation = entity.generation;
        let target_type = entity.object_type;
        let max_hp = entity.stat(0).filter(|n| *n > 0);
        let target_hp = entity.stat(1);
        if self
            .local
            .as_ref()
            .is_some_and(|l| l.object == id && l.area == Some(self.map_ordinal))
        {
            if let Some(hp) = hp_observed {
                self.hp_buffer.observe(HpSample {
                    at: self.now,
                    hp,
                    max_hp,
                });
            }
        }
        if let Some(hp) = hp_observed {
            self.kill_detector.hp((id, generation), hp, self.now);
        }
        if let Some(ty) = target_type {
            self.instance.resolve_quest(id, ty);
            if self
                .instance
                .quest
                .as_ref()
                .is_some_and(|q| q.object_id == id)
            {
                self.instance.quest_generation.get_or_insert(generation);
            }
        }
        let kill = self.kill_detector.resolve_quest(id, generation, self.now);
        self.observe_kill(kill, events);
        if let Some(f) = self.fights.get_mut(&(id, generation)) {
            let old = (
                f.base.target_type,
                f.base.boss_type,
                f.base.max_hp,
                f.base.target_hp,
            );
            if f.base.target_hp != target_hp {
                self.dirty_fights.insert((id, generation));
            }
            f.base.target_hp = target_hp;
            f.base.target_type = target_type;
            f.base.boss_type = target_type.filter(|t| {
                self.object_index
                    .get(t)
                    .is_some_and(|&i| self.catalog.objects[i].is_boss)
            });
            f.base.max_hp = f.base.max_hp.into_iter().chain(max_hp).max();
            if old
                != (
                    f.base.target_type,
                    f.base.boss_type,
                    f.base.max_hp,
                    f.base.target_hp,
                )
            {
                f.changed();
                self.dirty_fights.insert((id, generation));
            }
        }
        self.refresh_character(events);
        if is_player {
            self.player_presence(id, true, events);
        }
        if let Some(fame) = fame_observed {
            if self
                .local
                .as_ref()
                .is_some_and(|l| l.object == id && l.area == Some(self.map_ordinal))
            {
                if let Some(run) = self
                    .run
                    .as_mut()
                    .filter(|r| r.view.ended.is_none() && r.view.died_at.is_none())
                {
                    match &mut run.fame_samples {
                        Some((create, _, last)) if *create == self.create_number => {
                            *last = Some(fame)
                        }
                        sample => *sample = Some((self.create_number, fame, None)),
                    }
                }
            }
        }
        self.observe_bag(id, events);
    }
    pub(crate) fn remove_entity(&mut self, id: i32, events: &mut Vec<EngineEvent>) {
        self.player_presence(id, false, events);
        if let Some(scope) = &mut self.scope {
            scope.capped_objects.remove(&id);
        }
        self.unlink_summoner(id);
        if let Some(children) = self.summons.remove(&id) {
            for child in children {
                if let Some(e) = self.entities.get_mut(&child) {
                    e.summoner = None;
                }
                self.schedule_inactive(child);
            }
        }
        if let Some(e) = self.entities.get_mut(&id) {
            if e.present {
                e.present = false;
                e.last_seen = self.now;
                if let Some(scope) = &mut self.scope {
                    scope.touched.insert(id);
                }
            }
        }
        self.remove_owner_shots(id);
        self.schedule_inactive(id);
    }
}

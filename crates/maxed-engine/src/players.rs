use crate::{characters::equipment, clock, engine::series::Series, Engine};
use maxed_types::*;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum player profiles and roster members admitted in one observation scope.
pub const PROFILE_LIMIT: usize = 512;

/// Presentation-only presence positioned within the accompanying engine batch.
#[derive(Clone, Debug, PartialEq)]
pub struct RosterPresence {
    /// Apply before this zero-based event index (or after all events at `len`).
    pub event_index: usize,
    pub presence: PlayerPresence,
}

struct ProfileSlot {
    view: PlayerProfileView,
    published: Option<PlayerProfileView>,
}

// One observation scope lives for the whole map, including after a run ends.
pub(crate) struct Scope {
    pub id: String,
    pub run_id: Option<String>,
    profiles: BTreeMap<String, ProfileSlot>,
    present: BTreeSet<String>,
    present_objects: BTreeSet<i32>,
    pub(crate) capped_objects: BTreeSet<i32>,
    roster_dirty: BTreeSet<String>,
    pub(crate) touched: BTreeSet<i32>,
    rekeyed: BTreeSet<String>,
    damaged_players: BTreeSet<String>,
    capped: bool,
    timeline: Series,
    dirty_players: BTreeSet<String>,
    published: Timestamp,
    published_buckets: Vec<(Timestamp, Timestamp)>,
    full: bool,
    compacted: bool,
    disabled_published: bool,
}

impl Scope {
    pub(crate) fn new(id: String, run_id: Option<String>, at: Timestamp) -> Self {
        Self {
            id,
            run_id,
            profiles: BTreeMap::new(),
            present: BTreeSet::new(),
            present_objects: BTreeSet::new(),
            capped_objects: BTreeSet::new(),
            roster_dirty: BTreeSet::new(),
            touched: BTreeSet::new(),
            rekeyed: BTreeSet::new(),
            damaged_players: BTreeSet::new(),
            capped: false,
            timeline: Series::default(),
            dirty_players: BTreeSet::new(),
            published: at,
            published_buckets: vec![],
            full: false,
            compacted: false,
            disabled_published: false,
        }
    }

    pub(crate) fn merge_identity(&mut self, old: &str, new: &str) {
        // Rekeying preserves the object id, so present_objects stays unchanged.
        if self.present.remove(old) {
            self.present.insert(new.into());
        }
        if self.damaged_players.remove(old) {
            self.damaged_players.insert(new.into());
        }
        if let Some(mut slot) = self.profiles.remove(old) {
            slot.view.player_id = new.into();
            self.profiles.insert(new.into(), slot);
            self.rekeyed.insert(new.into());
            self.roster_dirty.remove(old);
            self.roster_dirty.insert(new.into());
        }
        self.timeline.merge_identity(old, new);
        self.full = true;
        if self.timeline.dimensions().0 > 0 && !self.disabled_published {
            self.dirty_players.remove(old);
            self.dirty_players.insert(new.into());
        }
    }
}

impl Engine {
    /// Presence from the latest engine update, in emission order; never persisted.
    /// Drain alongside that update's events, before calling the engine again.
    /// Event indices preserve scope endings and identity merges without placing
    /// presence in the durable event stream. Undrained updates are discarded when
    /// the next engine update starts, so non-roster replay callers retain no backlog.
    pub fn take_roster_presence(&mut self) -> Vec<RosterPresence> {
        std::mem::take(&mut self.roster_presence)
    }

    /// Refreshed current-scope profiles for presentation only, never persistence.
    pub fn take_roster_profiles(&mut self) -> Vec<PlayerProfileView> {
        let Some(scope) = &mut self.scope else {
            return vec![];
        };
        std::mem::take(&mut scope.roster_dirty)
            .into_iter()
            .filter_map(|id| scope.profiles.get(&id).map(|slot| slot.view.clone()))
            .collect()
    }

    pub(crate) fn scope_id(&self) -> Option<String> {
        self.scope.as_ref().map(|scope| scope.id.clone())
    }

    pub(crate) fn player_presence(
        &mut self,
        id: i32,
        present: bool,
        events: &mut Vec<EngineEvent>,
    ) {
        let Some(scope) = &self.scope else { return };
        // Avoid allocating a player row for every steady-state tick merge.
        if scope.capped_objects.contains(&id) || present == scope.present_objects.contains(&id) {
            return;
        }
        let Some(row) = self.player_row(id) else {
            return;
        };
        if present && !scope.profiles.contains_key(&row.player_id) {
            // Reserve the profile slot now, before another object in this packet
            // can consume it. Publication still uses the normal profile throttle.
            if self.refresh_profile(id, events).is_none() {
                return;
            }
        }
        let scope = self.scope.as_mut().expect("active scope");
        let changed = if present {
            scope.present_objects.insert(id);
            scope.present.insert(row.player_id.clone())
        } else {
            scope.present_objects.remove(&id);
            scope.present.remove(&row.player_id)
        };
        if changed {
            self.roster_presence.push(RosterPresence {
                event_index: events.len(),
                presence: PlayerPresence {
                    scope_id: scope.id.clone(),
                    run_id: scope.run_id.clone(),
                    player_id: row.player_id,
                    object_id: id,
                    present,
                    at: clock::millis(self.now),
                },
            });
        }
    }

    fn refresh_profile(&mut self, id: i32, events: &mut Vec<EngineEvent>) -> Option<String> {
        if self.scope.as_ref()?.capped_objects.contains(&id) {
            return None;
        }
        let row = self.player_row(id)?;
        let entity = &self.entities[&id];
        let scope = self.scope.as_ref().expect("active scope");
        if !entity.present && !scope.profiles.contains_key(&row.player_id) {
            return None;
        }
        if !scope.profiles.contains_key(&row.player_id) && scope.profiles.len() >= PROFILE_LIMIT {
            let scope = self.scope.as_mut().expect("active scope");
            scope.capped_objects.insert(id);
            if !scope.capped {
                scope.capped = true;
                self.mark(CoverageIssue::ResourceLimit, events);
            }
            return None;
        }
        let (maxed, maxed_stats) = if row.is_local {
            let panel = self.character_panel(entity);
            let maxed = panel.lines.iter().try_fold(0u8, |count, line| {
                line.at_cap.map(|at_cap| count + u8::from(at_cap))
            });
            let maxed_stats = [0, 3, 20, 21, 22, 28, 26, 27]
                .into_iter()
                .filter(|id| {
                    panel
                        .lines
                        .iter()
                        .any(|line| line.stat_id == *id && line.at_cap == Some(true))
                })
                .collect();
            (maxed, maxed_stats)
        } else {
            // Remote players have no exalt levels; preserve their observed-stat evaluation.
            (
                self.maxed(row.class_id, |id| entity.stat(id)),
                self.maxed_stats(row.class_id, |id| entity.stat(id)),
            )
        };
        let view = PlayerProfileView {
            scope_id: scope.id.clone(),
            run_id: scope.run_id.clone(),
            player_id: row.player_id.clone(),
            object_id: id,
            is_local: row.is_local,
            name: row.name,
            class_id: row.class_id,
            level: entity.stat(7).and_then(|v| u8::try_from(v).ok()),
            stars: entity.stat(30),
            fame: entity.stat(57),
            skin_id: entity.stat(25),
            guild: entity.guild.clone(),
            stats: entity
                .stats
                .iter()
                .filter(|(id, _)| matches!(id, 0 | 3 | 20..=22 | 26..=28 | 46..=53))
                .map(|(&stat_id, &value)| StatView { stat_id, value })
                .collect(),
            maxed,
            maxed_stats,
            equipment: equipment(Some(entity)),
            first_seen: clock::millis(entity.first_seen),
            last_seen: clock::millis(entity.last_seen),
        };
        let scope = self.scope.as_mut().expect("active scope");
        let player_id = row.player_id.clone();
        if let Some(slot) = scope.profiles.get_mut(&row.player_id) {
            // A clock-only refresh is not a changed roster profile.
            slot.view.last_seen = view.last_seen;
            if slot.view != view {
                scope.roster_dirty.insert(player_id.clone());
            }
            slot.view = view;
        } else {
            scope.roster_dirty.insert(player_id.clone());
            scope.profiles.insert(
                row.player_id,
                ProfileSlot {
                    view,
                    published: None,
                },
            );
        }
        Some(player_id)
    }

    pub(crate) fn flush_profiles(&mut self, events: &mut Vec<EngineEvent>) {
        let Some(scope) = &mut self.scope else { return };
        let touched = std::mem::take(&mut scope.touched);
        let mut rebuilt = std::mem::take(&mut scope.rekeyed);
        for id in touched {
            if let Some(player_id) = self.refresh_profile(id, events) {
                rebuilt.insert(player_id);
            }
        }
        let scope = self.scope.as_mut().expect("active scope");
        for id in rebuilt {
            if scope.run_id.is_none() && !scope.damaged_players.contains(&id) {
                continue;
            }
            let Some(slot) = scope.profiles.get_mut(&id) else {
                continue;
            };
            let mut compare = slot.view.clone();
            if let Some(published) = &slot.published {
                compare.last_seen = published.last_seen;
            }
            if slot.published.as_ref() != Some(&compare) {
                events.push(EngineEvent::PlayerObserved(slot.view.clone()));
                slot.published = Some(slot.view.clone());
            }
        }
    }

    pub(crate) fn drain_fight_timelines(&mut self) {
        for fight in self.fights.values_mut() {
            let contributions = std::mem::take(&mut fight.timeline_out);
            let Some(scope) = &mut self.scope else {
                continue;
            };
            for (at, player, damage) in contributions {
                if damage > 0. && scope.damaged_players.insert(player.player_id.clone()) {
                    scope.rekeyed.insert(player.player_id.clone());
                }
                if scope.disabled_published {
                    continue;
                }
                let count = scope.timeline.buckets.len();
                scope.timeline.add(at, Some(&player), damage);
                scope.compacted |= scope.timeline.buckets.len() < count;
                scope.dirty_players.insert(player.player_id);
            }
        }
    }

    pub(crate) fn flush_timeline(&mut self, force: bool, events: &mut Vec<EngineEvent>) {
        let Some(scope) = &mut self.scope else { return };
        if scope.disabled_published
            || scope.dirty_players.is_empty()
            || !force && self.now.0.saturating_sub(scope.published.0) < 5_000_000
        {
            return;
        }
        let full = scope.full || !scope.timeline.buckets.starts_with(&scope.published_buckets);
        let rows = scope
            .timeline
            .project_selected(|id| full || scope.dirty_players.contains(id));
        let disabled = rows.is_none();
        let players = rows
            .unwrap_or_default()
            .into_iter()
            .map(|row| PlayerSeries {
                player_id: row.player_id,
                object_id: row.object_id,
                is_local: row.is_local,
                points: row.points,
            })
            .collect();
        let buckets = if disabled {
            vec![]
        } else {
            scope
                .timeline
                .buckets
                .iter()
                .zip(scope.timeline.bucket_totals())
                .map(
                    |(&(from, until), (party_total, party_active))| ScopeBucket {
                        from,
                        until,
                        party_total,
                        party_active,
                    },
                )
                .collect()
        };
        events.push(EngineEvent::ScopeTimelineUpdated(ScopeTimelineUpdate {
            scope_id: scope.id.clone(),
            run_id: scope.run_id.clone(),
            buckets,
            players,
            full,
            compacted: scope.compacted,
            disabled,
        }));
        scope.published = self.now;
        scope.published_buckets.clone_from(&scope.timeline.buckets);
        scope.dirty_players.clear();
        scope.full = false;
        scope.disabled_published = disabled;
    }

    pub(crate) fn end_scope(&mut self, events: &mut Vec<EngineEvent>) {
        self.drain_fight_timelines();
        self.flush_profiles(events);
        self.flush_timeline(true, events);
        if let Some(scope) = &mut self.scope {
            scope.present_objects.clear();
            scope.capped_objects.clear();
            for player_id in std::mem::take(&mut scope.present) {
                let Some(slot) = scope.profiles.get(&player_id) else {
                    continue;
                };
                self.roster_presence.push(RosterPresence {
                    event_index: events.len(),
                    presence: PlayerPresence {
                        scope_id: scope.id.clone(),
                        run_id: scope.run_id.clone(),
                        player_id,
                        object_id: slot.view.object_id,
                        present: false,
                        at: clock::millis(self.now),
                    },
                });
            }
        }
        if let Some(scope) = self.scope.take() {
            for slot in scope.profiles.into_values() {
                if scope.run_id.is_none() && !scope.damaged_players.contains(&slot.view.player_id) {
                    continue;
                }
                if slot.published.as_ref() != Some(&slot.view) {
                    events.push(EngineEvent::PlayerObserved(slot.view));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineConfig;
    use std::sync::Arc;

    fn engine() -> Engine {
        Engine::new(
            EngineConfig::default(),
            Arc::new(Catalog {
                format_version: 1,
                client_fingerprint: "synthetic-presence".into(),
                items: vec![],
                objects: vec![ObjectDef {
                    object_type: 100,
                    name: "Synthetic Player".into(),
                    is_player: true,
                    is_boss: false,
                    is_miniboss: false,
                    class_name: None,
                    dungeon_name: None,
                    event_boss: None,
                    realm_event_key: None,
                    sprite_key: None,
                }],
                classes: vec![],
                enchants: vec![],
                dungeons: vec![],
                bags: vec![],
                sprites: vec![],
                tiles: vec![],
                crucible_bonuses: vec![],
            }),
        )
        .unwrap()
    }

    #[test]
    fn end_scope_skips_presence_with_missing_profile() {
        let mut engine = engine();
        let mut scope = Scope::new("synthetic:map:1".into(), None, Timestamp(0));
        scope.present.insert("synthetic:player:3".into());
        scope.present_objects.insert(3);
        engine.scope = Some(scope);
        let mut events = vec![];

        engine.end_scope(&mut events);

        assert!(engine.scope.is_none());
        assert!(events.is_empty());
        assert!(engine.take_roster_presence().is_empty());
    }

    #[test]
    fn capped_player_ticks_emit_nothing_and_keep_state_bounded() {
        let mut engine = engine();
        engine.scope = Some(Scope::new("synthetic:map:1".into(), None, Timestamp(0)));
        let mut events = vec![];
        for id in 0..=PROFILE_LIMIT as i32 {
            let mut entity = crate::world::Entity::new(0, Timestamp(0));
            entity.object_type = Some(100);
            engine.entities.insert(id, entity);
            engine.player_presence(id, true, &mut events);
        }
        assert_eq!(engine.take_roster_profiles().len(), PROFILE_LIMIT);
        assert_eq!(engine.take_roster_presence().len(), PROFILE_LIMIT);
        let capped_id = PROFILE_LIMIT as i32;
        let dimensions = |scope: &Scope| {
            (
                scope.profiles.len(),
                scope.present.len(),
                scope.present_objects.len(),
                scope.capped_objects.len(),
                scope.touched.len(),
                scope.roster_dirty.len(),
            )
        };
        let before = dimensions(engine.scope.as_ref().unwrap());
        assert_eq!(before, (512, 512, 512, 1, 0, 0));
        events.clear();
        for tick in 1..=100 {
            engine.now = Timestamp(tick * 250_000);
            engine.merge_entity(
                maxed_protocol::defs::ObjectStatusData {
                    objectId: capped_id,
                    pos: maxed_protocol::defs::WorldPosData { x: 0.0, y: 0.0 },
                    stats: vec![],
                },
                None,
                &mut events,
            );
            engine.flush_profiles(&mut events);
            assert!(events.is_empty());
            assert!(engine.take_roster_profiles().is_empty());
            assert!(engine.take_roster_presence().is_empty());
            assert_eq!(dimensions(engine.scope.as_ref().unwrap()), before);
        }
        engine.remove_entity(capped_id, &mut events);
        assert!(engine.scope.as_ref().unwrap().capped_objects.is_empty());
        engine.player_presence(9999, false, &mut events);
        assert!(events.is_empty());
        engine.end_scope(&mut events);
        assert!(engine.scope.is_none());
    }
}

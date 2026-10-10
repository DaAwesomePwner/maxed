//! Snapshot-only series selection. Event projections remain complete.
use crate::{fights::FightKey, Engine};
#[cfg(test)]
use maxed_types::PlayerSeries;
use maxed_types::{TargetKind, Timestamp};
use std::collections::{BTreeMap, BTreeSet};

pub const SNAPSHOT_SERIES_LIMIT: usize = 16;

#[derive(Clone, Eq, PartialEq)]
struct Rank {
    at: Option<Timestamp>,
    id: String,
    key: FightKey,
}
impl Ord for Rank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .at
            .cmp(&self.at)
            .then_with(|| self.id.cmp(&other.id))
            .then_with(|| self.key.cmp(&other.key))
    }
}
impl PartialOrd for Rank {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
struct Entry {
    rank: Rank,
    local: Option<Rank>,
    priority: bool,
    live_priority: bool,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Selection {
    keys: [Option<FightKey>; SNAPSHOT_SERIES_LIMIT],
}
impl Selection {
    pub fn contains(&self, key: FightKey) -> bool {
        self.keys.contains(&Some(key))
    }
}
#[derive(Default)]
pub(crate) struct SnapshotSeriesCache {
    dirty: BTreeSet<FightKey>,
    entries: BTreeMap<FightKey, Entry>,
    priority: BTreeSet<Rank>,
    live_priority: BTreeSet<Rank>,
    remaining: BTreeSet<Rank>,
    local: BTreeSet<Rank>,
    selected: Selection,
    #[cfg(test)]
    pub fight_lookups: u64,
}
impl SnapshotSeriesCache {
    pub fn invalidate(&mut self, key: FightKey) {
        self.dirty.insert(key);
    }
    pub fn forget(&mut self, key: FightKey) {
        if self.entries.contains_key(&key) {
            self.dirty.insert(key);
        } else {
            self.dirty.remove(&key);
        }
    }
    fn remove(&mut self, key: FightKey) {
        if let Some(old) = self.entries.remove(&key) {
            if old.live_priority {
                self.live_priority.remove(&old.rank);
            }
            if old.priority {
                self.priority.remove(&old.rank);
            } else {
                self.remaining.remove(&old.rank);
            }
            if let Some(local) = old.local {
                self.local.remove(&local);
            }
        }
    }
    fn select_where(&self, visible: impl Fn(FightKey) -> bool, live: bool) -> Selection {
        let mut selected = Selection::default();
        let mut count = 0;
        // Live reserves the latest-local boss focus, then every boss tier
        // and current quest, before ordinary rows. Domain order stays unchanged:
        // Boss/Quest, latest local, remaining. Each set sorts by recency then ID.
        let local = self.local.iter().find(|rank| visible(rank.key));
        let focus = live
            .then(|| {
                self.local.iter().find(|rank| {
                    visible(rank.key)
                        && self
                            .entries
                            .get(&rank.key)
                            .is_some_and(|entry| entry.live_priority)
                })
            })
            .flatten();
        let priority = if live {
            &self.live_priority
        } else {
            &self.priority
        };
        for rank in focus
            .into_iter()
            .chain(priority)
            .chain(local)
            .chain(&self.remaining)
            .filter(|rank| visible(rank.key))
        {
            if count == SNAPSHOT_SERIES_LIMIT {
                break;
            }
            if !selected.contains(rank.key) {
                selected.keys[count] = Some(rank.key);
                count += 1;
            }
        }
        selected
    }
}
impl Engine {
    pub(crate) fn snapshot_series_selection(&self) -> Selection {
        self.snapshot_series_selection_for(false)
    }
    pub(crate) fn live_snapshot_series_selection(&self) -> Selection {
        self.snapshot_series_selection_for(true)
    }
    fn snapshot_series_selection_for(&self, live_only: bool) -> Selection {
        let mut cache = self
            .snapshot_series_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !cache.dirty.is_empty() {
            for key in std::mem::take(&mut cache.dirty) {
                #[cfg(test)]
                {
                    cache.fight_lookups += 1;
                }
                cache.remove(key);
                let Some(f) = self.fights.get(&key).filter(|f| f.base.ended.is_none()) else {
                    continue;
                };
                let rank = Rank {
                    at: f.base.last_hit,
                    id: f.base.id.clone(),
                    key,
                };
                let local = f.latest_local_hit().map(|at| Rank {
                    at: Some(at),
                    ..rank.clone()
                });
                let priority = matches!(
                    f.base.target_kind,
                    Some(TargetKind::Boss | TargetKind::Quest)
                );
                let live_priority = Self::live_fight_priority(f);
                if live_priority {
                    cache.live_priority.insert(rank.clone());
                }
                if priority {
                    cache.priority.insert(rank.clone());
                } else {
                    cache.remaining.insert(rank.clone());
                }
                if let Some(local) = &local {
                    cache.local.insert(local.clone());
                }
                cache.entries.insert(
                    key,
                    Entry {
                        rank,
                        local,
                        priority,
                        live_priority,
                    },
                );
            }
            cache.selected = cache.select_where(|_| true, false);
        }
        if live_only {
            // Visibility can expire with the clock even when no fight is dirty.
            cache.select_where(
                |key| {
                    self.fights
                        .get(&key)
                        .is_some_and(|f| self.live_fight_visible(f))
                },
                true,
            )
        } else {
            cache.selected
        }
    }
    #[cfg(test)]
    pub(super) fn snapshot_series_projection(&self) -> Vec<Option<Vec<PlayerSeries>>> {
        let selected = self.snapshot_series_selection();
        self.fights
            .iter()
            .map(|(&key, f)| {
                selected
                    .contains(key)
                    .then(|| f.projected().player_series.clone())
                    .flatten()
            })
            .collect()
    }
}

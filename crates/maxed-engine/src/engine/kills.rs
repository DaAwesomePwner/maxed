//! Evidence detector only: it never retires a fight or changes capture coverage.
use super::notices::LootKill;
use maxed_types::Timestamp;
use std::collections::{BTreeMap, BTreeSet};
pub(crate) type TargetKey = (i32, u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KillEvidence {
    VictoryQuestTarget,
    VictorySoleBoss,
    HpZeroRemoved,
    RealmAnnouncement,
    AnnouncedDeathRemoved,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Kill {
    pub target: TargetKey,
    pub at: Timestamp,
    pub evidence: KillEvidence,
    pub provisional: bool,
}
pub(crate) struct ActiveTarget<'a> {
    pub key: TargetKey,
    pub boss: bool,
    pub last_hit: Option<Timestamp>,
    pub name: Option<&'a str>,
}
#[derive(Clone, Copy)]
struct QuestRef {
    id: i32,
    generation: Option<u32>,
}
impl QuestRef {
    fn key(self) -> Option<TargetKey> {
        self.generation.map(|generation| (self.id, generation))
    }
}
struct Removal {
    at: Timestamp,
    loot: Option<LootKill>,
}
#[derive(Default)]
pub(crate) struct Kills {
    current: Option<(QuestRef, Timestamp)>,
    previous: Option<(QuestRef, Timestamp)>,
    pending_victory: Option<Timestamp>,
    announcements: BTreeMap<TargetKey, Timestamp>,
    removals: BTreeMap<TargetKey, Removal>,
    announcement_expiry: BTreeSet<(Timestamp, TargetKey, bool)>,
    zero_hp: BTreeMap<TargetKey, Timestamp>,
    zero_expiry: BTreeSet<(Timestamp, TargetKey)>,
}
impl Kills {
    #[cfg(test)]
    pub fn quest(&mut self, target: Option<TargetKey>, at: Timestamp) -> Option<Kill> {
        match target {
            Some((id, generation)) => self.quest_id(id, Some(generation), at),
            None => self.quest_id(-1, None, at),
        }
    }
    pub fn quest_id(&mut self, id: i32, generation: Option<u32>, at: Timestamp) -> Option<Kill> {
        if id >= 0 {
            let since = self
                .current
                .filter(|(q, _)| q.id == id)
                .map_or(at, |(_, since)| since);
            self.current = Some((QuestRef { id, generation }, since));
            self.previous = None;
        } else if id == -1 {
            if let Some((target, _)) = self.current.take() {
                self.previous = Some((target, at));
                if let Some(key) = target.key() {
                    if self
                        .pending_victory
                        .is_some_and(|v| within(at, v, 1_000_000))
                    {
                        self.pending_victory = None;
                        return Some(kill(key, at, KillEvidence::VictoryQuestTarget, false));
                    }
                }
            }
        }
        None
    }
    pub fn resolve_quest(&mut self, id: i32, generation: u32, at: Timestamp) -> Option<Kill> {
        let mut resolved = false;
        for entry in [&mut self.current, &mut self.previous]
            .into_iter()
            .flatten()
        {
            if entry.0.id == id && entry.0.generation.is_none() {
                entry.0.generation = Some(generation);
                resolved = true;
            }
        }
        let victory = self.pending_victory?;
        if !resolved || !within(at, victory, 1_000_000) {
            return None;
        }
        let target = self
            .current
            .filter(|(_, since)| *since <= victory)
            .or_else(|| {
                self.previous.filter(|(_, clear)| {
                    within(victory, *clear, 1_000_000) || within(*clear, victory, 1_000_000)
                })
            })?
            .0
            .key()?;
        self.pending_victory = None;
        Some(kill(
            target,
            victory,
            KillEvidence::VictoryQuestTarget,
            false,
        ))
    }
    pub fn pending_quest_victory_at(&self, key: TargetKey, at: Timestamp) -> Option<Timestamp> {
        let victory = self.pending_victory.filter(|v| within(at, *v, 1_000_000))?;
        let quest = self
            .current
            .filter(|(_, since)| *since <= victory)
            .or_else(|| {
                self.previous.filter(|(_, clear)| {
                    within(victory, *clear, 1_000_000) || within(*clear, victory, 1_000_000)
                })
            })?
            .0;
        (quest.id == key.0
            && quest
                .generation
                .is_none_or(|generation| generation == key.1))
        .then_some(victory)
    }
    pub fn victory(&mut self, at: Timestamp, active: &[ActiveTarget<'_>]) -> Option<Kill> {
        let quest = self.current.map(|(q, _)| q).or_else(|| {
            self.previous
                .filter(|(_, t)| within(at, *t, 1_000_000))
                .map(|(k, _)| k)
        });
        if let Some(target) = quest {
            if let Some(key) = target.key() {
                self.pending_victory = None;
                return Some(kill(key, at, KillEvidence::VictoryQuestTarget, false));
            }
            // A known quest ID without an observed generation is not evidence
            // for a different active boss. Wait for the object to resolve.
            self.pending_victory = Some(at);
            return None;
        }
        let mut bosses = active
            .iter()
            .filter(|a| a.boss && a.last_hit.is_some_and(|t| within(at, t, 10_000_000)));
        if let Some(boss) = bosses.next() {
            if bosses.next().is_none() {
                self.pending_victory = None;
                return Some(kill(boss.key, at, KillEvidence::VictorySoleBoss, true));
            }
        }
        self.pending_victory = Some(at);
        None
    }
    pub fn hp(&mut self, key: TargetKey, hp: i32, at: Timestamp) {
        while let Some(&(time, key)) = self.zero_expiry.first() {
            if at.0.saturating_sub(time.0) <= 2_000_000 {
                break;
            }
            self.zero_expiry.remove(&(time, key));
            self.zero_hp.remove(&key);
        }
        if hp == 0 {
            // Match the engine entity bound, including hostile input streams.
            if self.zero_hp.len() < 100_000 || self.zero_hp.contains_key(&key) {
                if let Some(old) = self.zero_hp.insert(key, at) {
                    self.zero_expiry.remove(&(old, key));
                }
                self.zero_expiry.insert((at, key));
            }
        } else if hp > 0 {
            if let Some(old) = self.zero_hp.remove(&key) {
                self.zero_expiry.remove(&(old, key));
            }
        }
    }
    pub fn removed_at(&self, key: TargetKey) -> Option<Timestamp> {
        self.removals.get(&key).map(|removed| removed.at)
    }
    pub fn removed_loot(&self, key: TargetKey) -> Option<LootKill> {
        self.removals.get(&key).and_then(|removed| removed.loot)
    }
    pub fn removed(
        &mut self,
        key: TargetKey,
        at: Timestamp,
        loot: Option<LootKill>,
    ) -> Option<Kill> {
        self.expire_announcements(at);
        if self.removals.len() < 100_000 || self.removals.contains_key(&key) {
            if let Some(old) = self.removals.insert(key, Removal { at, loot }) {
                self.announcement_expiry.remove(&(old.at, key, false));
            }
            self.announcement_expiry.insert((at, key, false));
        }
        if let Some(time) = self.announcements.remove(&key) {
            self.announcement_expiry.remove(&(time, key, true));
            if within(at, time, 500_000) {
                return Some(kill(key, at, KillEvidence::AnnouncedDeathRemoved, false));
            }
        }
        let zero = self.zero_hp.remove(&key)?;
        self.zero_expiry.remove(&(zero, key));
        Some(zero)
            .filter(|t| within(at, *t, 2_000_000))
            .map(|_| kill(key, at, KillEvidence::HpZeroRemoved, false))
    }
    pub fn oryx(&mut self, key: &str, at: Timestamp, active: &[ActiveTarget<'_>]) -> Option<Kill> {
        self.expire_announcements(at);
        let normalized = key.replace('_', " ");
        let mut matches = active.iter().filter(|a| {
            a.name
                .is_some_and(|n| n.replace('_', " ").eq_ignore_ascii_case(&normalized))
        });
        let target = matches.next()?.key;
        if matches.next().is_some() {
            return None;
        }
        if let Some(removed) = self.removed_at(target).filter(|t| within(at, *t, 500_000)) {
            return Some(kill(
                target,
                removed,
                KillEvidence::AnnouncedDeathRemoved,
                false,
            ));
        }
        self.remember_announcement(target, at);
        Some(kill(target, at, KillEvidence::RealmAnnouncement, true))
    }
    fn remember_announcement(&mut self, key: TargetKey, at: Timestamp) {
        if self.announcements.len() < 100_000 || self.announcements.contains_key(&key) {
            if let Some(old) = self.announcements.insert(key, at) {
                self.announcement_expiry.remove(&(old, key, true));
            }
            self.announcement_expiry.insert((at, key, true));
        }
    }
    pub fn expire_announcements(&mut self, at: Timestamp) {
        while let Some(&(time, key, announcement)) = self.announcement_expiry.first() {
            if at.0.saturating_sub(time.0) <= 500_000 {
                break;
            }
            self.announcement_expiry.remove(&(time, key, announcement));
            if announcement {
                self.announcements.remove(&key);
            } else {
                self.removals.remove(&key);
            }
        }
    }
}
fn within(now: Timestamp, before: Timestamp, window: u64) -> bool {
    now.0.checked_sub(before.0).is_some_and(|age| age <= window)
}
fn kill(target: TargetKey, at: Timestamp, evidence: KillEvidence, provisional: bool) -> Kill {
    Kill {
        target,
        at,
        evidence,
        provisional,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn boss(id: i32) -> ActiveTarget<'static> {
        ActiveTarget {
            key: (id, 1),
            boss: true,
            last_hit: Some(Timestamp(0)),
            name: Some("Synthetic Boss"),
        }
    }
    #[test]
    fn quest_clear_before_victory_same_tick_attributes_previous_target() {
        let mut k = Kills::default();
        k.quest(Some((42, 1)), Timestamp(0));
        k.quest(None, Timestamp(100));
        let kill = k.victory(Timestamp(100), &[]).unwrap();
        assert_eq!(
            (kill.target, kill.evidence, kill.provisional),
            ((42, 1), KillEvidence::VictoryQuestTarget, false)
        );
    }
    #[test]
    fn unresolved_quest_binds_generation_after_clear_and_victory_in_either_order() {
        for clear_first in [true, false] {
            let mut k = Kills::default();
            k.quest_id(42, None, Timestamp(0));
            if clear_first {
                k.quest_id(-1, None, Timestamp(100));
            }
            assert_eq!(k.victory(Timestamp(100), &[]), None);
            if !clear_first {
                k.quest_id(-1, None, Timestamp(100));
            }
            let kill = k.resolve_quest(42, 7, Timestamp(200)).unwrap();
            assert_eq!(kill.target, (42, 7));
            assert_eq!(kill.at, Timestamp(100));
            assert_eq!(kill.evidence, KillEvidence::VictoryQuestTarget);
            assert!(!kill.provisional);
            assert_eq!(k.resolve_quest(42, 8, Timestamp(300)), None);
            assert_eq!(k.victory(Timestamp(400), &[]).unwrap().target, (42, 7));
        }
    }
    #[test]
    fn pending_quest_victory_boundary_is_scoped_and_expires() {
        let mut k = Kills::default();
        k.quest_id(42, None, Timestamp(0));
        k.victory(Timestamp(100), &[]);
        assert_eq!(
            k.pending_quest_victory_at((42, 1), Timestamp(200)),
            Some(Timestamp(100))
        );
        assert_eq!(k.pending_quest_victory_at((43, 1), Timestamp(200)), None);
        k.quest_id(-1, None, Timestamp(300));
        assert_eq!(
            k.pending_quest_victory_at((42, 1), Timestamp(400)),
            Some(Timestamp(100))
        );
        assert_eq!(
            k.pending_quest_victory_at((42, 1), Timestamp(1_000_101)),
            None
        );
        k.quest_id(43, None, Timestamp(1_000_200));
        assert_eq!(
            k.pending_quest_victory_at((43, 1), Timestamp(1_000_200)),
            None
        );
    }
    #[test]
    fn unresolved_quest_resolution_does_not_replay_stale_victory() {
        let mut k = Kills::default();
        k.quest_id(42, None, Timestamp(0));
        k.victory(Timestamp(100), &[]);
        assert_eq!(k.resolve_quest(42, 7, Timestamp(1_000_101)), None);
        let mut k = Kills::default();
        k.victory(Timestamp(100), &[]);
        k.quest_id(42, None, Timestamp(200));
        assert_eq!(k.resolve_quest(42, 7, Timestamp(300)), None);
        assert_eq!(
            k.quest_id(-1, None, Timestamp(400)).unwrap().target,
            (42, 7)
        );
    }
    #[test]
    fn victory_with_two_boss_fights_and_no_quest_is_unattributed() {
        let mut k = Kills::default();
        assert_eq!(k.victory(Timestamp(100), &[boss(1), boss(2)]), None);
        assert_eq!(
            k.victory(Timestamp(100), &[boss(1)]).unwrap().evidence,
            KillEvidence::VictorySoleBoss
        );
    }
    #[test]
    fn drop_without_hp_zero_is_not_a_kill() {
        let mut k = Kills::default();
        k.hp((42, 1), 100, Timestamp(0));
        assert_eq!(k.removed((42, 1), Timestamp(1), None), None);
        k.hp((42, 1), 0, Timestamp(2));
        assert!(k.removed((42, 1), Timestamp(3), None).is_some());
    }
    #[test]
    fn scripted_boss_removed_with_positive_hp_needs_victory() {
        let mut k = Kills::default();
        k.quest(Some((42, 1)), Timestamp(0));
        k.hp((42, 1), 500, Timestamp(0));
        assert_eq!(k.removed((42, 1), Timestamp(10), None), None);
        assert_eq!(
            k.victory(Timestamp(10), &[]).unwrap().evidence,
            KillEvidence::VictoryQuestTarget
        );
    }
    #[test]
    fn hp_requires_same_generation_deadline_and_no_recovery() {
        let mut k = Kills::default();
        k.hp((42, 1), 0, Timestamp(0));
        assert_eq!(k.removed((42, 2), Timestamp(1), None), None);
        k.hp((42, 1), 1, Timestamp(2));
        assert_eq!(k.removed((42, 1), Timestamp(3), None), None);
        k.hp((42, 1), 0, Timestamp(4));
        assert_eq!(k.removed((42, 1), Timestamp(2_000_005), None), None);
        k.hp((42, 1), 0, Timestamp(5));
        assert!(k.removed((42, 1), Timestamp(2_000_005), None).is_some());
    }
    #[test]
    fn clear_after_unattributed_victory_and_expiry() {
        let mut k = Kills::default();
        assert_eq!(k.victory(Timestamp(0), &[]), None);
        k.quest(Some((42, 1)), Timestamp(10));
        assert_eq!(k.quest(None, Timestamp(1_000_000)).unwrap().target, (42, 1));
        let mut k = Kills::default();
        k.victory(Timestamp(0), &[]);
        k.quest(Some((42, 1)), Timestamp(10));
        assert_eq!(k.quest(None, Timestamp(1_000_001)), None);
    }
    #[test]
    fn oryx_requires_unique_normalized_active_target() {
        let mut k = Kills::default();
        assert!(
            k.oryx("synthetic_boss", Timestamp(1), &[boss(1)])
                .unwrap()
                .provisional
        );
        assert_eq!(k.oryx("other", Timestamp(1), &[boss(1)]), None);
        assert_eq!(
            k.oryx("Synthetic_Boss", Timestamp(1), &[boss(1), boss(2)]),
            None
        );
    }
    #[test]
    fn victory_windows_do_not_accept_stale_or_future_evidence() {
        let mut k = Kills::default();
        k.quest(Some((42, 1)), Timestamp(0));
        k.quest(None, Timestamp(1));
        assert_eq!(k.victory(Timestamp(1_000_002), &[]), None);
        assert_eq!(k.victory(Timestamp(10_000_001), &[boss(1)]), None);
        let mut k = Kills::default();
        let mut future = boss(1);
        future.last_hit = Some(Timestamp(100));
        assert_eq!(k.victory(Timestamp(99), &[future]), None);
        assert!(
            k.victory(Timestamp(10_000_000), &[boss(1)])
                .unwrap()
                .provisional
        );
    }
    #[test]
    fn announcement_pairs_require_unique_names_and_same_generation() {
        let mut k = Kills::default();
        k.oryx("Synthetic_Boss", Timestamp(10), &[boss(1)]);
        assert_eq!(k.removed((1, 2), Timestamp(20), None), None);
        let mut k = Kills::default();
        k.removed((1, 1), Timestamp(10), None);
        assert_eq!(k.oryx("Other_Boss", Timestamp(20), &[boss(1)]), None);
        assert_eq!(
            k.oryx("Synthetic_Boss", Timestamp(20), &[boss(1), boss(2)]),
            None
        );
        let mut next_generation = boss(1);
        next_generation.key.1 = 2;
        assert!(
            k.oryx("Synthetic_Boss", Timestamp(20), &[next_generation])
                .unwrap()
                .provisional
        );
    }

    #[test]
    fn hp_state_is_bounded_and_expiry_releases_capacity() {
        let mut k = Kills::default();
        for id in 0..100_001 {
            k.hp((id, 1), 0, Timestamp(0));
        }
        assert_eq!(k.zero_hp.len(), 100_000);
        assert_eq!(k.zero_expiry.len(), 100_000);
        k.hp((100_001, 1), 0, Timestamp(2_000_001));
        assert_eq!(k.zero_hp.len(), 1);
        assert_eq!(k.zero_expiry.len(), 1);
        k.hp((100_001, 1), 5, Timestamp(2_000_002));
        assert!(k.zero_hp.is_empty());
        assert!(k.zero_expiry.is_empty());
    }
}

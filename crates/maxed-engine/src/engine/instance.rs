//! Bounded per-map observations; public conversion lives in publish.rs.
use maxed_protocol::defs::MapInfo;
use maxed_types::{AreaKind, Timestamp};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AreaContext {
    pub instance_cap: Option<u16>,
    pub realm_name: Option<String>,
    pub entered_from_realm: bool,
    pub difficulty: Option<f64>,
    pub game_opened_epoch_s: Option<i64>,
    pub instance_age_micros: Option<u64>,
    pub modifiers: Vec<String>,
    pub grade: Option<String>,
}
impl AreaContext {
    pub fn from_map(p: &MapInfo, kind: &AreaKind, at: Timestamp) -> Self {
        let realm_name = p
            .realmName
            .strip_prefix("NexusPortal.")
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let opened = (p.gameOpenedTime > 0).then_some(i64::from(p.gameOpenedTime));
        Self {
            instance_cap: u16::try_from(p.maxPlayerCount).ok().filter(|v| *v > 0),
            entered_from_realm: realm_name.is_some() && *kind != AreaKind::Realm,
            realm_name,
            difficulty: (p.difficulty.is_finite() && p.difficulty >= 0.)
                .then_some(f64::from(p.difficulty)),
            game_opened_epoch_s: opened,
            instance_age_micros: opened.and_then(|s| at.0.checked_sub(s as u64 * 1_000_000)),
            modifiers: [
                &p.dungeonModifiers,
                &p.dungeonModifiers2,
                &p.dungeonModifiers3,
                &p.dungeonModifiers4,
            ]
            .into_iter()
            .filter_map(|s| s.as_ref().filter(|s| !s.is_empty()).cloned())
            .collect(),
            grade: p.dungeonGrade.clone().filter(|s| !s.is_empty()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RealmScore {
    pub current: i32,
    pub max: i32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuestTarget {
    pub object_id: i32,
    pub object_type: Option<u16>,
    pub since: Timestamp,
}
#[derive(Default)]
pub(crate) struct Instance {
    pub context: Option<AreaContext>,
    pub roster: BTreeSet<i32>,
    pub population_peak: Option<u32>,
    pub realm_score: Option<RealmScore>,
    score_max: Option<i32>,
    pub quest: Option<QuestTarget>,
    pub quest_generation: Option<u32>,
    pub previous_quest: Option<(QuestTarget, Timestamp)>,
    realm_visit: bool,
    visit_fame_entry: Option<(u64, Option<i32>)>,
}
impl Instance {
    pub fn enter(&mut self, p: &MapInfo, kind: &AreaKind, at: Timestamp) {
        *self = Self::default();
        self.realm_visit = *kind == AreaKind::Realm;
        self.context = Some(AreaContext::from_map(p, kind, at));
        self.score_max = (p.has_scores && p.maxRealmScore > 0).then_some(p.maxRealmScore);
        self.score_update(p.currentRealmScore);
    }
    pub fn observe_visit_fame(&mut self, epoch: u64, fame: Option<i32>) {
        if self.realm_visit {
            self.visit_fame_entry
                .get_or_insert((epoch, fame.filter(|v| *v >= 0)));
        }
    }
    pub fn visit_fame_delta(&self, epoch: u64, fame: Option<i32>) -> Option<i32> {
        if !self.realm_visit {
            return None;
        }
        let (entry_epoch, entry) = self.visit_fame_entry?;
        if entry_epoch != epoch {
            return None;
        }
        fame?.checked_sub(entry?).filter(|v| *v >= 0)
    }
    pub fn population(&self) -> Option<u32> {
        self.population_peak.map(|_| self.roster.len() as u32)
    }
    pub fn roster_update(&mut self, joins: impl IntoIterator<Item = i32>, leaves: &[i32]) {
        for id in joins {
            self.roster.insert(id);
            self.population_peak.get_or_insert(0);
        }
        for id in leaves {
            self.roster.remove(id);
        }
        if let Some(peak) = &mut self.population_peak {
            *peak = (*peak).max(self.roster.len() as u32);
        }
    }
    pub fn score_update(&mut self, score: i32) {
        self.realm_score = self.score_max.filter(|_| score >= 0).map(|max| RealmScore {
            current: score,
            max,
        });
    }
    pub fn quest_update(&mut self, id: i32, at: Timestamp) {
        if id == -1 {
            if let Some(q) = self.quest.take() {
                self.previous_quest = Some((q, at));
            }
        } else if id >= 0 && self.quest.as_ref().is_none_or(|q| q.object_id != id) {
            self.quest = Some(QuestTarget {
                object_id: id,
                object_type: None,
                since: at,
            });
            self.previous_quest = None;
        }
    }
    pub fn resolve_quest(&mut self, id: i32, object_type: u16) {
        if let Some(q) = &mut self.quest {
            if q.object_id == id {
                q.object_type = Some(object_type);
            }
        }
        if let Some((q, _)) = &mut self.previous_quest {
            if q.object_id == id {
                q.object_type = Some(object_type);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn map() -> MapInfo {
        MapInfo {
            width: 100,
            height: 100,
            name: "Synthetic Sanctum".into(),
            displayName: String::new(),
            realmName: "NexusPortal.SynthRealm".into(),
            fp: 1,
            seed: 1,
            background: 0,
            difficulty: 2.5,
            allowPlayerTeleport: false,
            noSave: false,
            showDisplays: false,
            maxPlayerCount: 50,
            gameOpenedTime: 10,
            versionNumber: String::new(),
            unknown1: 0,
            viewDistance: 0,
            unknown2: false,
            unknownInt: 0,
            modifiers: Default::default(),
            dungeonModifiers: Some("Synthetic modifier".into()),
            dungeonModifiers2: Some(String::new()),
            dungeonModifiers3: None,
            dungeonModifiers4: None,
            dungeonGrade: None,
            bgColor: 0,
            has_scores: true,
            maxRealmScore: 300000,
            currentRealmScore: 100,
        }
    }
    #[test]
    fn context_strips_realm_prefix_and_rejects_sentinels() {
        let mut p = map();
        let c = AreaContext::from_map(&p, &AreaKind::Dungeon, Timestamp(12_000_000));
        assert_eq!(c.realm_name.as_deref(), Some("SynthRealm"));
        assert!(c.entered_from_realm);
        assert_eq!(c.instance_age_micros, Some(2_000_000));
        assert_eq!(c.instance_cap, Some(50));
        assert_eq!(c.difficulty, Some(2.5));
        assert_eq!(c.modifiers, ["Synthetic modifier"]);
        assert!(!AreaContext::from_map(&p, &AreaKind::Realm, Timestamp(0)).entered_from_realm);
        assert_eq!(
            AreaContext::from_map(&p, &AreaKind::Realm, Timestamp(0)).instance_age_micros,
            None
        );
        p.realmName = "Nexus".into();
        p.maxPlayerCount = -1;
        p.gameOpenedTime = -1;
        p.difficulty = -1.;
        let c = AreaContext::from_map(&p, &AreaKind::Dungeon, Timestamp(0));
        assert_eq!(
            (
                c.realm_name,
                c.instance_cap,
                c.game_opened_epoch_s,
                c.difficulty
            ),
            (None, None, None, None)
        );
        p.difficulty = f32::NAN;
        assert_eq!(
            AreaContext::from_map(&p, &AreaKind::Dungeon, Timestamp(0)).difficulty,
            None
        );
    }
    #[test]
    fn roster_unknown_until_first_player_update() {
        let mut i = Instance::default();
        i.enter(&map(), &AreaKind::Dungeon, Timestamp(0));
        i.roster_update([], &[1]);
        assert_eq!(i.population(), None);
        i.roster_update([1], &[]);
        assert_eq!(i.population(), Some(1));
        i.roster_update([], &[1]);
        assert_eq!(i.population(), Some(0));
        i.enter(&map(), &AreaKind::Dungeon, Timestamp(0));
        assert_eq!(i.population(), None);
    }
    #[test]
    fn roster_counts_distant_joins_and_explicit_leaves() {
        let mut i = Instance::default();
        i.roster_update([10, 20, 10], &[]);
        i.roster_update([30], &[20, 999]);
        assert_eq!(i.population(), Some(2));
        assert_eq!(i.population_peak, Some(2));
        assert_eq!(i.roster, BTreeSet::from([10, 30]));
    }
    #[test]
    fn realm_score_update_is_absolute_not_delta() {
        let mut i = Instance::default();
        i.enter(&map(), &AreaKind::Realm, Timestamp(0));
        i.score_update(120);
        i.score_update(150);
        assert_eq!(
            i.realm_score,
            Some(RealmScore {
                current: 150,
                max: 300000
            })
        );
        i.score_update(-1);
        assert_eq!(i.realm_score, None);
        i.score_update(160);
        assert_eq!(i.realm_score.as_ref().unwrap().current, 160);
        let mut p = map();
        p.currentRealmScore = -1;
        p.maxRealmScore = -1;
        i.enter(&p, &AreaKind::Dungeon, Timestamp(0));
        i.score_update(10);
        assert_eq!(i.realm_score, None);
        p.maxRealmScore = 300000;
        i.enter(&p, &AreaKind::Realm, Timestamp(0));
        assert_eq!(i.realm_score, None);
        i.score_update(10);
        assert_eq!(i.realm_score.as_ref().unwrap().current, 10);
    }
    #[test]
    fn quest_preserves_clear_time_and_resolves_late_object() {
        let mut i = Instance::default();
        i.quest_update(42, Timestamp(1));
        i.resolve_quest(42, 300);
        i.quest_update(42, Timestamp(2));
        i.quest_update(-1, Timestamp(3));
        let (q, at) = i.previous_quest.unwrap();
        assert_eq!(
            (q.object_id, q.object_type, q.since, at),
            (42, Some(300), Timestamp(1), Timestamp(3))
        );
    }
}

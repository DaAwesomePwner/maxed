//! Caller supplies one run's fights. Realm fights are always excluded.
use maxed_types::{FightView, RunPlayerTotal, RunTotals};
pub const RUN_TOTAL_PLAYER_LIMIT: usize = 64;
pub fn run_totals(fights: &[FightView], fame: Option<(i32, i32)>) -> RunTotals {
    run_totals_iter(fights.iter(), fame)
}
pub(crate) fn run_totals_iter<'a>(
    fights: impl IntoIterator<Item = &'a FightView>,
    fame: Option<(i32, i32)>,
) -> RunTotals {
    let mut totals = RunTotals {
        fame_delta: fame.and_then(|(first, last)| last.checked_sub(first)),
        fights: 0,
        kills: 0,
        first_hit: None,
        last_hit: None,
        total_damage: 0.0,
        unattributed_damage: 0.0,
        players: Vec::new(),
        local: None,
        partial: false,
    };
    let mut players = std::collections::BTreeMap::<String, RunPlayerTotal>::new();
    // The signature has no run argument. The first dungeon fight establishes
    // the scope; callers normally pass an already filtered run's fights.
    let mut run_id = None;
    for f in fights {
        let Some(id) = f.run_id.as_ref() else {
            continue;
        };
        let expected = run_id.get_or_insert(id);
        if id != *expected {
            continue;
        }
        totals.fights = totals.fights.saturating_add(1);
        totals.kills = totals.kills.saturating_add(u32::from(f.kill.is_some()));
        totals.first_hit = totals.first_hit.into_iter().chain(f.first_hit).min();
        totals.last_hit = totals.last_hit.into_iter().chain(f.last_hit).max();
        totals.total_damage += f.total_damage;
        totals.unattributed_damage += f.unattributed_damage;
        totals.partial |= !f.complete_capture
            || !f.issues.is_empty()
            || f.players
                .iter()
                .flat_map(|p| &p.sources)
                .any(|s| s.estimated);
        for p in &f.players {
            let row = players
                .entry(p.player_id.clone())
                .or_insert_with(|| RunPlayerTotal {
                    player_id: p.player_id.clone(),
                    name: None,
                    class_id: None,
                    is_local: false,
                    damage: 0.0,
                    hits: 0,
                    deaths: 0,
                    share: None,
                    rank: None,
                    dps: None,
                });
            if p.name.is_some() {
                row.name.clone_from(&p.name);
            }
            if p.class_id.is_some() {
                row.class_id = p.class_id;
            }
            row.is_local |= p.is_local;
            row.damage += p.damage;
            row.hits = row.hits.saturating_add(p.hits);
            row.deaths = row.deaths.saturating_add(p.deaths);
        }
    }
    finalize(totals, players.into_values().collect())
}
fn finalize(totals: RunTotals, mut players: Vec<RunPlayerTotal>) -> RunTotals {
    players.sort_by(|a, b| {
        b.damage
            .total_cmp(&a.damage)
            .then_with(|| a.player_id.cmp(&b.player_id))
    });
    finalize_sorted(totals, players)
}
fn finalize_sorted(mut totals: RunTotals, mut players: Vec<RunPlayerTotal>) -> RunTotals {
    let mut last = None;
    let mut rank = 0;
    for (i, p) in players.iter_mut().enumerate() {
        p.share = crate::share(p.damage, totals.total_damage);
        p.dps = totals
            .first_hit
            .zip(totals.last_hit)
            .and_then(|(first, last)| crate::dps(p.damage, first, last));
        if p.damage > 0. {
            if last != Some(p.damage) {
                rank = i as u32 + 1;
                last = Some(p.damage);
            }
            p.rank = Some(rank);
        }
    }
    totals.local = players.iter().find(|p| p.is_local).cloned();
    players.truncate(RUN_TOTAL_PLAYER_LIMIT);
    if let Some(local) = &totals.local {
        if !players.iter().any(|p| p.player_id == local.player_id) {
            if players.len() == RUN_TOTAL_PLAYER_LIMIT {
                players.pop();
            }
            players.push(local.clone());
        }
    }
    totals.players = players;
    totals
}

// Store only each fight's compact totals contribution, never its sources or
// series. Fight mutation/drop directly invalidates its key; unchanged fights
// are never inspected on the warm or dirty path. Metadata keeps fight-key order
// so replacing/removing a fight preserves the pure reducer's precedence.
use crate::fights::FightKey;
use maxed_types::Timestamp;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone)]
struct Contribution {
    total: f64,
    unattributed: f64,
    first: Option<Timestamp>,
    last: Option<Timestamp>,
    kill: bool,
    partial: bool,
    players: Vec<PlayerContribution>,
}
#[derive(Clone)]
struct PlayerContribution {
    player_id: String,
    name: Option<String>,
    class_id: Option<u16>,
    is_local: bool,
    damage: f64,
    hits: u32,
    deaths: u32,
}
impl Contribution {
    fn new(f: &FightView) -> Self {
        Self {
            total: f.total_damage,
            unattributed: f.unattributed_damage,
            first: f.first_hit,
            last: f.last_hit,
            kill: f.kill.is_some(),
            partial: !f.complete_capture
                || !f.issues.is_empty()
                || f.players
                    .iter()
                    .flat_map(|p| &p.sources)
                    .any(|s| s.estimated),
            players: f
                .players
                .iter()
                .map(|p| PlayerContribution {
                    player_id: p.player_id.clone(),
                    name: p.name.clone(),
                    class_id: p.class_id,
                    is_local: p.is_local,
                    damage: p.damage,
                    hits: p.hits,
                    deaths: p.deaths,
                })
                .collect(),
        }
    }
}
struct PlayerAggregate {
    row: std::sync::Arc<RunPlayerTotal>,
    damage: f64,
    hits: u64,
    deaths: u64,
    locals: usize,
    members: usize,
    names: BTreeMap<FightKey, usize>,
    classes: BTreeMap<FightKey, usize>,
}
impl PlayerAggregate {
    fn new(id: &str) -> Self {
        Self {
            row: std::sync::Arc::new(RunPlayerTotal {
                player_id: id.into(),
                name: None,
                class_id: None,
                is_local: false,
                damage: 0.,
                hits: 0,
                deaths: 0,
                share: None,
                rank: None,
                dps: None,
            }),
            damage: 0.,
            hits: 0,
            deaths: 0,
            locals: 0,
            members: 0,
            names: BTreeMap::new(),
            classes: BTreeMap::new(),
        }
    }
}
// Maintain damage order as contributions change; snapshot finalization never sorts.
#[derive(Clone)]
struct RankKey(std::sync::Arc<RunPlayerTotal>);
impl PartialEq for RankKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for RankKey {}
impl PartialOrd for RankKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for RankKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .0
            .damage
            .total_cmp(&self.0.damage)
            .then_with(|| self.0.player_id.cmp(&other.0.player_id))
    }
}
#[derive(Default)]
pub(crate) struct RunTotalsCache {
    run_id: Option<String>,
    contributions: BTreeMap<FightKey, Contribution>,
    pending: BTreeSet<FightKey>,
    ranked: BTreeSet<RankKey>,
    players: BTreeMap<String, PlayerAggregate>,
    first: BTreeMap<Timestamp, usize>,
    last: BTreeMap<Timestamp, usize>,
    damage: f64,
    unattributed: f64,
    kills: u32,
    partial: usize,
    view: Option<RunTotals>,
    #[cfg(test)]
    pub rebuilds: u64,
    #[cfg(test)]
    pub contribution_builds: u64,
    #[cfg(test)]
    pub profile: [std::time::Duration; 5],
    #[cfg(test)]
    pub fight_lookups: u64,
}
fn timestamp_count(counts: &mut BTreeMap<Timestamp, usize>, at: Option<Timestamp>, add: bool) {
    if let Some(at) = at {
        if add {
            *counts.entry(at).or_default() += 1;
        } else if let Some(n) = counts.get_mut(&at) {
            *n -= 1;
            if *n == 0 {
                counts.remove(&at);
            }
        }
    }
}
impl RunTotalsCache {
    #[cfg(test)]
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub(crate) fn invalidate(&mut self, key: FightKey) {
        self.pending.insert(key);
    }
    pub(crate) fn forget(&mut self, key: FightKey) {
        if self.contributions.contains_key(&key) {
            self.pending.insert(key);
        } else {
            self.pending.remove(&key);
        }
    }
    fn adjust(&mut self, key: FightKey, c: &Contribution, add: bool) {
        let sign = if add { 1. } else { -1. };
        self.damage += sign * c.total;
        self.unattributed += sign * c.unattributed;
        if add {
            self.kills += u32::from(c.kill);
            self.partial += usize::from(c.partial);
        } else {
            self.kills -= u32::from(c.kill);
            self.partial -= usize::from(c.partial);
        }
        timestamp_count(&mut self.first, c.first, add);
        timestamp_count(&mut self.last, c.last, add);
        for (index, p) in c.players.iter().enumerate() {
            if add {
                self.players
                    .entry(p.player_id.clone())
                    .or_insert_with(|| PlayerAggregate::new(&p.player_id));
            }
            let Some(a) = self.players.get_mut(&p.player_id) else {
                continue;
            };
            self.ranked.remove(&RankKey(a.row.clone()));
            if add {
                a.damage += p.damage;
                a.hits += u64::from(p.hits);
                a.deaths += u64::from(p.deaths);
                a.locals += usize::from(p.is_local);
                a.members += 1;
                if p.name.is_some() {
                    a.names.insert(key, index);
                }
                if p.class_id.is_some() {
                    a.classes.insert(key, index);
                }
            } else {
                a.damage -= p.damage;
                a.hits -= u64::from(p.hits);
                a.deaths -= u64::from(p.deaths);
                a.locals -= usize::from(p.is_local);
                a.members -= 1;
                a.names.remove(&key);
                a.classes.remove(&key);
            }
            if a.members == 0 {
                self.players.remove(&p.player_id);
                continue;
            }
            // Metadata lookups are paid only for affected players, never by
            // snapshot finalization. Indexes avoid searching another fight's rows.
            let name = a.names.last_key_value().and_then(|(source, &index)| {
                if *source == key && add {
                    p.name.as_ref()
                } else {
                    self.contributions
                        .get(source)?
                        .players
                        .get(index)?
                        .name
                        .as_ref()
                }
            });
            let class_id = a.classes.last_key_value().and_then(|(source, &index)| {
                if *source == key && add {
                    p.class_id
                } else {
                    self.contributions.get(source)?.players.get(index)?.class_id
                }
            });
            // The ranking entry was removed before mutation; only this aggregate
            // owns the Arc, so make_mut reuses the row and its allocations.
            let row = std::sync::Arc::make_mut(&mut a.row);
            match (name, &mut row.name) {
                (Some(name), Some(old)) => old.clone_from(name),
                (name, old) => *old = name.cloned(),
            }
            row.class_id = class_id;
            row.damage = a.damage;
            row.hits = a.hits.min(u64::from(u32::MAX)) as u32;
            row.deaths = a.deaths.min(u64::from(u32::MAX)) as u32;
            row.is_local = a.locals > 0;
            self.ranked.insert(RankKey(a.row.clone()));
        }
    }
    fn finish(&self, mut players: Vec<RunPlayerTotal>) -> RunTotals {
        let totals = RunTotals {
            fame_delta: None,
            fights: self.contributions.len() as u32,
            kills: self.kills,
            first_hit: self.first.first_key_value().map(|(&t, _)| t),
            last_hit: self.last.last_key_value().map(|(&t, _)| t),
            total_damage: self.damage,
            unattributed_damage: self.unattributed,
            players: Vec::new(),
            local: None,
            partial: self.partial > 0,
        };
        players.truncate(self.ranked.len());
        for (index, key) in self.ranked.iter().enumerate() {
            let row = key.0.as_ref();
            if let Some(old) = players.get_mut(index) {
                old.player_id.clone_from(&row.player_id);
                old.name.clone_from(&row.name);
                old.class_id = row.class_id;
                old.is_local = row.is_local;
                old.damage = row.damage;
                old.hits = row.hits;
                old.deaths = row.deaths;
                old.share = None;
                old.rank = None;
                old.dps = None;
            } else {
                players.push(row.clone());
            }
        }
        finalize_sorted(totals, players)
    }
}
impl crate::Engine {
    pub(super) fn snapshot_run_totals(&self) -> Option<RunTotals> {
        let run = self.run.as_ref()?;
        let mut cache = self
            .run_totals_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        #[cfg(test)]
        let discovery = std::time::Instant::now();
        #[cfg(test)]
        {
            cache.profile = Default::default();
        }
        if cache.run_id.as_ref() != Some(&run.view.id) {
            *cache = RunTotalsCache {
                run_id: Some(run.view.id.clone()),
                pending: self.fights.keys().copied().collect(),
                ..Default::default()
            };
        }
        if cache.view.is_none() || !cache.pending.is_empty() {
            let dirty = std::mem::take(&mut cache.pending);
            #[cfg(test)]
            {
                cache.profile[0] = discovery.elapsed();
            }
            #[cfg(test)]
            let updates = std::time::Instant::now();
            for key in dirty {
                #[cfg(test)]
                {
                    cache.fight_lookups += 1;
                }
                if let Some(previous) = cache.contributions.remove(&key) {
                    cache.adjust(key, &previous, false);
                }
                if let Some(f) = self
                    .fights
                    .get(&key)
                    .filter(|f| f.base.run_id.as_ref() == Some(&run.view.id))
                {
                    #[cfg(test)]
                    let projection = std::time::Instant::now();
                    let next = Contribution::new(f.projected());
                    #[cfg(test)]
                    {
                        cache.profile[1] += projection.elapsed();
                    }
                    cache.adjust(key, &next, true);
                    cache.contributions.insert(key, next);
                    #[cfg(test)]
                    {
                        cache.contribution_builds += 1;
                    }
                }
            }
            #[cfg(test)]
            {
                cache.profile[2] = updates.elapsed().saturating_sub(cache.profile[1]);
            }
            #[cfg(test)]
            let finalize = std::time::Instant::now();
            let reusable = cache.view.take().map(|v| v.players).unwrap_or_default();
            cache.view = Some(cache.finish(reusable));
            #[cfg(test)]
            {
                cache.profile[3] = finalize.elapsed();
                cache.rebuilds += 1;
            }
        }
        #[cfg(test)]
        let clone = std::time::Instant::now();
        let mut view = cache.view.clone();
        if let Some(view) = &mut view {
            view.fame_delta = run
                .fame_samples
                .filter(|(create, _, _)| *create == self.create_number)
                .and_then(|(_, first, last)| last?.checked_sub(first));
        }
        #[cfg(test)]
        {
            cache.profile[4] = clone.elapsed();
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::Timestamp;
    fn fight() -> FightView {
        maxed_types::demo::demo_snapshot().fights.remove(0)
    }
    #[test]
    fn run_totals_rank_share_from_full_rows() {
        let mut f = fight();
        let template = f.players[0].clone();
        f.players.clear();
        f.total_damage = 0.;
        for n in 0..70 {
            let mut p = template.clone();
            p.player_id = format!("synthetic-{n:02}");
            p.damage = (70 - n) as f64;
            p.is_local = n == 69;
            f.total_damage += p.damage;
            f.players.push(p);
        }
        let totals = run_totals(&[f.clone()], Some((10, 15)));
        assert_eq!(totals.players.len(), 64);
        let local = totals.local.unwrap();
        assert_eq!(local.rank, Some(70));
        assert_eq!(local.share, Some(1. / f.total_damage));
        assert!(totals.players.iter().any(|p| p.is_local));
        assert_eq!(totals.fame_delta, Some(5));
        assert_eq!(local.dps, Some(0.5));
    }
    #[test]
    fn run_totals_excludes_realm_fights() {
        let f = fight();
        let mut realm = f.clone();
        realm.run_id = None;
        realm.total_damage = 9999.;
        let totals = run_totals(&[f.clone(), realm], None);
        assert_eq!(totals.fights, 1);
        assert_eq!(totals.total_damage, f.total_damage);
        assert_eq!(totals.fame_delta, None);
    }
    #[test]
    fn totals_merge_players_and_use_encounter_span() {
        let mut a = fight();
        a.first_hit = Some(Timestamp(0));
        a.last_hit = Some(Timestamp(1_000_000));
        let mut b = a.clone();
        b.first_hit = Some(Timestamp(3_000_000));
        b.last_hit = Some(Timestamp(4_000_000));
        let total = run_totals(&[a, b], None);
        assert_eq!(total.players.len(), 1);
        assert_eq!(total.players[0].damage, 600.);
        assert_eq!(total.players[0].dps, Some(150.));
        assert!(total.partial);
    }
}

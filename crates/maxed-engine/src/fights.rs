use crate::{dps, share, Engine};
use maxed_types::*;
use std::collections::BTreeMap;

#[derive(Clone)]
pub(crate) struct Contribution {
    pub player: Option<PlayerDamage>,
    pub source: SourceDamage,
    pub at: Timestamp,
    pub active_at: Timestamp,
}
pub(crate) type FightKey = (i32, u32);
pub(crate) type PairKey = (i32, u32, u64, u16);
pub(crate) struct Pair {
    pub estimate: Option<Contribution>,
    pub estimates: u32,
    pub servers: u32,
    pub server_damage: Option<f64>,
    // Keep the first local server time replaceable until the pair settles.
    server_activity: Option<(String, Timestamp)>,
    pub at: Timestamp,
}
pub(crate) struct Fight {
    pub base: FightView,
    pub was_quest: bool,
    pub pending: BTreeMap<(i32, u32, u64, u16), Pair>,
    pub series: crate::engine::series::Series,
    pub series_budget: crate::engine::series::SeriesBudget,
    pub timeline_out: Vec<(Timestamp, PlayerDamage, f64)>,
    local_last_hit: Option<Timestamp>,
    player_windows: BTreeMap<String, (Timestamp, Timestamp)>,
    snapshot_series_cache:
        std::sync::Arc<std::sync::Mutex<crate::engine::snapshot_series::SnapshotSeriesCache>>,
    totals_cache: std::sync::Arc<std::sync::Mutex<crate::engine::totals::RunTotalsCache>>,
    projection: std::sync::OnceLock<FightView>,
    #[cfg(test)]
    pub projection_builds: std::sync::atomic::AtomicU64,
    #[cfg(test)]
    pub series_nanos: std::sync::atomic::AtomicU64,
}
impl Drop for Fight {
    fn drop(&mut self) {
        self.totals_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .forget((self.base.target_id, self.base.target_generation));
        self.snapshot_series_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .forget((self.base.target_id, self.base.target_generation));
    }
}
impl Fight {
    pub(crate) fn latest_local_hit(&self) -> Option<Timestamp> {
        self.local_last_hit
            .into_iter()
            .chain(
                self.pending
                    .values()
                    .filter(|p| p.estimates == 1 && p.servers == 0)
                    .filter_map(|p| p.estimate.as_ref())
                    .filter(|c| c.player.as_ref().is_some_and(|p| p.is_local))
                    .map(|c| c.active_at),
            )
            .max()
    }
    fn accept(&mut self, c: &Contribution) {
        if c.player.as_ref().is_some_and(|p| p.is_local) {
            self.local_last_hit = self
                .local_last_hit
                .into_iter()
                .chain(Some(c.active_at))
                .max();
        }
        add(&mut self.base, &mut self.series, c);
        if let Some(player) = &c.player {
            self.timeline_out
                .push((c.at, player.clone(), c.source.damage));
        }
    }
    fn record_activity(&mut self, c: &Contribution) {
        if let Some(p) = &c.player {
            if self
                .base
                .players
                .iter()
                .any(|row| row.player_id == p.player_id)
            {
                activity(&mut self.player_windows, &p.player_id, c.active_at);
            }
        }
    }
    pub(crate) fn project_snapshot(&self, include_series: bool) -> FightView {
        if include_series {
            return self.project();
        }
        let v = self.projected();
        // Clone the legacy fields without allocating/copying series that the
        // snapshot will omit. Event callers continue to use project().
        FightView {
            owner_character_id: v.owner_character_id.clone(),
            scope_id: v.scope_id.clone(),
            dps_basis: v.dps_basis,
            is_current_quest: v.is_current_quest,
            local_last_hit: v.local_last_hit,
            local_active_micros: v.local_active_micros,
            id: v.id.clone(),
            run_id: v.run_id.clone(),
            session_id: v.session_id.clone(),
            target_id: v.target_id,
            target_generation: v.target_generation,
            boss_type: v.boss_type,
            target_type: v.target_type,
            max_hp: v.max_hp,
            started: v.started,
            ended: v.ended,
            first_hit: v.first_hit,
            last_hit: v.last_hit,
            end_reason: v.end_reason.clone(),
            players: v.players.clone(),
            total_damage: v.total_damage,
            unattributed_damage: v.unattributed_damage,
            buckets: v.buckets.clone(),
            complete_capture: v.complete_capture,
            pending_local_reconciliation: v.pending_local_reconciliation,
            issues: v.issues.clone(),
            kill: v.kill.clone(),
            target_hp: v.target_hp,
            target_kind: v.target_kind.clone(),
            player_series: None,
        }
    }
    pub fn project(&self) -> FightView {
        self.projected().clone()
    }
    pub fn projected(&self) -> &FightView {
        self.projection.get_or_init(|| {
            #[cfg(test)]
            self.projection_builds
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.project_uncached(true)
        })
    }
    pub fn changed(&mut self) {
        self.totals_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .invalidate((self.base.target_id, self.base.target_generation));
        self.snapshot_series_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .invalidate((self.base.target_id, self.base.target_generation));
        self.projection.take();
    }
    pub(crate) fn project_uncached(&self, include_series: bool) -> FightView {
        let mut v = self.base.clone();
        v.pending_local_reconciliation = self.pending.values().any(|p| {
            (p.estimates > 0
                && p.servers == 0
                && p.estimate
                    .as_ref()
                    .and_then(|c| c.player.as_ref())
                    .is_some_and(|p| p.is_local))
                || (p.estimates == 0 && p.servers > 0 && p.server_activity.is_some())
        });
        v.local_last_hit = self.latest_local_hit();
        let mut series = std::borrow::Cow::Borrowed(&self.series);
        let mut windows = std::borrow::Cow::Borrowed(&self.player_windows);
        for p in self.pending.values() {
            if let Some((id, at)) = &p.server_activity {
                activity(windows.to_mut(), id, *at);
            }
            if p.estimates == 1 && p.servers == 0 {
                if let Some(c) = &p.estimate {
                    add(&mut v, series.to_mut(), c);
                    if let Some(player) = &c.player {
                        if v.players.iter().any(|p| p.player_id == player.player_id) {
                            activity(windows.to_mut(), &player.player_id, c.active_at);
                        }
                    }
                }
            }
        }
        v.players.sort_by(|a, b| {
            b.damage
                .total_cmp(&a.damage)
                .then_with(|| a.player_id.cmp(&b.player_id))
        });
        let mut locals = v.players.iter().filter(|p| p.is_local);
        v.local_active_micros = locals
            .next()
            .filter(|_| locals.next().is_none())
            .filter(|_| !v.issues.contains(&CoverageIssue::MissingIdentity))
            .and_then(|p| windows.get(&p.player_id))
            .map(|&(first, last)| {
                let last = v.ended.map_or(last, |end| last.min(end));
                last.0.saturating_sub(first.0)
            });
        let mut last = None;
        let mut rank = 0;
        for (i, p) in v.players.iter_mut().enumerate() {
            p.dps = windows.get(&p.player_id).and_then(|&(first, last)| {
                let last = v.ended.map_or(last, |end| last.min(end));
                (last.0.saturating_sub(first.0) >= 3_000_000)
                    .then(|| dps(p.damage, first, last))
                    .flatten()
            });
            p.share = share(p.damage, v.total_damage);
            if p.damage > 0. {
                if last != Some(p.damage) {
                    rank = i as u32 + 1;
                    last = Some(p.damage);
                }
                p.rank = Some(rank);
            } else {
                p.rank = None;
            }
        }
        if self.series_budget.disabled
            || series.is_disabled()
            || v.players.iter().filter(|p| p.is_local).count() != 1
        {
            for bucket in &mut v.buckets {
                bucket.others_damage = None;
                bucket.others_presence = None;
            }
        }
        #[cfg(test)]
        let series_at = std::time::Instant::now();
        if include_series && !self.series_budget.disabled {
            v.player_series = series
                .project_selected(|id| {
                    v.players
                        .iter()
                        .take(5)
                        .chain(v.players.iter().filter(|p| p.is_local))
                        .any(|p| p.player_id == id)
                })
                .map(|rows| {
                    rows.into_iter()
                        .map(|row| PlayerSeries {
                            player_id: row.player_id,
                            object_id: row.object_id,
                            is_local: row.is_local,
                            points: row.points,
                        })
                        .collect()
                });
            if let Some(rows) = &mut v.player_series {
                for p in v
                    .players
                    .iter()
                    .take(5)
                    .chain(v.players.iter().filter(|p| p.is_local))
                {
                    if !rows.iter().any(|s| s.player_id == p.player_id) {
                        rows.push(PlayerSeries {
                            player_id: p.player_id.clone(),
                            object_id: p.object_id,
                            is_local: p.is_local,
                            points: vec![0.; v.buckets.len()],
                        });
                    }
                }
            }
        }
        #[cfg(test)]
        self.series_nanos.store(
            series_at.elapsed().as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        v
    }
    fn latch_projected_series_cap(&mut self) {
        if self.series_budget.disabled {
            return;
        }
        let (players, buckets) = self.series.dimensions();
        // Most fights cannot reach the bound even if every pending pair adds
        // a player and bucket. Only materialize a budget near the threshold.
        if players
            .saturating_add(self.pending.len())
            .saturating_mul(buckets.saturating_add(self.pending.len()))
            <= crate::engine::series::SERIES_CELL_LIMIT
        {
            return;
        }
        let mut budget = self.series.budget();
        for pair in self
            .pending
            .values()
            .filter(|p| p.estimates == 1 && p.servers == 0)
        {
            if let Some(c) = &pair.estimate {
                budget.observe(c.at, c.player.as_ref());
            }
        }
        if budget.disabled {
            self.changed();
            self.series_budget.disabled = true;
            self.series.disable();
        }
    }
    pub fn settle_pair(&mut self, key: PairKey) {
        if let Some(p) = self.pending.remove(&key) {
            self.changed();
            if let Some((id, at)) = p.server_activity {
                activity(&mut self.player_windows, &id, at);
            }
            if p.estimates == 1 && p.servers == 0 {
                if let Some(c) = p.estimate {
                    self.accept(&c);
                    self.record_activity(&c);
                }
            }
        }
    }
}

fn bucket(
    v: &mut FightView,
    c: &Contribution,
    others_damage: Option<f64>,
    others_presence: Option<f64>,
) {
    let at = c.at.0 / 1_000_000 * 1_000_000;
    let local_known = v.players.iter().any(|p| p.is_local);
    let local = c.player.as_ref().is_some_and(|p| p.is_local);
    if let Some(b) =
        crate::engine::series::find_bucket(&v.buckets, Timestamp(at), |b| (b.from, b.until))
            .map(|index| &mut v.buckets[index])
    {
        b.total += c.source.damage;
        if local_known && b.local.is_none() {
            b.local = Some(0.);
        }
        if local {
            b.local = Some(b.local.unwrap_or(0.) + c.source.damage);
        }
        b.others_damage = b.others_damage.zip(others_damage).map(|(a, b)| a + b);
        b.others_presence = b.others_presence.zip(others_presence).map(|(a, b)| a + b);
    } else {
        // Same position a stable sort after pushing would give: after any equal `from`.
        let insert = v.buckets.partition_point(|b| b.from.0 <= at);
        v.buckets.insert(
            insert,
            DamageBucket {
                from: Timestamp(at),
                until: Timestamp(at.saturating_add(1_000_000)),
                total: c.source.damage,
                local: local_known.then_some(if local { c.source.damage } else { 0. }),
                others_damage,
                others_presence,
            },
        );
    }
    if v.buckets.len() > 1800 {
        v.buckets = v
            .buckets
            .chunks(2)
            .map(|pair| {
                let a = &pair[0];
                let b = pair.last().expect("nonempty pair");
                DamageBucket {
                    from: a.from,
                    until: b.until,
                    total: pair.iter().map(|b| b.total).sum(),
                    local: pair
                        .iter()
                        .all(|b| b.local.is_some())
                        .then(|| pair.iter().map(|b| b.local.unwrap_or(0.)).sum()),
                    others_damage: pair.iter().map(|b| b.others_damage).sum(),
                    others_presence: pair.iter().map(|b| b.others_presence).sum(),
                }
            })
            .collect();
    }
}

fn activity(windows: &mut BTreeMap<String, (Timestamp, Timestamp)>, id: &str, at: Timestamp) {
    windows
        .entry(id.into())
        .and_modify(|(first, last)| {
            *first = (*first).min(at);
            *last = (*last).max(at);
        })
        .or_insert((at, at));
}

fn add(v: &mut FightView, series: &mut crate::engine::series::Series, c: &Contribution) {
    let other = c.player.as_ref().filter(|p| !p.is_local);
    // Check before insertion/compaction: a cell witnesses the first hit by
    // this player in the one-second bucket, regardless of later hit count.
    let first_other = other.is_some_and(|p| !series.has_cell(c.at, &p.player_id));
    series.add(c.at, c.player.as_ref(), c.source.damage);
    let others_known = !series.is_disabled();
    let others_damage = others_known.then_some(if other.is_some() { c.source.damage } else { 0. });
    let others_presence = others_known.then_some(if first_other { 1. } else { 0. });
    let damage = c.source.damage;
    v.total_damage += damage;
    if let Some(player) = &c.player {
        let p = if let Some(i) = v
            .players
            .iter()
            .position(|p| p.player_id == player.player_id)
        {
            &mut v.players[i]
        } else {
            // Bound projection cardinality independently of map duration.
            if v.players.len() >= 4096 {
                v.unattributed_damage += damage;
                v.complete_capture = false;
                if !v.issues.contains(&CoverageIssue::ResourceLimit) {
                    v.issues.push(CoverageIssue::ResourceLimit);
                }
                bucket(v, c, others_known.then_some(0.), others_known.then_some(0.));
                return;
            }
            v.players.push(player.clone());
            v.players.last_mut().expect("pushed player")
        };
        p.damage += damage;
        p.hits = p.hits.saturating_add(c.source.hits);
        p.biggest_hit = Some(p.biggest_hit.unwrap_or(0.).max(damage));
        if let Some(s) = p.sources.iter_mut().find(|s| {
            s.item_type == c.source.item_type
                && s.kind == c.source.kind
                && s.enchant == c.source.enchant
                && s.estimated == c.source.estimated
                && s.reasons == c.source.reasons
        }) {
            s.damage += damage;
            s.hits = s.hits.saturating_add(c.source.hits);
            s.biggest_hit = Some(s.biggest_hit.unwrap_or(0.).max(damage));
        } else if p.sources.len() < 255 {
            p.sources.push(c.source.clone());
        } else {
            v.complete_capture = false;
            if !v.issues.contains(&CoverageIssue::ResourceLimit) {
                v.issues.push(CoverageIssue::ResourceLimit);
            }
            if let Some(s) = p.sources.iter_mut().find(|s| {
                s.reasons
                    .iter()
                    .any(|r| r == "resource-limit-source-summary")
            }) {
                s.damage += damage;
                s.hits = s.hits.saturating_add(c.source.hits);
                s.biggest_hit = Some(s.biggest_hit.unwrap_or(0.).max(damage));
                s.estimated |= c.source.estimated;
            } else {
                p.sources.push(SourceDamage {
                    item_type: None,
                    ability: false,
                    kind: SourceKind::Unknown,
                    enchant: crate::parse_enchant(None),
                    damage,
                    hits: c.source.hits,
                    biggest_hit: Some(damage),
                    estimated: c.source.estimated,
                    reasons: vec!["resource-limit-source-summary".into()],
                });
            }
        }
    } else {
        v.unattributed_damage += damage;
    }
    bucket(v, c, others_damage, others_presence);
}

impl Engine {
    pub(crate) fn player_row(&self, id: i32) -> Option<PlayerDamage> {
        if !self.is_player(id) {
            return None;
        }
        let e = self.entities.get(&id)?;
        let is_local = self.local.as_ref().is_some_and(|l| l.object == id);
        let player_id = if is_local {
            self.character
                .as_ref()
                .map(|c| c.id.clone())
                .unwrap_or_else(|| self.id("local", self.map_ordinal))
        } else {
            format!(
                "{}:player:{}:{id}:{}",
                self.config.stream_id, self.map_ordinal, e.generation
            )
        };
        Some(PlayerDamage {
            object_id: id,
            player_id,
            // Display only; player_id and character aliasing retain their existing inputs.
            name: e
                .name
                .as_deref()
                .map(|name| crate::names::display_name(name).to_owned()),
            class_id: e.object_type,
            is_local,
            damage: 0.,
            hits: 0,
            biggest_hit: None,
            deaths: 0,
            dps: None,
            share: None,
            rank: None,
            sources: vec![],
        })
    }
    pub(crate) fn merge_player_identity(&mut self, old: &str, new: &str) {
        if let Some(scope) = &mut self.scope {
            scope.merge_identity(old, new);
        }
        for (key, f) in &mut self.fights {
            self.dirty_fights.insert(*key);
            f.changed();
            f.series.merge_identity(old, new);
            f.series_budget.merge_identity(old, new);
            for (_, player, _) in &mut f.timeline_out {
                if player.player_id == old {
                    player.player_id = new.into();
                }
            }
            if let Some((first, last)) = f.player_windows.remove(old) {
                f.player_windows
                    .entry(new.into())
                    .and_modify(|window| {
                        window.0 = window.0.min(first);
                        window.1 = window.1.max(last);
                    })
                    .or_insert((first, last));
            }
            for p in &mut f.base.players {
                if p.player_id == old {
                    p.player_id = new.into();
                }
            }
            for pair in f.pending.values_mut() {
                if let Some((id, _)) = &mut pair.server_activity {
                    if id == old {
                        *id = new.into();
                    }
                }
                if let Some(c) = &mut pair.estimate {
                    if let Some(p) = &mut c.player {
                        if p.player_id == old {
                            p.player_id = new.into();
                        }
                    }
                }
            }
        }
    }
    pub(crate) fn retire_fight(
        &mut self,
        key: (i32, u32),
        reason: FightEnd,
        events: &mut Vec<EngineEvent>,
    ) {
        self.dirty_fights.remove(&key);
        self.observed_kills.remove(&key);
        self.notices.loot_kills.remove(&key);
        // Settlement can accept new damage. Drain it while the fight still
        // belongs to the engine so retirement cannot discard timeline entries.
        if let Some(f) = self.fights.get_mut(&key) {
            for (pk, p) in &f.pending {
                self.pair_expiry.remove(&(p.at, key, *pk));
            }
            let pairs: Vec<_> = f.pending.keys().copied().collect();
            for pair in pairs {
                f.settle_pair(pair);
            }
        }
        self.drain_fight_timelines();
        if let Some(f) = self.fights.remove(&key) {
            let mut v = f.project();
            v.ended.get_or_insert(crate::clock::millis(self.now));
            v.is_current_quest = false;
            let resource_limit = reason == FightEnd::ResourceLimit;
            let killed = v.kill.as_ref().is_some_and(|k| !k.provisional);
            let interrupted = !killed && !matches!(reason, FightEnd::MapChanged | FightEnd::Killed);
            let reason = if killed { FightEnd::Killed } else { reason };
            v.end_reason = Some(reason.clone());
            if interrupted {
                v.complete_capture = false;
            }
            if resource_limit {
                v.complete_capture = false;
                if !v.issues.contains(&CoverageIssue::ResourceLimit) {
                    v.issues.push(CoverageIssue::ResourceLimit);
                }
            }
            self.retired_targets.insert(key);
            if let Some(e) = self
                .entities
                .get_mut(&key.0)
                .filter(|e| e.generation == key.1)
            {
                e.fight_retired = true;
            }
            while self.retired_targets.len() > self.config.max_entities {
                self.retired_targets.pop_first();
            }
            events.push(EngineEvent::FightEnded(v));
            self.release_entity(key);
        }
    }
    pub(crate) fn report(
        &mut self,
        target: i32,
        owner: i32,
        bullet: u16,
        server: bool,
        amount: Result<i32, CoverageIssue>,
        events: &mut Vec<EngineEvent>,
    ) {
        self.report_with_coverage(target, owner, bullet, server, (amount, None), events);
    }
    pub(crate) fn report_with_coverage(
        &mut self,
        target: i32,
        owner: i32,
        bullet: u16,
        server: bool,
        damage: (Result<i32, CoverageIssue>, Option<CoverageIssue>),
        events: &mut Vec<EngineEvent>,
    ) {
        let (amount, assumption) = damage;
        if self.is_player(target) || self.local.as_ref().is_some_and(|l| l.object == target) {
            return;
        }
        if !self.ensure_entity(target, events) {
            return;
        }
        // TomatoData.damage / Entity.genericDamageHit retain the reported
        // amount unless the target is known to be a player (excluded above).
        // Missing target metadata affects coverage, not server damage evidence.
        let problem = amount.as_ref().err().cloned();
        let amount = amount.ok();
        let target_entity = &self.entities[&target];
        let key = (target, target_entity.generation);
        if self.retired_targets.contains(&key) || target_entity.fight_retired {
            return;
        }
        let ty = target_entity.object_type;
        let max_hp = target_entity.stat(0).filter(|n| *n > 0);
        let shot = self
            .shots
            .get(&(owner, bullet))
            .filter(|s| self.now.0.saturating_sub(s.at.0) <= 5_000_000)
            .cloned();
        let owner_gen = self.entities.get(&owner).map(|e| e.generation).unwrap_or(0);
        let pair_key = (
            owner,
            owner_gen,
            shot.as_ref().map(|s| s.number).unwrap_or(0),
            bullet,
        );
        let at = self.tick.unwrap_or(crate::clock::millis(self.now));
        let boundary = [
            self.fights.get(&key).and_then(|f| f.base.ended),
            self.kill_detector
                .removed_at(key)
                .filter(|_| !target_entity.present),
            self.kill_detector.pending_quest_victory_at(key, self.now),
        ]
        .into_iter()
        .flatten()
        .min();
        // A late counterpart belongs to its pre-boundary hit even after a
        // newer tick. It may reconcile that hit, never create post-kill combat.
        let at = boundary
            .filter(|_| server)
            .and_then(|end| {
                self.fights
                    .get(&key)?
                    .pending
                    .get(&pair_key)
                    .filter(|p| p.servers == 0 && p.estimates == 1)?
                    .estimate
                    .as_ref()
                    .filter(|c| c.at <= end)
                    .map(|c| c.at)
            })
            .unwrap_or(at);
        let new = !self.fights.contains_key(&key);
        // Removal or an unresolved quest Victory can be confirmed later;
        // combat newer than that boundary must not enter the pending fight.
        if boundary.is_some_and(|end| at > end) {
            return;
        }
        if new {
            if self.fights.len() >= self.config.max_active_fights {
                // Final kills can leave without losing coverage, but retain them
                // through the same five-second window used by pair settlement.
                let settled = self
                    .fights
                    .iter()
                    .filter(|(_, f)| {
                        f.pending.is_empty()
                            && f.base.kill.as_ref().is_some_and(|kill| !kill.provisional)
                            && f.base.ended.is_some_and(|end| {
                                crate::clock::millis(self.now).0.saturating_sub(end.0) > 5_000_000
                            })
                    })
                    .min_by_key(|(_, f)| f.base.last_hit)
                    .map(|(k, _)| *k);
                if let Some(old) = settled {
                    self.retire_fight(old, FightEnd::Killed, events);
                } else {
                    let open = self
                        .fights
                        .iter()
                        .filter(|(_, f)| f.base.ended.is_none())
                        .min_by_key(|(_, f)| f.base.last_hit)
                        .map(|(k, _)| *k);
                    self.mark(CoverageIssue::ResourceLimit, events);
                    if let Some(old) = open {
                        self.retire_fight(old, FightEnd::ResourceLimit, events);
                    } else {
                        // All slots are awaiting final settlement. Drop the new
                        // observation rather than prematurely settling a kill.
                        if self.dropped_fight_targets.len() < self.config.max_entities {
                            self.dropped_fight_targets.insert(key);
                        } else if !self.dropped_fight_targets.contains(&key) {
                            // Never forget lost damage silently when the marker
                            // budget fills: conservatively taint new fights until
                            // the next map instead of growing this set.
                            self.dropped_fight_targets_overflow = true;
                        }
                        return;
                    }
                }
            }
            let mut issues = vec![];
            if self.dropped_fight_targets.remove(&key) || self.dropped_fight_targets_overflow {
                issues.push(CoverageIssue::ResourceLimit);
            }
            if self.map.is_none() {
                issues.push(CoverageIssue::Midstream);
            }
            if self.character.as_ref().is_none_or(|c| !c.identity_verified) {
                issues.push(CoverageIssue::MissingIdentity);
            }
            if ty.is_none_or(|t| !self.object_index.contains_key(&t)) {
                issues.push(CoverageIssue::MissingCatalog);
            }
            let players = self
                .local
                .as_ref()
                .and_then(|l| self.player_row(l.object))
                .into_iter()
                .collect();
            let boss_type = ty.filter(|t| {
                self.object_index
                    .get(t)
                    .is_some_and(|&i| self.catalog.objects[i].is_boss)
            });
            self.fights.insert(
                key,
                Fight {
                    base: FightView {
                        owner_character_id: self.character.as_ref().map(|c| c.id.clone()),
                        scope_id: self.scope_id(),
                        dps_basis: DpsBasis::PlayerActiveWindowV1,
                        pending_local_reconciliation: false,
                        is_current_quest: self.is_quest(key),
                        local_last_hit: None,
                        local_active_micros: None,
                        kill: None,
                        target_hp: self.entities.get(&target).and_then(|e| e.stat(1)),
                        target_kind: self.target_kind(key, ty),
                        player_series: None,
                        id: format!(
                            "{}:fight:{}:{target}:{}",
                            self.config.stream_id, self.map_ordinal, key.1
                        ),
                        run_id: self.run.as_ref().map(|r| r.view.id.clone()),
                        session_id: self.session_id(),
                        target_id: target,
                        target_generation: key.1,
                        boss_type,
                        target_type: ty,
                        max_hp,
                        started: at,
                        ended: None,
                        first_hit: Some(at),
                        last_hit: Some(at),
                        end_reason: None,
                        players,
                        total_damage: 0.,
                        unattributed_damage: 0.,
                        buckets: vec![],
                        complete_capture: issues.is_empty(),
                        issues,
                    },
                    was_quest: self.is_quest(key),
                    pending: BTreeMap::new(),
                    series: Default::default(),
                    series_budget: Default::default(),
                    timeline_out: vec![],
                    local_last_hit: None,
                    player_windows: BTreeMap::new(),
                    snapshot_series_cache: self.snapshot_series_cache.clone(),
                    totals_cache: self.run_totals_cache.clone(),
                    projection: Default::default(),
                    #[cfg(test)]
                    projection_builds: Default::default(),
                    #[cfg(test)]
                    series_nanos: Default::default(),
                },
            );
        }
        let resolved = self.resolve_owner(owner);
        let player = resolved.and_then(|id| self.player_row(id));
        let mut source = self.source(
            owner,
            shot.as_ref(),
            amount.unwrap_or(0).max(0) as f64,
            !server && assumption.is_some(),
        );
        if !server {
            // Reconstruction is provenance; only uncertain inputs make it estimated.
            source.reasons = vec!["client-projectile-reconstruction".into()];
        }
        let mut contribution = Contribution {
            player,
            source,
            at,
            // Local ENEMYHIT has its own receipt time; server reports retain
            // their established tick clock. Buckets keep the shared tick clock.
            active_at: if server { at } else { self.now },
        };
        if let Some(f) = self.fights.get(&key).filter(|f| f.base.ended.is_some()) {
            // Only settle an existing estimate from combat observed by the kill.
            // New hits cannot reopen or extend a finished fight.
            if !server
                || !f.pending.get(&pair_key).is_some_and(|p| {
                    p.servers == 0
                        && p.estimates == 1
                        && p.estimate.as_ref().is_some_and(|c| c.at <= at)
                })
            {
                return;
            }
        }
        let pair = self.fights[&key].pending.get(&pair_key);
        let paired_damage = if server {
            pair.filter(|p| p.estimates == 1 && p.servers == 0)
                .and_then(|p| p.estimate.as_ref())
                .map(|estimate| {
                    if estimate.player.as_ref().is_some_and(|p| p.is_local) {
                        contribution.active_at = estimate.active_at;
                    }
                    estimate.source.damage
                })
        } else {
            pair.filter(|p| p.servers == 1)
                .and_then(|p| p.server_damage)
        };
        let mismatched = amount
            .zip(paired_damage)
            .is_some_and(|(amount, paired)| f64::from(amount) != paired);
        // Target state timing and kill clamps can disagree without RNG drift.
        // Scope uncertainty to this fight; lost shots/gaps invalidate map RNG.
        if mismatched && !server {
            contribution.source.estimated = true;
        }
        if amount.is_some_and(|n| n > 0)
            && self.pair_expiry.len() >= self.config.max_shots
            && !self.fights[&key].pending.contains_key(&pair_key)
        {
            if let Some((_, fk, pk)) = self.pair_expiry.pop_first() {
                if let Some(f) = self.fights.get_mut(&fk) {
                    if let Some(p) = f.pending.remove(&pk) {
                        f.changed();
                        if let Some((id, at)) = p.server_activity {
                            activity(&mut f.player_windows, &id, at);
                        }
                        if p.estimates > 0 && p.servers == 0 {
                            issue(&mut f.base, CoverageIssue::AmbiguousHit);
                        }
                        self.dirty_fights.insert(fk);
                    }
                }
            }
            self.mark(CoverageIssue::ResourceLimit, events);
        }
        self.protect_entity(target);
        if let Some(id) = resolved {
            self.protect_entity(id);
        }
        let f = self.fights.get_mut(&key).expect("fight inserted");
        f.changed();
        if mismatched {
            issue(&mut f.base, CoverageIssue::AmbiguousHit);
        }
        if f.base.ended.is_none() {
            f.base.last_hit = f.base.last_hit.into_iter().chain(Some(at)).max();
        }
        f.base.max_hp = f.base.max_hp.into_iter().chain(max_hp).max();
        if self.tick.is_none() {
            issue(&mut f.base, CoverageIssue::Midstream);
        }
        if let Some(problem) = problem {
            issue(&mut f.base, problem);
        }
        if let Some(assumption) = assumption {
            issue(&mut f.base, assumption);
        }
        if amount.is_some_and(|n| n > 0) {
            f.series_budget
                .observe(contribution.at, contribution.player.as_ref());
            if f.series_budget.disabled {
                f.series.disable();
            }
            if !f.pending.contains_key(&pair_key) {
                self.pair_expiry.insert((self.now, key, pair_key));
            }
            let pair = f.pending.entry(pair_key).or_insert(Pair {
                estimate: None,
                estimates: 0,
                servers: 0,
                server_damage: None,
                server_activity: None,
                at: self.now,
            });
            if server {
                pair.servers = pair.servers.saturating_add(1);
                pair.server_damage = amount.map(f64::from);
            } else {
                pair.estimates = pair.estimates.saturating_add(1);
                pair.estimate = Some(contribution.clone());
            }
            if pair.estimates > 1 || pair.estimates > 0 && pair.servers > 1 {
                issue(&mut f.base, CoverageIssue::AmbiguousHit);
            }
            if !server && pair.servers == 1 && pair.estimates == 1 {
                if let Some((_, at)) = &mut pair.server_activity {
                    *at = contribution.active_at;
                    f.local_last_hit = f.local_last_hit.into_iter().chain(Some(*at)).max();
                }
            }
            let defer_activity = server
                && pair.servers == 1
                && contribution.player.as_ref().is_some_and(|p| p.is_local);
            // D-hit: server values are permanent; only one unpaired estimate is projected.
            if server {
                f.accept(&contribution);
                if defer_activity {
                    let player = contribution.player.as_ref().expect("local player");
                    if f.base
                        .players
                        .iter()
                        .any(|p| p.player_id == player.player_id)
                    {
                        f.pending
                            .get_mut(&pair_key)
                            .expect("inserted pair")
                            .server_activity =
                            Some((player.player_id.clone(), contribution.active_at));
                    }
                } else {
                    f.record_activity(&contribution);
                }
            }
        }
        f.latch_projected_series_cap();
        if new {
            self.dirty_fights.remove(&key);
            events.push(EngineEvent::FightStarted(f.project()));
        } else {
            self.dirty_fights.insert(key);
        }
    }
}
fn issue(v: &mut FightView, i: CoverageIssue) {
    v.complete_capture = false;
    if !v.issues.contains(&i) {
        v.issues.push(i);
    }
}

#[cfg(test)]
#[path = "fights_local_window_tests.rs"]
mod local_window_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_target_markers_are_bounded_and_overflow_clears_with_world() {
        let mut engine = Engine::new(
            crate::EngineConfig {
                max_entities: 4,
                max_active_fights: 1,
                ..Default::default()
            },
            std::sync::Arc::new(maxed_types::demo::demo_catalog()),
        )
        .unwrap();
        engine.advance_to(Timestamp(1_000_000));
        engine.report(42, 1, 1, true, Ok(100), &mut vec![]);
        let fight = engine.fights.values_mut().next().unwrap();
        fight.base.ended = Some(Timestamp(1_000_000));
        fight.base.kill = Some(KillView {
            at: Timestamp(1_000_000),
            provisional: false,
            evidence: KillEvidence::HpZeroRemoved,
        });
        for target in 43..60 {
            engine.report(target, 1, 1, true, Ok(100), &mut vec![]);
            // Model untracked entities leaving memory while their lost-hit
            // evidence must survive for the rest of this map.
            engine.entities.remove(&target);
            assert!(engine.dropped_fight_targets.len() <= 4);
            assert_eq!(engine.fights.len(), 1);
        }
        assert_eq!(engine.dropped_fight_targets.len(), 4);
        assert!(engine.dropped_fight_targets_overflow);
        engine.advance_to(Timestamp(7_000_000));
        engine.report(60, 1, 1, true, Ok(100), &mut vec![]);
        let fight = engine.fights.values().next().unwrap();
        assert_eq!(fight.base.target_id, 60);
        assert!(fight.base.issues.contains(&CoverageIssue::ResourceLimit));
        assert!(!fight.base.complete_capture);
        engine.finish(Timestamp(8_000_000), FightEnd::CaptureStopped);
        assert!(engine.dropped_fight_targets.is_empty());
        assert!(!engine.dropped_fight_targets_overflow);
    }

    #[test]
    fn latest_open_fight_hit_ignores_ended_and_hitless_fights() {
        let mut engine = Engine::new(
            Default::default(),
            std::sync::Arc::new(maxed_types::demo::demo_catalog()),
        )
        .unwrap();
        assert_eq!(engine.latest_open_fight_hit(), None);

        engine.advance_to(Timestamp(1_000_000));
        engine.report(42, 1, 1, true, Ok(100), &mut vec![]);
        assert_eq!(engine.latest_open_fight_hit(), Some(Timestamp(1_000_000)));
        engine.advance_to(Timestamp(2_000_000));
        engine.report(43, 1, 2, true, Ok(100), &mut vec![]);
        assert_eq!(engine.latest_open_fight_hit(), Some(Timestamp(2_000_000)));

        let latest = engine
            .fights
            .values_mut()
            .find(|f| f.base.target_id == 43)
            .unwrap();
        latest.base.ended = Some(Timestamp(2_000_000));
        assert_eq!(engine.latest_open_fight_hit(), Some(Timestamp(1_000_000)));
        let open = engine
            .fights
            .values_mut()
            .find(|f| f.base.target_id == 42)
            .unwrap();
        open.base.last_hit = None;
        assert_eq!(engine.latest_open_fight_hit(), None);

        engine.advance_to(Timestamp(7_000_000));
        engine.report(42, 1, 3, true, Ok(100), &mut vec![]);
        assert_eq!(engine.latest_open_fight_hit(), Some(Timestamp(7_000_000)));
        engine.finish(Timestamp(7_000_001), FightEnd::CaptureStopped);
        assert_eq!(engine.latest_open_fight_hit(), None);
    }

    #[test]
    fn projected_series_cap_latches_independent_of_snapshot_and_cancelled_estimate() {
        let base = maxed_types::demo::demo_snapshot().fights.remove(0);
        let mut rows = Vec::new();
        for i in 0..112 {
            let mut row = base.players[0].clone();
            row.player_id = format!("synthetic-{i}");
            row.object_id = i;
            row.is_local = i == 0;
            rows.push(row);
        }
        let mut series = crate::engine::series::Series::default();
        let mut compact = crate::engine::series::Series::default();
        for row in &rows[..111] {
            series.add(Timestamp(0), Some(row), 1.);
            compact.add(Timestamp(0), Some(row), 1.);
        }
        for i in 1..1790 {
            series.add(Timestamp(i * 1_000_000), Some(&rows[0]), 1.);
        }
        for i in 1..900 {
            compact.add(Timestamp(i * 1_000_000), Some(&rows[0]), 1.);
        }
        let mut f = Fight {
            base,
            was_quest: false,
            pending: BTreeMap::new(),
            series,
            series_budget: compact.budget(),
            timeline_out: vec![],
            local_last_hit: None,
            player_windows: BTreeMap::new(),
            snapshot_series_cache: Default::default(),
            totals_cache: Default::default(),
            projection: Default::default(),
            projection_builds: Default::default(),
            series_nanos: Default::default(),
        };
        let source = f.base.players[0].sources[0].clone();
        f.pending.insert(
            (112, 0, 0, 0),
            Pair {
                estimate: Some(Contribution {
                    player: Some(rows[111].clone()),
                    source,
                    at: Timestamp(0),
                    active_at: Timestamp(0),
                }),
                estimates: 1,
                servers: 0,
                server_damage: None,
                server_activity: None,
                at: Timestamp(0),
            },
        );
        // Historical estimate compaction may have a coarser budget grid than
        // the accepted series. Projection itself remains read-only.
        assert!(f.project().player_series.is_none());
        assert!(!f.series_budget.disabled);
        f.latch_projected_series_cap();
        f.pending.clear();
        assert!(f.series_budget.disabled);
        assert!(f.project().player_series.is_none());
    }

    #[test]
    fn bucket_picks_same_bucket_for_every_position() {
        const S: u64 = 1_000_000;
        let base = maxed_types::demo::demo_snapshot().fights.remove(0);
        let source = base.players[0].sources[0].clone();
        let make = |from: u64, until: u64| DamageBucket {
            from: Timestamp(from * S),
            until: Timestamp(until * S),
            total: 1.,
            local: None,
            others_damage: None,
            others_presence: None,
        };
        let ranges = [(10, 11), (11, 13), (20, 21)];
        let probes = [
            ("newest bucket", 20 * S + 500_000, Some(2)),
            ("newest bucket at from", 20 * S, Some(2)),
            ("newest bucket at until", 21 * S, None),
            ("older bucket", 11 * S + 500_000, Some(1)),
            ("older bucket at from", 11 * S, Some(1)),
            ("older bucket at until", 13 * S, None),
            ("first bucket", 10 * S + 1, Some(0)),
            ("before first", 5 * S, None),
            ("gap between", 15 * S, None),
            ("after last", 30 * S, None),
        ];
        for (label, at, index) in probes {
            let mut v = base.clone();
            v.players.clear();
            v.buckets = ranges.iter().map(|(a, b)| make(*a, *b)).collect();
            let c = Contribution {
                player: None,
                source: SourceDamage {
                    damage: 5.,
                    ..source.clone()
                },
                at: Timestamp(at),
                active_at: Timestamp(at),
            };
            bucket(&mut v, &c, None, None);
            let mut expected: Vec<_> = ranges.iter().map(|(a, b)| make(*a, *b)).collect();
            match index {
                Some(i) => expected[i].total += 5.,
                None => {
                    let rounded = at / S;
                    let mut added = make(rounded, rounded + 1);
                    added.total = 5.;
                    expected.push(added);
                    expected.sort_by_key(|b| b.from);
                }
            }
            assert_eq!(v.buckets, expected, "{label}");
        }
    }

    #[test]
    fn out_of_order_hits_keep_view_buckets_and_series_cells_aligned() {
        const S: u64 = 1_000_000;
        let mut v = maxed_types::demo::demo_snapshot().fights.remove(0);
        let source = v.players[0].sources[0].clone();
        let mut template = v.players[0].clone();
        template.is_local = false;
        v.players.clear();
        v.buckets.clear();
        let mut series = crate::engine::series::Series::default();
        let mut hit = |v: &mut FightView, id: usize, at: u64, damage: f64| {
            let mut p = template.clone();
            p.player_id = format!("synthetic-{id}");
            p.object_id = id as i32;
            let c = Contribution {
                player: Some(p),
                source: SourceDamage {
                    damage,
                    ..source.clone()
                },
                at: Timestamp(at),
                active_at: Timestamp(at),
            };
            add(v, &mut series, &c);
        };
        hit(&mut v, 1, 20 * S, 1.);
        hit(&mut v, 1, 21 * S, 2.);
        hit(&mut v, 2, 21 * S, 3.);
        // Before every bucket, into a gap, into an existing bucket, after the last.
        hit(&mut v, 2, 10 * S, 5.);
        hit(&mut v, 1, 15 * S, 7.);
        hit(&mut v, 2, 21 * S + 7, 6.);
        hit(&mut v, 1, 30 * S, 8.);
        let froms: Vec<_> = v.buckets.iter().map(|b| b.from.0 / S).collect();
        assert_eq!(froms, vec![10, 15, 20, 21, 30]);
        assert_eq!(
            v.buckets.iter().map(|b| b.total).collect::<Vec<_>>(),
            vec![5., 7., 1., 11., 8.]
        );
        assert_eq!(series.buckets.len(), v.buckets.len());
        let rows = series.project().unwrap();
        assert_eq!(rows[0].points, vec![0., 7., 1., 2., 8.]);
        assert_eq!(rows[1].points, vec![5., 0., 0., 9., 0.]);
        for (i, b) in v.buckets.iter().enumerate() {
            assert_eq!(rows.iter().map(|r| r.points[i]).sum::<f64>(), b.total);
            assert_eq!(
                (series.buckets[i].0, series.buckets[i].1),
                (b.from, b.until)
            );
        }
    }
}

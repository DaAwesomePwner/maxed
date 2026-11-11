//! Sparse cells use the fight bucket boundaries, including pairwise compaction.
use maxed_types::{PlayerDamage, Timestamp};
use std::collections::BTreeMap;
pub(crate) const SERIES_CELL_LIMIT: usize = 200_000;
/// Index of the bucket containing `at`. Buckets are sorted and non-overlapping,
/// so live hits resolve against the newest bucket and the rest by binary search.
pub(crate) fn find_bucket<T>(
    buckets: &[T],
    at: Timestamp,
    span: impl Fn(&T) -> (Timestamp, Timestamp),
) -> Option<usize> {
    let contains = |bucket: &T| {
        let (from, until) = span(bucket);
        from <= at && at < until
    };
    let last = buckets.len().checked_sub(1)?;
    if contains(&buckets[last]) {
        return Some(last);
    }
    let index = buckets
        .partition_point(|bucket| span(bucket).0 <= at)
        .checked_sub(1)?;
    contains(&buckets[index]).then_some(index)
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlayerSeries {
    pub player_id: String,
    pub object_id: i32,
    pub is_local: bool,
    pub points: Vec<f64>,
}
#[derive(Clone, Default)]
pub(crate) struct Series {
    pub buckets: Vec<(Timestamp, Timestamp)>,
    cells: BTreeMap<(String, usize), f64>,
    players: BTreeMap<String, (i32, bool)>,
    disabled: bool,
}
impl Series {
    pub fn add(&mut self, at: Timestamp, player: Option<&PlayerDamage>, damage: f64) {
        if self.disabled || !damage.is_finite() || damage < 0. {
            return;
        }
        let at = Timestamp(at.0 / 1_000_000 * 1_000_000);
        let mut index = find_bucket(&self.buckets, at, |b| *b);
        if index.is_none() {
            let insert = self.buckets.partition_point(|(from, _)| *from < at);
            if insert < self.buckets.len() {
                self.cells = std::mem::take(&mut self.cells)
                    .into_iter()
                    .map(|((p, i), v)| ((p, if i >= insert { i + 1 } else { i }), v))
                    .collect();
            }
            self.buckets
                .insert(insert, (at, Timestamp(at.0.saturating_add(1_000_000))));
            index = Some(insert);
        }
        let mut index = index.expect("found or inserted bucket");
        if self.buckets.len() > 1800 {
            self.buckets = self
                .buckets
                .chunks(2)
                .map(|b| (b[0].0, b[b.len() - 1].1))
                .collect();
            let mut compact = BTreeMap::new();
            for ((p, i), v) in std::mem::take(&mut self.cells) {
                *compact.entry((p, i / 2)).or_insert(0.) += v;
            }
            self.cells = compact;
            index /= 2;
        }
        if let Some(p) = player {
            self.players
                .entry(p.player_id.clone())
                .and_modify(|(_, local)| *local |= p.is_local)
                .or_insert((p.object_id, p.is_local));
            let key = (p.player_id.clone(), index);
            if self.cells.len() >= SERIES_CELL_LIMIT && !self.cells.contains_key(&key) {
                self.disable();
                return;
            }
            let value = self.cells.entry(key).or_insert(0.);
            *value += damage;
            if !value.is_finite() {
                self.disable();
                return;
            }
        }
        // The dense contract projection is bounded too, even for very sparse
        // input. Exceeding either bound permanently discards only this series.
        if self.players.len().saturating_mul(self.buckets.len()) > SERIES_CELL_LIMIT {
            self.disable();
        }
    }
    pub fn disable(&mut self) {
        self.disabled = true;
        self.cells.clear();
        self.players.clear();
    }
    pub fn dimensions(&self) -> (usize, usize) {
        (self.players.len(), self.buckets.len())
    }
    pub fn bucket_totals(&self) -> Vec<(f64, u16)> {
        let mut totals = vec![(0., 0u16); self.buckets.len()];
        for ((_, index), damage) in &self.cells {
            totals[*index].0 += damage;
            if *damage > 0. {
                totals[*index].1 = totals[*index].1.saturating_add(1);
            }
        }
        totals
    }
    pub fn has_cell(&self, at: Timestamp, player_id: &str) -> bool {
        find_bucket(&self.buckets, at, |b| *b)
            .is_some_and(|index| self.cells.contains_key(&(player_id.into(), index)))
    }
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
    pub fn budget(&self) -> SeriesBudget {
        SeriesBudget {
            buckets: self.buckets.clone(),
            players: self.players.keys().cloned().collect(),
            disabled: self.disabled,
        }
    }
    pub fn merge_identity(&mut self, old: &str, new: &str) {
        if let Some(meta) = self.players.remove(old) {
            self.players.entry(new.into()).or_insert(meta);
        }
        let keys: Vec<_> = self
            .cells
            .range((old.into(), 0)..=(old.into(), usize::MAX))
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            if let Some(value) = self.cells.remove(&key) {
                *self.cells.entry((new.into(), key.1)).or_default() += value;
            }
        }
    }
    #[cfg(test)]
    pub fn project(&self) -> Option<Vec<PlayerSeries>> {
        self.project_selected(|_| true)
    }
    pub fn project_selected(&self, selected: impl Fn(&str) -> bool) -> Option<Vec<PlayerSeries>> {
        if self.disabled {
            return None;
        }
        Some(
            self.players
                .iter()
                .filter(|(id, _)| selected(id))
                .map(|(id, (object_id, is_local))| {
                    let mut points = vec![0.; self.buckets.len()];
                    for ((_, i), v) in self.cells.range((id.clone(), 0)..=(id.clone(), usize::MAX))
                    {
                        points[*i] = *v;
                    }
                    PlayerSeries {
                        player_id: id.clone(),
                        object_id: *object_id,
                        is_local: *is_local,
                        points,
                    }
                })
                .collect(),
        )
    }
}
// Count observed cells at input time, including unresolved estimates. This
// permanent safety latch must never depend on how often callers take snapshots.
#[derive(Default)]
pub(crate) struct SeriesBudget {
    buckets: Vec<(Timestamp, Timestamp)>,
    players: std::collections::BTreeSet<String>,
    pub disabled: bool,
}
impl SeriesBudget {
    pub fn observe(&mut self, at: Timestamp, player: Option<&PlayerDamage>) {
        if self.disabled {
            return;
        }
        let at = Timestamp(at.0 / 1_000_000 * 1_000_000);
        if find_bucket(&self.buckets, at, |b| *b).is_none() {
            let index = self.buckets.partition_point(|(from, _)| *from < at);
            self.buckets
                .insert(index, (at, Timestamp(at.0.saturating_add(1_000_000))));
            if self.buckets.len() > 1800 {
                self.buckets = self
                    .buckets
                    .chunks(2)
                    .map(|b| (b[0].0, b[b.len() - 1].1))
                    .collect();
            }
        }
        if let Some(p) = player {
            self.players.insert(p.player_id.clone());
        }
        if self.players.len().saturating_mul(self.buckets.len()) > SERIES_CELL_LIMIT {
            self.disabled = true;
            self.players.clear();
            self.buckets.clear();
        }
    }
    pub fn merge_identity(&mut self, old: &str, new: &str) {
        if self.players.remove(old) {
            self.players.insert(new.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn player(id: usize) -> PlayerDamage {
        let mut p = maxed_types::demo::demo_snapshot()
            .fights
            .remove(0)
            .players
            .remove(0);
        p.player_id = format!("synthetic-{id}");
        p.object_id = id as i32;
        p
    }
    #[test]
    fn series_cell_cap_nulls_series_not_fight() {
        let mut s = Series::default();
        let mut damage = 0.;
        for id in 0..201 {
            let p = player(id);
            for bucket in 0..1000 {
                s.add(Timestamp(bucket * 1_000_000), Some(&p), 1.);
                damage += 1.;
            }
        }
        assert_eq!(damage, 201000.);
        assert_eq!(s.project(), None);
        assert!(s.cells.is_empty());
        s.add(Timestamp(1), Some(&player(0)), 1.);
        assert_eq!(s.project(), None);
    }
    #[test]
    fn series_compaction_matches_fight_buckets() {
        let mut s = Series::default();
        let p = player(1);
        for i in 0..1801 {
            s.add(Timestamp(i * 1_000_000), Some(&p), 1.);
        }
        assert_eq!(s.buckets.len(), 901);
        assert_eq!(s.buckets[0], (Timestamp(0), Timestamp(2_000_000)));
        s.add(Timestamp(1_500_000), Some(&p), 3.);
        let rows = s.project().unwrap();
        assert_eq!(rows[0].points[0], 5.);
        assert_eq!(rows[0].points.iter().sum::<f64>(), 1804.);
    }

    const S: u64 = 1_000_000;
    fn secs(range: std::ops::Range<u64>) -> (Timestamp, Timestamp) {
        (Timestamp(range.start * S), Timestamp(range.end * S))
    }
    // Contiguous 10..13 (one merged bucket 11..13), then a gap, then 20..21.
    fn gapped() -> Vec<(Timestamp, Timestamp)> {
        vec![secs(10..11), secs(11..13), secs(20..21)]
    }
    /// (label, timestamp in micros, expected bucket index)
    fn probes() -> Vec<(&'static str, u64, Option<usize>)> {
        vec![
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
        ]
    }
    #[test]
    fn find_bucket_matches_linear_scan() {
        let buckets = gapped();
        for at in (0..25 * S).step_by(250_000) {
            let expected = buckets
                .iter()
                .position(|(from, until)| *from <= Timestamp(at) && Timestamp(at) < *until);
            assert_eq!(find_bucket(&buckets, Timestamp(at), |b| *b), expected);
        }
        assert_eq!(
            find_bucket(&[], Timestamp(1), |b: &(Timestamp, Timestamp)| *b),
            None
        );
    }
    #[test]
    fn has_cell_resolves_every_position() {
        let p = player(1);
        let mut s = Series::default();
        for (from, _) in gapped() {
            s.add(from, Some(&p), 1.);
        }
        s.add(Timestamp(12 * S), Some(&p), 1.);
        s.add(Timestamp(11 * S), Some(&player(2)), 1.);
        for (label, at, index) in probes() {
            assert_eq!(
                s.has_cell(Timestamp(at), "synthetic-1"),
                index.is_some(),
                "{label}"
            );
        }
        assert!(s.has_cell(Timestamp(11 * S), "synthetic-2"));
        assert!(!s.has_cell(Timestamp(10 * S), "synthetic-2"));
        assert!(!s.has_cell(Timestamp(20 * S), "synthetic-2"));
    }
    #[test]
    fn add_picks_same_bucket_for_every_position() {
        let p = player(1);
        for (label, at, index) in probes() {
            let mut s = Series::default();
            for (from, _) in gapped() {
                s.add(from, Some(&p), 1.);
            }
            // Series buckets are built from single seconds; install the merged one.
            s.buckets = gapped();
            s.cells = [
                ((p.player_id.clone(), 0), 1.),
                ((p.player_id.clone(), 1), 1.),
                ((p.player_id.clone(), 2), 1.),
            ]
            .into_iter()
            .collect();
            s.add(Timestamp(at), Some(&p), 5.);
            let rounded = at / S * S;
            match index {
                Some(i) => {
                    assert_eq!(s.buckets, gapped(), "{label}");
                    assert_eq!(s.cells[&(p.player_id.clone(), i)], 6., "{label}");
                    assert_eq!(s.cells.len(), 3, "{label}");
                }
                None => {
                    let mut expected = gapped();
                    let at_index = expected.partition_point(|(from, _)| from.0 < rounded);
                    expected.insert(at_index, secs(rounded / S..rounded / S + 1));
                    assert_eq!(s.buckets, expected, "{label}");
                    assert_eq!(s.cells[&(p.player_id.clone(), at_index)], 5., "{label}");
                    assert_eq!(s.cells.len(), 4, "{label}");
                    assert_eq!(s.cells.values().sum::<f64>(), 8., "{label}");
                }
            }
        }
    }
    #[test]
    fn budget_observe_picks_same_bucket_for_every_position() {
        for (label, at, index) in probes() {
            let mut budget = SeriesBudget {
                buckets: gapped(),
                ..Default::default()
            };
            budget.observe(Timestamp(at), Some(&player(1)));
            let mut expected = gapped();
            if index.is_none() {
                let rounded = at / S * S;
                let at_index = expected.partition_point(|(from, _)| from.0 < rounded);
                expected.insert(at_index, secs(rounded / S..rounded / S + 1));
            }
            assert_eq!(budget.buckets, expected, "{label}");
            assert!(!budget.disabled, "{label}");
        }
    }
}

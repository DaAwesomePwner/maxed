//! Bounded presentation resolution. Stored/domain projections stay complete.
use maxed_types::{DamageBucket, FightView};

/// Target resolution, not permission to fill unobserved gaps. Output has at
/// most max(120, contiguous input runs) buckets: at most 1,800 for engine views.
/// More than 120 disjoint runs cannot all retain their sums/endpoints in 120
/// intervals. The UI already samples those views to 120 gap-separated intervals.
pub const LIVE_CHART_BUCKETS: usize = 120;

pub fn compact_live_fight(view: &mut FightView) {
    // Keep the internal identity witness; omit only its unused presentation row.
    if view.local_last_hit.is_none() {
        view.players.retain(|p| !p.is_local || p.damage != 0.);
        if let Some(series) = &mut view.player_series {
            series.retain(|p| !p.is_local);
        }
    }
    let count = view.buckets.len();
    if count <= LIVE_CHART_BUCKETS {
        return;
    }
    let mut runs = Vec::new();
    let mut start = 0;
    for index in 1..count {
        if view.buckets[index - 1].until != view.buckets[index].from {
            runs.push(start..index);
            start = index;
        }
    }
    runs.push(start..count);
    let budget = LIVE_CHART_BUCKETS.max(runs.len());
    // Find the smallest common merge factor that fits without joining runs.
    let (mut low, mut high) = (1, count);
    while low < high {
        let width = low + (high - low) / 2;
        if runs
            .iter()
            .map(|run| run.len().div_ceil(width))
            .sum::<usize>()
            > budget
        {
            low = width + 1;
        } else {
            high = width;
        }
    }
    let ranges: Vec<_> = runs
        .into_iter()
        .flat_map(|run| {
            (run.start..run.end)
                .step_by(low)
                .map(move |start| start..(start + low).min(run.end))
        })
        .collect();
    let buckets = std::mem::take(&mut view.buckets);
    view.buckets = ranges
        .iter()
        .map(|range| {
            let chunk = &buckets[range.clone()];
            DamageBucket {
                from: chunk[0].from,
                until: chunk.last().expect("nonempty").until,
                total: chunk.iter().map(|b| b.total).sum(),
                local: chunk.iter().map(|b| b.local).sum(),
            }
        })
        .collect();
    if let Some(series) = &mut view.player_series {
        if series.iter().any(|row| row.points.len() != count) {
            view.player_series = None;
        } else {
            for row in series {
                row.points = ranges
                    .iter()
                    .map(|range| row.points[range.clone()].iter().sum())
                    .collect();
            }
        }
    }
}

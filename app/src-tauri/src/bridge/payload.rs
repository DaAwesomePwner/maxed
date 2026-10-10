use maxed_types::{LiveDelta, LiveSnapshot, LiveSnapshotUpdate};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct SnapshotEncoder {
    previous: Option<LiveSnapshot>,
}
impl SnapshotEncoder {
    pub fn encode(&mut self, snapshot: LiveSnapshot) -> LiveSnapshotUpdate {
        let result = match &self.previous {
            Some(previous)
                if previous.stream_id == snapshot.stream_id
                    && previous.sequence.0 < snapshot.sequence.0 =>
            {
                let mut changes = snapshot.clone();
                let fights: BTreeMap<_, _> =
                    previous.fights.iter().map(|row| (&row.id, row)).collect();
                let drops: BTreeMap<_, _> =
                    previous.drops.iter().map(|row| (&row.id, row)).collect();
                changes
                    .fights
                    .retain(|row| fights.get(&row.id).is_none_or(|old| **old != *row));
                changes
                    .drops
                    .retain(|row| drops.get(&row.id).is_none_or(|old| **old != *row));
                let current_fights: std::collections::BTreeSet<_> =
                    snapshot.fights.iter().map(|row| &row.id).collect();
                let current_drops: std::collections::BTreeSet<_> =
                    snapshot.drops.iter().map(|row| &row.id).collect();
                LiveSnapshotUpdate::Delta(LiveDelta {
                    base_sequence: previous.sequence,
                    snapshot: changes,
                    removed_fights: fights
                        .keys()
                        .filter(|id| !current_fights.contains(**id))
                        .map(|id| (*id).clone())
                        .collect(),
                    removed_drops: drops
                        .keys()
                        .filter(|id| !current_drops.contains(**id))
                        .map(|id| (*id).clone())
                        .collect(),
                })
            }
            _ => LiveSnapshotUpdate::Full(snapshot.clone()),
        };
        self.previous = Some(snapshot);
        result
    }
}

pub fn presentation_event(mut event: maxed_types::EventEnvelope) -> maxed_types::EventEnvelope {
    use maxed_types::{EngineEvent, TargetKind};
    if let EngineEvent::FightUpdated(fight) = &mut event.event {
        let priority = fight.is_current_quest
            || !matches!(fight.target_kind, None | Some(TargetKind::Ordinary));
        if priority && fight.ended.is_none() {
            // Snapshots own live charts. Their 16-fight series cap is accepted:
            // Home renders bucket charts, not per-player series. Terminal events
            // retain their charts for departure and retained visit history.
            fight.buckets = Vec::new();
            fight.player_series = None;
            event.fight_chart_omitted = true;
        }
    }
    if let EngineEvent::FightStarted(fight)
    | EngineEvent::FightUpdated(fight)
    | EngineEvent::FightEnded(fight) = &mut event.event
    {
        if !fight.is_current_quest && matches!(fight.target_kind, None | Some(TargetKind::Ordinary))
        {
            // Drop the backing allocation too; queued ordinary fights need no chart.
            fight.buckets = Vec::new();
            fight.player_series = None;
        }
        maxed_engine::compact_live_fight(fight);
    }
    event
}

/// Split already compacted presentation events at a hard event-count bound.
/// Gaps and source resets are never hidden inside a claimed contiguous range.
pub use maxed_types::EVENT_BATCH_LIMIT;
pub fn event_batches(events: Vec<maxed_types::EventEnvelope>) -> Vec<maxed_types::EventBatch> {
    let mut batches: Vec<maxed_types::EventBatch> = Vec::new();
    for event in events {
        let append = batches.last().is_some_and(|batch| {
            batch.events.len() < EVENT_BATCH_LIMIT
                && batch.stream_id == event.stream_id
                && batch.last_sequence.0 .0.checked_add(1) == Some(event.sequence.0 .0)
        });
        if append {
            let batch = batches.last_mut().unwrap();
            batch.last_sequence = event.sequence;
            batch.events.push(event);
        } else {
            batches.push(maxed_types::EventBatch {
                stream_id: event.stream_id.clone(),
                first_sequence: event.sequence,
                last_sequence: event.sequence,
                events: vec![event],
            });
        }
    }
    batches
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::{EngineEvent, EventEnvelope, PlayerSeries, Sequence, TargetKind, Timestamp};

    #[test]
    fn chart_omission_covers_priority_transitions_and_preserves_terminal_events() {
        // Cover both current-quest flag states for every target kind, since the
        // flag can be set before classification catches up.
        for kind in [
            None,
            Some(TargetKind::Ordinary),
            Some(TargetKind::Boss),
            Some(TargetKind::EventBoss),
            Some(TargetKind::Miniboss),
            Some(TargetKind::Quest),
        ] {
            for quest in [false, true] {
                for variant in 0..4 {
                    let mut fight = maxed_types::demo::demo_snapshot().fights.remove(0);
                    fight.target_kind = kind.clone();
                    fight.is_current_quest = quest;
                    fight.ended = (variant >= 2).then_some(Timestamp(50));
                    fight.player_series = Some(vec![PlayerSeries {
                        player_id: "synthetic".into(),
                        object_id: 1,
                        is_local: false,
                        points: fight.buckets.iter().map(|bucket| bucket.total).collect(),
                    }]);
                    assert!(!fight.buckets.is_empty());
                    let event = match variant {
                        0 => EngineEvent::FightStarted(fight.clone()),
                        1 | 3 => EngineEvent::FightUpdated(fight.clone()),
                        _ => EngineEvent::FightEnded(fight.clone()),
                    };
                    let original = EventEnvelope {
                        stream_id: "synthetic".into(),
                        sequence: Sequence(Timestamp(1)),
                        at: Timestamp(50),
                        event,
                        fight_chart_omitted: false,
                    };
                    let bytes = serde_json::to_vec(&original).unwrap();
                    let presented = presentation_event(original.clone());
                    let priority = quest || !matches!(kind, None | Some(TargetKind::Ordinary));
                    let omitted = priority && variant == 1;
                    assert_eq!(presented.fight_chart_omitted, omitted);
                    if omitted || !priority {
                        fight.buckets = Vec::new();
                        fight.player_series = None;
                    }
                    maxed_engine::compact_live_fight(&mut fight);
                    let mut expected = original.clone();
                    expected.fight_chart_omitted = omitted;
                    expected.event = match variant {
                        0 => EngineEvent::FightStarted(fight),
                        1 | 3 => EngineEvent::FightUpdated(fight),
                        _ => EngineEvent::FightEnded(fight),
                    };
                    assert_eq!(presented, expected);
                    // The domain copy (including chart allocations) remains complete.
                    assert_eq!(serde_json::to_vec(&original).unwrap(), bytes);
                    if let EngineEvent::FightUpdated(fight) = &presented.event {
                        if omitted {
                            assert_eq!(fight.buckets.capacity(), 0);
                        }
                    }
                }
            }
        }
    }
}

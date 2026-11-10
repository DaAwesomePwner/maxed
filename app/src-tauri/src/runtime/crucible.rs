//! Bounded, non-blocking retry of configuration independently of event batches.
use crossbeam_channel::{Receiver, TryRecvError};
use maxed_store::StoreWriter;
use maxed_types::{CrucibleDef, Timestamp};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct PendingDefinitions {
    pending: BTreeMap<String, f64>,
    in_flight: Option<(Receiver<bool>, Vec<CrucibleDef>)>,
}

impl PendingDefinitions {
    pub(super) fn drain(
        &mut self,
        writer: Option<&StoreWriter>,
        definitions: Vec<CrucibleDef>,
        at: Timestamp,
    ) {
        if let Some((response, sent)) = &self.in_flight {
            match response.try_recv() {
                Ok(true) => {
                    for definition in sent {
                        if self.pending.get(&definition.id) == Some(&definition.damage_multiplier) {
                            self.pending.remove(&definition.id);
                        }
                    }
                    self.in_flight = None;
                }
                Ok(false) | Err(TryRecvError::Disconnected) => self.in_flight = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        for definition in definitions {
            if self.pending.len() < 4096 || self.pending.contains_key(&definition.id) {
                self.pending
                    .insert(definition.id, definition.damage_multiplier);
            }
        }
        if self.in_flight.is_some() || self.pending.is_empty() {
            return;
        }
        if let Some(writer) = writer {
            let definitions: Vec<_> = self
                .pending
                .iter()
                .map(|(id, damage_multiplier)| CrucibleDef {
                    id: id.clone(),
                    damage_multiplier: *damage_multiplier,
                })
                .collect();
            if let Ok(response) = writer.try_save_crucible_definitions(definitions.clone(), at) {
                self.in_flight = Some((response, definitions));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_definitions_are_bounded_and_latest_wins() {
        let mut pending = PendingDefinitions::default();
        pending.drain(
            None,
            (0..4097)
                .map(|id| CrucibleDef {
                    id: id.to_string(),
                    damage_multiplier: 1.,
                })
                .collect(),
            Timestamp(1),
        );
        pending.drain(
            None,
            vec![CrucibleDef {
                id: "0".into(),
                damage_multiplier: 2.,
            }],
            Timestamp(2),
        );
        assert_eq!(pending.pending.len(), 4096);
        assert_eq!(pending.pending["0"], 2.);
    }

    #[test]
    fn absent_writer_retains_latest_and_in_flight_ack_cannot_erase_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("definitions");
        let writer = maxed_store::start_writer(path.clone(), Default::default()).unwrap();
        let def = |damage_multiplier| CrucibleDef {
            id: "synthetic".into(),
            damage_multiplier,
        };
        let mut pending = PendingDefinitions::default();
        pending.drain(None, vec![def(1.)], Timestamp(1));
        let (reply, response) = crossbeam_channel::bounded(1);
        pending.in_flight = Some((response, vec![def(1.)]));
        pending.drain(None, vec![def(2.)], Timestamp(2));
        reply.send(true).unwrap(); // The old write completes after the update.

        pending.drain(Some(&writer), vec![], Timestamp(3));
        writer.flush().unwrap();
        // Observe a newer value while acknowledging the previous one.
        pending.drain(Some(&writer), vec![def(3.)], Timestamp(4));
        writer.flush().unwrap();
        pending.drain(Some(&writer), vec![], Timestamp(5));
        assert!(pending.pending.is_empty());
        assert_eq!(
            maxed_store::StoreReader::read_crucible_definitions_file(&path).unwrap(),
            vec![def(3.)]
        );
        writer.shutdown().unwrap();
    }
}

//! Local-only calculation inputs, paired on the engine owner before presentation.
use super::CaptureRuntime;
use maxed_engine::character::{calculate, input_revision, CharacterInput};
use maxed_types::*;
use std::sync::{atomic::Ordering, Arc};

const STALE: &str = "stale_character_revision";
const UNAVAILABLE: &str = "character_inputs_unavailable";

/// Only the identity fields needed to validate the owner observation.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub(super) struct CharacterIdentity {
    stream_id: String,
    character_id: Option<String>,
}

impl CharacterIdentity {
    pub fn from_snapshot(snapshot: &LiveSnapshot) -> Self {
        Self {
            stream_id: snapshot.stream_id.clone(),
            character_id: snapshot.character.as_ref().map(|c| c.id.clone()),
        }
    }
}

#[derive(Clone)]
pub(super) struct CharacterInputs {
    pub generation: Arc<()>,
    pub stream_id: String,
    pub character: Option<CharacterView>,
    pub catalog: Option<Arc<Catalog>>,
    pub capture: CaptureStatus,
}

impl CharacterInputs {
    pub fn new(
        generation: Arc<()>,
        stream_id: String,
        mut character: Option<CharacterView>,
        catalog: Option<Arc<Catalog>>,
        capture: CaptureStatus,
    ) -> Self {
        if let Some(character) = &mut character {
            character.calculation_revision =
                Some(input_revision(&stream_id, character, catalog.as_deref()));
        }
        Self {
            generation,
            stream_id,
            character,
            catalog,
            capture,
        }
    }

    fn revision(&self) -> Option<&CharacterInputRevision> {
        self.character
            .as_ref()
            .and_then(|c| c.calculation_revision.as_ref())
    }

    fn matches_snapshot(&self, snapshot: &CharacterIdentity) -> bool {
        snapshot.stream_id == self.stream_id
            && snapshot.character_id.as_ref() == self.character.as_ref().map(|c| &c.id)
    }

    fn same_inputs(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.generation, &other.generation)
            && self.stream_id == other.stream_id
            && self.revision() == other.revision()
            && self.capture.state == other.capture.state
            && match (&self.catalog, &other.catalog) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

/// The epoch changes only for relevant inputs or freshness transitions, including
/// A -> B -> A. Cloned observations retain their own catalog and adapter generation.
pub(super) struct CharacterViewState {
    pub input: Option<Arc<CharacterInputs>>,
    epoch: Arc<()>,
    capture: CaptureState,
}

impl Default for CharacterViewState {
    fn default() -> Self {
        Self {
            input: None,
            epoch: Arc::new(()),
            capture: CaptureState::Stopped,
        }
    }
}

impl CharacterViewState {
    pub fn observe(&mut self, input: CharacterInputs) {
        if self
            .input
            .as_ref()
            .is_none_or(|old| !old.same_inputs(&input))
        {
            self.epoch = Arc::new(());
        }
        self.input = Some(Arc::new(input));
    }

    pub fn capture_changed(&mut self, capture: CaptureState) {
        if self.capture != capture {
            self.capture = capture;
            self.epoch = Arc::new(());
        }
    }
}

impl CaptureRuntime {
    fn character_identity(&self) -> CharacterIdentity {
        // Match snapshot reads: rendezvous with the owner, buffered reply, and
        // final cached view on disconnect. Never hold a lock while waiting.
        let requests = self
            .shared
            .character_requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(requests) = requests {
            let (reply, response) = crossbeam_channel::bounded(1);
            if requests.send(reply).is_ok() {
                if let Ok(identity) = response.recv() {
                    return identity;
                }
            }
        }
        CharacterIdentity::from_snapshot(
            &self
                .shared
                .views
                .snapshot
                .lock()
                .unwrap_or_else(|p| p.into_inner()),
        )
    }

    pub fn get_character_calculation(
        &self,
        request: CharacterCalculationRequest,
    ) -> Result<CharacterCalculation, String> {
        request.validate().map_err(str::to_owned)?;
        // Use the existing engine-owner read path, including metadata-only changes.
        // Neither this wait nor the calculation owns lifecycle or view locks.
        let snapshot = self.character_identity();
        if snapshot.character_id.is_none() {
            return Err("no_local_character".into());
        }
        let (input, epoch, capture) = {
            let view = self
                .shared
                .views
                .character
                .lock()
                .map_err(|_| UNAVAILABLE)?;
            (
                view.input.clone().ok_or("no_local_character")?,
                view.epoch.clone(),
                view.capture.clone(),
            )
        };
        let character = input.character.as_ref().ok_or("no_local_character")?;
        if !input.matches_snapshot(&snapshot)
            || input.revision() != Some(&request.expected_revision)
        {
            return Err(STALE.into());
        }
        for item_type in &request.comparison_item_types {
            let slot = input
                .catalog
                .as_ref()
                .and_then(|catalog| {
                    catalog
                        .items
                        .iter()
                        .find(|item| item.object_type == *item_type)
                })
                .and_then(|item| item.slot_type);
            if slot.is_some_and(|slot| !matches!(slot, 1 | 2 | 3 | 8 | 17 | 24)) {
                return Err("invalid_character_comparison_selection".into());
            }
        }
        let mut result = calculate(
            CharacterInput {
                character,
                stream_id: &input.stream_id,
                capture: &input.capture,
            },
            input.catalog.as_deref(),
            &request,
        );
        #[cfg(test)]
        {
            let hook = self
                .shared
                .after_character_calculation
                .lock()
                .unwrap()
                .take();
            if let Some(hook) = hook {
                hook();
            }
        }
        // An owner read observes pending metadata changes as well as stream changes.
        // No snapshot from either read is joined to an independently loaded catalog.
        let snapshot = self.character_identity();
        let view = self
            .shared
            .views
            .character
            .lock()
            .map_err(|_| UNAVAILABLE)?;
        if !input.matches_snapshot(&snapshot)
            || !Arc::ptr_eq(&epoch, &view.epoch)
            || view
                .input
                .as_ref()
                .is_none_or(|current| !current.same_inputs(&input))
        {
            return Err(STALE.into());
        }
        if result.freshness == CharacterFreshness::Live
            && (capture != CaptureState::Live
                || self.shared.failed.load(Ordering::Acquire)
                || self.shared.bridge_failed.load(Ordering::Acquire)
                || self.closing.load(Ordering::Acquire))
        {
            result.freshness = CharacterFreshness::LastSeen;
        }
        Ok(result)
    }
}

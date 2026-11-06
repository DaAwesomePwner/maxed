import { writable, type Readable } from 'svelte/store';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { CharacterCalculation } from '../bindings/CharacterCalculation';
import type { CharacterCalculationRequest } from '../bindings/CharacterCalculationRequest';
import type { CharacterFreshness } from '../bindings/CharacterFreshness';
import type { CharacterInputRevision } from '../bindings/CharacterInputRevision';
import type { CharacterView } from '../bindings/CharacterView';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { Client } from '../bridge/client';
import type { LiveState } from '../bridge/live';

export interface CharacterViewState {
  observed: CharacterView | null;
  calculation: CharacterCalculation | null;
  freshness: CharacterFreshness | null;
  defenseRaw: string;
  /** Last valid control; displayed results must use calculation.applied_defense. */
  defense: number;
  defenseError: string | null;
  comparisons: number[];
  comparisonError: string | null;
  loading: boolean;
  error: string | null;
}

function revisionKey(revision: CharacterInputRevision) {
  return [revision.stream_id, revision.character_id, revision.input_hash, revision.catalog_fingerprint];
}

/** Owned by the mounted Shell, which supplies its existing live subscription. */
export function createCharacterController(client: Pick<Client, 'getCharacterCalculation'>) {
  let view: CharacterViewState = { observed: null, calculation: null, freshness: null,
    defenseRaw: '0', defense: 0, defenseError: null, comparisons: [], comparisonError: null,
    loading: false, error: null };
  const store = writable(view);
  let identity = '', desiredKey = '', resultKey = '';
  let revision: CharacterInputRevision | null = null;
  let active = false, disposed = false, inFlight = false, generation = 0;
  let unavailable: string | null = null;
  let pending: { request: CharacterCalculationRequest; key: string; generation: number } | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function publish() {
    view = { ...view, freshness: !view.observed ? null : view.observed.died_at !== null ? 'died'
      : view.calculation && active && resultKey === desiredKey && !view.loading && !view.error
        ? view.calculation.freshness : 'last_seen' };
    store.set(view);
  }

  function schedule() {
    if (disposed || inFlight || timer !== undefined || !pending) return;
    // Coalesce bursts in one turn without adding a polling loop or subscription.
    timer = setTimeout(() => { timer = undefined; void run(); }, 0);
  }

  async function run() {
    if (disposed || inFlight || !pending) return;
    const work = pending;
    pending = null;
    inFlight = true;
    try {
      const result = await client.getCharacterCalculation(work.request);
      if (disposed || work.generation !== generation) return;
      if (JSON.stringify(revisionKey(result.revision)) !== JSON.stringify(revisionKey(work.request.expected_revision))
        || result.catalog_fingerprint !== work.request.expected_revision.catalog_fingerprint
        || result.applied_defense !== work.request.defense) {
        view = { ...view, error: 'Calculation inputs changed. Waiting for matching character inputs.' };
        return;
      }
      resultKey = work.key;
      view = { ...view, calculation: result, error: null };
    } catch (error) {
      if (!disposed && work.generation === generation) {
        view = { ...view, error: error instanceof Error ? error.message : 'Character calculation unavailable.' };
      }
    } finally {
      inFlight = false;
      if (!disposed && work.generation === generation) {
        view = { ...view, loading: false };
        publish();
      }
      schedule();
    }
  }

  function reconcile() {
    const request: CharacterCalculationRequest | null = active && revision ? {
      defense: view.defense, comparison_item_types: [...view.comparisons], expected_revision: { ...revision },
    } : null;
    // Native owns the hash of relevant stats/equipment/conditions/identity. Time,
    // fight updates and history durability are deliberately absent from this key.
    const key = request ? JSON.stringify([revisionKey(request.expected_revision), request.defense,
      request.comparison_item_types]) : '';
    if (key !== desiredKey) {
      desiredKey = key;
      generation++;
      pending = request ? { request, key, generation } : null;
      view = { ...view, loading: request !== null, error: unavailable };
      if (!pending) { clearTimeout(timer); timer = undefined; }
      schedule();
    } else if (unavailable) view = { ...view, error: unavailable };
    publish();
  }

  return {
    state: { subscribe: store.subscribe } satisfies Readable<CharacterViewState>,
    update(snapshot: LiveSnapshot | null, capture: CaptureStatus | null, liveState: LiveState) {
      if (disposed) return;
      const character = snapshot?.character ?? null;
      const nextIdentity = JSON.stringify([snapshot?.stream_id ?? null, character?.id ?? null]);
      if (nextIdentity !== identity) {
        identity = nextIdentity;
        resultKey = '';
        view = { ...view, calculation: null, comparisons: [], comparisonError: null, error: null };
      }
      view = { ...view, observed: character ? structuredClone(character) : null };
      const candidate = character?.calculation_revision;
      revision = candidate && candidate.stream_id === snapshot?.stream_id && candidate.character_id === character?.id
        ? { ...candidate } : null;
      unavailable = !character ? 'No character observed. Start capture and enter the game.'
        : !revision ? 'Character calculation unavailable. Reconnect to obtain an input revision.' : null;
      active = !!character && character.died_at === null && capture?.state === 'Live' && liveState.phase === 'live';
      reconcile();
    },
    setDefense(raw: string) {
      if (disposed) return;
      const value = Number(raw.trim());
      const valid = /^\d+$/.test(raw.trim()) && Number.isInteger(value) && value >= 0 && value <= 10_000;
      view = { ...view, defenseRaw: raw, defenseError: valid ? null : 'Enter an integer from 0 to 10,000.' };
      if (valid) { view = { ...view, defense: value }; reconcile(); }
      else publish();
    },
    resetDefense() {
      if (disposed) return;
      view = { ...view, defenseRaw: '0', defense: 0, defenseError: null };
      reconcile();
    },
    setComparisons(types: number[]) {
      if (disposed) return;
      const unique = [...new Set(types)];
      if (unique.length > 2 || unique.some(type => !Number.isInteger(type) || type < 0 || type > 65535)) {
        view = { ...view, comparisonError: 'Choose at most two distinct item types in the range 0–65,535.' };
        publish();
        return;
      }
      view = { ...view, comparisons: unique, comparisonError: null };
      reconcile();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      generation++;
      pending = null;
      clearTimeout(timer);
    },
  };
}

import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
import fixture from '../fixtures/t0-live';

/** Invented DTOs, not decoder replay evidence. Every caller gets fresh nested data. */
export function homeSnapshot(overrides: Partial<LiveSnapshot> = {}): LiveSnapshot {
  const value: LiveSnapshot = structuredClone(fixture);
  value.stream_id = 'live-synthetic-home';
  value.capture = { ...value.capture, state: 'Live', message: 'Synthetic live fixture' };
  value.character = { ...value.character!, identity_verified: true, issues: [] };
  value.run = { ...value.run!, completed_at: null, outcome: null, evidence: [], complete_capture: true, issues: [] };
  value.fights[0].target_kind = 'boss';
  value.fights[0].complete_capture = true;
  value.fights[0].issues = [];
  value.issues = [];
  return Object.assign(value, overrides);
}

export function homeSummary(overrides: Partial<HomeSummary> = {}): HomeSummary {
  const snapshot = homeSnapshot();
  const run = { ...snapshot.run!, ended: '5000000', completed_at: '4000000', outcome: 'cleared' as const };
  return {
    runs: 1, clears: 1, deaths: 0, fame_delta: null, fame: [],
    best_drop: snapshot.drops[0], top_fight: snapshot.fights[0], recent_runs: [run], issues: [],
    recent_details: [{ run_id: run.id, local_share: 1, share_partial: false,
      observed_drop_count: 1, loot: snapshot.drops, loot_preview_limited: false, issues: [] }],
    fame_missing_baselines: 1, fame_sampled: false, unranked_drop_count: 1,
    ...overrides,
  };
}

export function saved(durable: string | null = '12', stream = 'live-synthetic-home'): StoreStatus {
  return { state: 'ready', stream_id: stream, durable, queued: 0, message: '' };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

export async function flush() { for (let i = 0; i < 12; i++) await Promise.resolve(); }

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { CharacterCalculation } from '../../src/lib/bindings/CharacterCalculation';
import type { CharacterCalculationRequest } from '../../src/lib/bindings/CharacterCalculationRequest';
import type { CharacterValue } from '../../src/lib/bindings/CharacterValue';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { LiveState } from '../../src/lib/bridge/live';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createCharacterController } from '../../src/lib/character/controller';
import seed from '../fixtures/t0-live';

const live: LiveState = { phase: 'live', historyStale: false, historyFailed: false,
  historyLagging: false, message: '' };
function snapshot(hash = 'a', stream = 'synthetic-stream', character = 'synthetic-character'): LiveSnapshot {
  const value: LiveSnapshot = structuredClone(seed);
  value.stream_id = stream;
  value.capture.state = 'Live';
  value.character!.id = character;
  value.character!.calculation_revision = { stream_id: stream, character_id: character,
    input_hash: hash, catalog_fingerprint: 'synthetic-catalog' };
  return value;
}
function reply(request: CharacterCalculationRequest): CharacterCalculation {
  const unknown: CharacterValue = { value: null, evidence: 'modeled', completeness: 'unavailable',
    reasons: ['missing_stats'], exclusions: [] };
  return { revision: structuredClone(request.expected_revision), observed_at: '100',
    catalog_fingerprint: request.expected_revision.catalog_fingerprint, formula_version: 'synthetic-v1',
    freshness: 'live', issues: ['missing_stats'], applied_defense: request.defense, weapons: [],
    regeneration: { applied_wis: null, wis: unknown, effects: [], hp_subtotal: unknown, mp_subtotal: unknown,
      hp_total: unknown, mp_total: unknown, base_vit: unknown, base_mp: unknown },
    exalt_damage: { status: 'deferred', value: null, reason: 'deferred' } };
}
function harness() {
  const calls: { request: CharacterCalculationRequest; resolve(value: CharacterCalculation): void;
    reject(reason: Error): void }[] = [];
  const transport = createFixtureTransport(seed, undefined, undefined, undefined, undefined, undefined,
    undefined, { characterCalculation: request => new Promise<CharacterCalculation>((resolve, reject) => {
      calls.push({ request, resolve, reject });
    }) });
  const listen = vi.spyOn(transport, 'listen');
  const controller = createCharacterController(createClient(transport));
  const update = (value = snapshot(), state = live) => controller.update(value, value.capture, state);
  const state = () => get(controller.state);
  const finish = async (index: number, patch: Partial<CharacterCalculation> = {}) => {
    calls[index].resolve({ ...reply(calls[index].request), ...patch });
    await vi.runAllTimersAsync();
  };
  return { controller, calls, listen, update, state, finish };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('Character request controller', () => {
  it('coalesces same-turn updates and replaces pending B with C while A remains the only in-flight request', async () => {
    const h = harness();
    h.update(); h.update(snapshot('a1'));
    await vi.runAllTimersAsync();
    expect(h.calls).toHaveLength(1);
    expect(h.calls[0].request.expected_revision.input_hash).toBe('a1');
    h.update(snapshot('b')); h.controller.setDefense('30');
    h.update(snapshot('c')); h.controller.setDefense('40'); h.controller.setComparisons([201]);
    await vi.runAllTimersAsync();
    expect(h.calls).toHaveLength(1);
    await h.finish(0);
    expect(h.state().calculation).toBeNull();
    expect(h.calls).toHaveLength(2);
    expect(h.calls[1].request).toEqual({ defense: 40, comparison_item_types: [201], expected_revision: {
      stream_id: 'synthetic-stream', character_id: 'synthetic-character', input_hash: 'c',
      catalog_fingerprint: 'synthetic-catalog',
    } });
    await h.finish(1);
    expect(h.state().calculation?.revision.input_hash).toBe('c');
    expect(h.state().loading).toBe(false);
    expect(h.listen).not.toHaveBeenCalled();
  });

  it('ignores unrelated fight, time, sequence and history updates but recalculates changed input and catalog revisions', async () => {
    const h = harness(); h.update(); await vi.runAllTimersAsync(); await h.finish(0);
    const next = snapshot(); next.fights = []; next.sequence = '999'; next.at = '900';
    next.character!.observed_at = '900';
    h.update(next, { ...live, historyStale: true, historyFailed: true });
    await vi.runAllTimersAsync(); expect(h.calls).toHaveLength(1);
    for (const hash of ['stats', 'equipment', 'conditions', 'identity']) {
      h.update(snapshot(hash)); await vi.runAllTimersAsync(); await h.finish(h.calls.length - 1);
    }
    const catalog = snapshot('identity'); catalog.character!.calculation_revision!.catalog_fingerprint = 'catalog-2';
    h.update(catalog); await vi.runAllTimersAsync();
    expect(h.calls).toHaveLength(6);
    expect(h.calls[5].request.expected_revision.catalog_fingerprint).toBe('catalog-2');
  });

  it.each(['stream', 'character'] as const)('clears results and picks immediately on a %s switch and ignores the previous reply', async kind => {
    const h = harness(); h.update(); h.controller.setComparisons([201, 202]);
    await vi.runAllTimersAsync(); await h.finish(0);
    h.controller.setDefense('20'); await vi.runAllTimersAsync();
    h.update(snapshot('b', kind === 'stream' ? 'synthetic-new-stream' : undefined,
      kind === 'character' ? 'synthetic-new-character' : undefined));
    expect(h.state().calculation).toBeNull(); expect(h.state().comparisons).toEqual([]);
    expect(h.state().defense).toBe(20);
    await h.finish(1);
    expect(h.state().calculation).toBeNull();
    expect(h.calls[2].request.comparison_item_types).toEqual([]);
    await h.finish(2); expect(h.state().calculation?.revision.input_hash).toBe('b');
  });

  it.each(['revision', 'catalog', 'defense'] as const)('discards a mismatched reply %s without retrying or fabricating values', async mismatch => {
    const h = harness(); h.update(); await vi.runAllTimersAsync();
    const result = reply(h.calls[0].request);
    if (mismatch === 'revision') result.revision.input_hash = 'older';
    if (mismatch === 'catalog') result.catalog_fingerprint = 'older';
    if (mismatch === 'defense') result.applied_defense = 99;
    await h.finish(0, result);
    expect(h.state().calculation).toBeNull(); expect(h.state().freshness).not.toBe('live');
    expect(h.state().error).toBeTruthy(); expect(h.calls).toHaveLength(1);
  });

  it('ignores obsolete rejections, preserves observations on current errors and does not retry on fight updates', async () => {
    const h = harness(); h.update(); await vi.runAllTimersAsync(); h.update(snapshot('b'));
    h.calls[0].reject(new Error('obsolete')); await vi.runAllTimersAsync();
    expect(h.state().error).toBeNull();
    h.calls[1].reject(new Error('Weapon calculations unavailable — extract game assets'));
    await vi.runAllTimersAsync();
    expect(h.state().error).toContain('extract game assets');
    expect(h.state().observed?.equipment).toEqual(seed.character.equipment);
    h.update(snapshot('b')); await vi.runAllTimersAsync(); expect(h.calls).toHaveLength(2);
  });

  it('exposes unavailable for old or inconsistent revision DTOs and clears when the character disappears', async () => {
    const h = harness();
    const old = snapshot(); delete old.character!.calculation_revision;
    h.update(old); await vi.runAllTimersAsync();
    expect(h.calls).toHaveLength(0); expect(h.state().error).toMatch(/reconnect/i);
    const wrong = snapshot(); wrong.character!.calculation_revision!.stream_id = 'old-stream';
    h.update(wrong); await vi.runAllTimersAsync(); expect(h.calls).toHaveLength(0);
    h.update(); await vi.runAllTimersAsync(); await h.finish(0);
    h.controller.setComparisons([201]); const empty = snapshot(); empty.character = null; h.update(empty);
    await vi.runAllTimersAsync();
    expect(h.state().calculation).toBeNull(); expect(h.state().comparisons).toEqual([]);
    expect(h.state().observed).toBeNull(); expect(h.state().error).toMatch(/No character/);
  });

  it.each(['Stopped', 'Recovering', 'Failed', 'resyncing', 'connecting', 'failed', 'died'] as const)
  ('freezes the last result during %s and cannot be revived by a pending reply', async phase => {
    const h = harness(); h.update(); await vi.runAllTimersAsync(); await h.finish(0);
    const previous = h.state().calculation;
    h.controller.setDefense('50'); await vi.runAllTimersAsync();
    const next = snapshot(); let state = live;
    if (phase === 'died') next.character!.died_at = '200';
    else if (phase === 'connecting' || phase === 'resyncing' || phase === 'failed') state = { ...live, phase };
    else next.capture.state = phase;
    h.update(next, state);
    expect(h.state().freshness).toBe(phase === 'died' ? 'died' : 'last_seen');
    await h.finish(1);
    expect(h.state().calculation).toEqual(previous);
    expect(h.state().freshness).toBe(phase === 'died' ? 'died' : 'last_seen');
    h.controller.setDefense('60'); await vi.runAllTimersAsync(); expect(h.calls).toHaveLength(2);
    h.update(); await vi.runAllTimersAsync(); expect(h.calls).toHaveLength(3);
    expect(h.state().freshness).toBe('last_seen');
    await h.finish(2); expect(h.state().freshness).toBe('live');
  });

  it('keeps an unchanged revision Last seen through reconnect until a fresh reply arrives', async () => {
    const h = harness(); h.update(); await vi.runAllTimersAsync(); await h.finish(0);
    h.update(snapshot(), { ...live, phase: 'resyncing' });
    expect(h.state().freshness).toBe('last_seen');
    h.update(); await vi.runAllTimersAsync();
    expect(h.state().freshness).toBe('last_seen');
    expect(h.calls).toHaveLength(2);
    await h.finish(1); expect(h.state().freshness).toBe('live');
  });

  it('invalidates an in-flight reply across reconnect even when the revision returns unchanged', async () => {
    const h = harness(); h.update(); await vi.runAllTimersAsync();
    h.update(snapshot(), { ...live, phase: 'resyncing' }); h.update();
    await h.finish(0);
    expect(h.state().calculation).toBeNull(); expect(h.calls).toHaveLength(2);
    await h.finish(1); expect(h.state().freshness).toBe('live');
  });

  it('preserves native stale freshness and independent incomplete calculation values', async () => {
    const h = harness(); h.update(); await vi.runAllTimersAsync(); await h.finish(0, { freshness: 'last_seen' });
    h.update(); expect(h.state().freshness).toBe('last_seen');
    expect(h.state().calculation?.regeneration.hp_total.value).toBeNull();
  });

  it('disposes queued and in-flight work without late emissions or new requests', async () => {
    const h = harness(); h.update(); await vi.runAllTimersAsync(); h.update(snapshot('b'));
    const changed = vi.fn(); const stop = h.controller.state.subscribe(changed);
    h.controller.dispose(); const count = changed.mock.calls.length;
    await h.finish(0); h.update(snapshot('c')); h.controller.setDefense('40');
    h.controller.setComparisons([201]); h.controller.resetDefense(); await vi.runAllTimersAsync();
    expect(changed).toHaveBeenCalledTimes(count); expect(h.calls).toHaveLength(1); stop();
    const queued = harness(); queued.update(); queued.controller.dispose();
    await vi.runAllTimersAsync(); expect(queued.calls).toHaveLength(0);
  });
});

describe('Character controls', () => {
  it('defaults to zero, accepts both boundaries, resets to zero and retains the result applied defense while pending', async () => {
    const h = harness(); h.update(); await vi.runAllTimersAsync();
    expect(h.calls[0].request.defense).toBe(0); await h.finish(0);
    h.controller.setDefense('10000'); await vi.runAllTimersAsync();
    expect(h.state().calculation?.applied_defense).toBe(0); expect(h.state().freshness).toBe('last_seen');
    await h.finish(1); expect(h.state().calculation?.applied_defense).toBe(10000);
    h.controller.resetDefense(); await vi.runAllTimersAsync();
    expect(h.calls[2].request.defense).toBe(0); expect(h.state().defenseRaw).toBe('0');
    await h.finish(2); expect(h.state().calculation?.applied_defense).toBe(0);
    expect(harness().state().defense).toBe(0);
  });

  it.each(['', ' ', '1.5', 'abc', '-1', 'Infinity', 'NaN', '10001', '1e3', '0x10'])
  ('rejects defense %j without clamping, extra requests or relabelling the last valid result', async raw => {
    const h = harness(); h.update(); h.controller.setDefense('25'); await vi.runAllTimersAsync(); await h.finish(0);
    h.controller.setDefense(raw); await vi.runAllTimersAsync();
    expect(h.state().defenseRaw).toBe(raw); expect(h.state().defenseError).toBeTruthy();
    expect(h.state().defense).toBe(25); expect(h.state().calculation?.applied_defense).toBe(25);
    expect(h.calls).toHaveLength(1);
    h.controller.setDefense('25'); await vi.runAllTimersAsync();
    expect(h.state().defenseError).toBeNull(); expect(h.calls).toHaveLength(1);
  });

  it('deduplicates comparisons, bounds them to two valid item types, and preserves authored equipped-first rows', async () => {
    const h = harness(); h.update(); h.controller.setComparisons([201, 201, 202]);
    await vi.runAllTimersAsync(); expect(h.calls[0].request.comparison_item_types).toEqual([201, 202]);
    for (const types of [[201, 202, 203], [-1], [65536], [1.5], [NaN]]) {
      h.controller.setComparisons(types); expect(h.state().comparisonError).toBeTruthy();
      expect(h.state().comparisons).toEqual([201, 202]);
    }
    await vi.runAllTimersAsync(); expect(h.calls).toHaveLength(1);
    const value = reply(h.calls[0].request).regeneration.hp_total;
    const row = { item_type: 200, groups: [], applied_att: null, applied_dex: null, applied_primary_mask: null,
      rate_factor: value, catalog_range: { min: null, max: null, evidence: 'catalog' as const,
        completeness: 'unavailable' as const, reasons: [], exclusions: [] },
      hit_range: { min: null, max: null, evidence: 'modeled' as const,
        completeness: 'unavailable' as const, reasons: [], exclusions: [] },
      hit_mean: value, attacks_per_sec: value, projectiles_per_sec: value, dps_subtotal: value, dps_total: value };
    await h.finish(0, { weapons: [row, { ...row, item_type: 201 }, { ...row, item_type: 202 }] });
    expect(h.state().calculation?.weapons.map(weapon => weapon.item_type)).toEqual([200, 201, 202]);
    h.controller.setComparisons([201, 202]); await vi.runAllTimersAsync(); expect(h.calls).toHaveLength(1);
    expect(h.state().comparisonError).toBeNull();
    h.controller.setComparisons([]); await vi.runAllTimersAsync();
    expect(h.calls[1].request.comparison_item_types).toEqual([]);
  });
});

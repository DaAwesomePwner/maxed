import { setDebugMode } from '../../src/lib/debug/debug';
import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { DamageBucket } from '../../src/lib/bindings/DamageBucket';
import type { PlayerSeries } from '../../src/lib/bindings/PlayerSeries';
import type { HomeLootProps, HomeLiveWindow } from '../../src/lib/home/v2/contracts';
import { lootItems, lootView } from '../../src/lib/home/v2/loot';
import DamageChart from '../../src/lib/home/v2/DamageChart.svelte';
import { characterVisualTypes, createHomeVisuals, homeVisualTypes, type HomeVisuals } from '../../src/lib/home/visuals';
import { deferred, flush, homeSnapshot } from '../home/fixtures';

afterEach(() => setDebugMode(false));

const tiers = new Map<number, string | null>([[10, 'UT'], [11, 'ST'], [12, '12'], [13, '2'], [14, null], [15, 'UT']]);
const visuals: HomeVisuals = { phase: 'ready', client_fingerprint: 'a'.repeat(64),
  objects: [...tiers].map(([object_type, item_tier]) => ({ object_type, item_tier, name: `Item ${object_type}`, sprite: null, bag: null })) };
function bag(id: string, rank: number | null, types: number[], overrides: Partial<DropView> = {}): DropView {
  return { ...homeSnapshot().drops[0], id, bag_rank: rank, items: types,
    contents: types.map((item_type, slot) => ({ item_type, slot, quantity: 1,
      enchant: { state: 'missing', slots: null, applied: null, ids: [] }, taken_observed_at: null })),
    contents_known: true, source_enemy: null, source_estimated: false, source_reason: 'source-not-observed', issues: [], ...overrides };
}
const input = (drops: DropView[]): HomeLootProps => ({ drops, visuals, partial: false, reasons: [], title: 'Visit Loot' });
const buckets: DamageBucket[] = [
  { from: '9007199254740993000', until: '9007199254741993000', total: 100, local: 20 },
  { from: '9007199254741993000', until: '9007199254743993000', total: 300, local: null },
  { from: '9007199254743993000', until: '9007199254744493000', total: 100, local: 0 },
];
const series: PlayerSeries = { player_id: 'self', object_id: 1, is_local: true, points: [10, 100, 25] };

describe('bag_order_tier_null_and_revisions', () => {
  it('orders supplied bag ranks, then tiers and IDs; a revision replaces rather than adds quantities', () => {
    const original = bag('revision', 10, [15]);
    const revision = bag('revision', 255, [10, 10, 14]);
    revision.contents[0].quantity = 3;
    revision.contents[1].taken_observed_at = revision.at;
    const drops = [original, bag('unknown', null, [10]), bag('b', 255, [11]), bag('a', 255, [11]),
      bag('numeric-low', 255, [13]), bag('numeric-high', 255, [12]), bag('null-tier', 255, [14]), revision];
    const before = structuredClone(drops);
    const view = lootView(input(drops));
    expect(view.drops.map(row => row.id)).toEqual(['revision', 'a', 'b', 'numeric-high', 'numeric-low', 'null-tier', 'unknown']);
    expect(view).toMatchObject({ bagCount: 7, utCount: 5, stCount: 2, unclassifiedCount: 2,
      bestUt: { drop: { id: 'revision' }, itemType: 10 }, bestSt: { drop: { id: 'a' }, itemType: 11 }, partial: true });
    expect(lootItems(revision, visuals).map(row => [row.slot, row.quantity, row.takenAt])).toEqual([[0, 3, null], [1, 1, revision.at], [2, 1, null]]);
    expect(drops).toEqual(before);
    const order = [10, 20, 30, 40, 50, 60, 70, 80, 90, 255];
    expect(lootView(input(order.map(rank => bag(String(rank), rank, [])))).drops.map(drop => drop.bag_rank)).toEqual(order.reverse());
  });

  it('preserves unseen, legacy, extended and source evidence without inferring a tier or ownership', () => {
    const unseen = bag('unseen', null, [10], { contents: [], contents_known: false, capture_scope: 'extended',
      source_estimated: false, source_reason: 'source-not-observed', issues: ['stream_gap'] });
    const legacy = bag('legacy', 90, [10, 10], { contents: [] });
    const view = lootView(input([unseen, legacy]));
    expect(view.utCount).toBe(2);
    expect(view.reasons).toEqual(expect.arrayContaining(['Some bag contents were not fully observed.', 'The source was not observed', 'Some traffic was missed']));
    const missing = lootView({ ...input([bag('missing', 255, [10])]), visuals: { ...visuals, objects: [] } });
    expect(missing).toMatchObject({ bestUt: null, utCount: 0, unclassifiedCount: 1, partial: true });
  });

  it('keeps informational source and ordering notes separate from partial coverage', () => {
    const complete = bag('complete', 255, [10, 11, 12]);
    expect(lootView(input([complete])).partial).toBe(false);
    const unranked = bag('unranked', null, [10]);
    const estimated = bag('estimated', 90, [11], { source_enemy: 300, source_estimated: true, source_reason: 'nearest-removed-hit-target' });
    const known = bag('known', 80, [12], { source_enemy: 301, source_reason: 'killed-target-same-tick' });
    const props = input([complete, unranked, estimated, known]);
    expect(lootView(props)).toMatchObject({ partial: false, reasons: [
      'The source was not observed', 'Source inferred from the nearest recently killed enemy that was hit', 'Unranked bags follow ranked bags.',
    ] });
    expect(lootView({ ...input([complete]), partial: true }).partial).toBe(true);
    expect(lootView(input([{ ...complete, contents_known: false }])).partial).toBe(true);
    expect(lootView(input([{ ...complete, issues: ['stream_gap'] }])).partial).toBe(true);
    expect(lootView(input([{ ...complete, contents: [] }])).partial).toBe(true);
    expect(lootView(input([{ ...complete, contents: [{ ...complete.contents[0], quantity: 0 }] }])).partial).toBe(true);
  });
});

describe('chart_bucket_duration_null_and_series_cap', () => {
  it.each(['total', 'local', 'player'] as const)('includes an omitted %s interval in peak DPS', key => {
    const many = Array.from({ length: 250 }, (_, i) => ({ from: String(i * 1_000_000), until: String((i + 1) * 1_000_000), total: 10, local: 5 }));
    const selected = { ...series, points: many.map(() => 2) };
    // Index 1 falls between displayed indexes 0 and 2 in the 120-interval sample.
    if (key === 'player') selected.points[1] = 999;
    else many[1][key] = 999;
    // A larger amount in an invalid-duration interval must not become the peak.
    many[3] = { ...many[3], until: many[3].from, total: 99999, local: 99999 };
    selected.points[3] = 99999;
    const page = render(DamageChart, { buckets: many, playerSeries: [selected], selectedPlayerId: 'self' });
    expect(page.container.querySelectorAll('option')).toHaveLength(120);
    expect([...page.container.querySelectorAll('option')].some(option => option.textContent?.startsWith('2:'))).toBe(false);
    expect(page.container.textContent?.replace(/\s+/g, ' ')).toContain('Peak 999 DPS');
  });

  it('uses exact microsecond deltas, positive durations and matching player indexes; null breaks only that line', async () => {
    const page = render(DamageChart, { buckets, playerSeries: [series], selectedPlayerId: 'self' });
    const path = (name: string) => page.container.querySelector(`path.${name}`)!.getAttribute('d')!;
    expect(path('party').match(/M/g)).toHaveLength(1);
    expect(path('local').match(/M/g)).toHaveLength(2);
    expect(path('player').match(/M/g)).toHaveLength(1);
    page.container.querySelector('details')!.open = true;
    expect(page.container.textContent?.replace(/\s+/g, ' ')).toContain('1s · Party 100 DPS · Local 20 DPS · Player 10 DPS');
    await fireEvent.change(screen.getByLabelText('Interval'), { target: { value: '2' } });
    expect(page.container.textContent?.replace(/\s+/g, ' ')).toContain('0.5s · Party 200 DPS · Local 0 DPS · Player 50 DPS');
    await page.rerender({ buckets: [{ ...buckets[0], until: buckets[0].from }], playerSeries: null, selectedPlayerId: 'self' });
    expect(page.container.textContent?.replace(/\s+/g, ' ')).toContain('No DPS');
    expect(page.container.textContent).not.toContain('no positive-duration buckets');
    expect(page.container.textContent?.replace(/\s+/g, ' ')).toContain('Party — DPS · Local — DPS · Player — DPS');
    expect(page.container.querySelector('svg')).toBeNull();
  });

  it.each([null, [], [series, series], [{ ...series, player_id: 'other' }], [{ ...series, points: [10] }]])('rejects omitted, ambiguous or misaligned selected series %#', playerSeries => {
    const page = render(DamageChart, { buckets, playerSeries, selectedPlayerId: 'self' });
    expect(page.container.querySelector('path.player')!.getAttribute('d')).toBe('');
    expect(page.container.querySelector('path.party')!.getAttribute('d')).not.toBe('');
    expect(page.container.textContent?.replace(/\s+/g, ' ')).toContain('Player — DPS');
    expect(page.container.textContent).not.toMatch(/series unavailable/);
  });

  it('preserves omitted temporal intervals and exposes keyboard-readable bucket values', async () => {
    const many = Array.from({ length: 250 }, (_, i) => ({ from: String(i * 1_000_000), until: String((i + 1) * 1_000_000), total: i, local: i }));
    const page = render(DamageChart, { buckets: many, playerSeries: null, selectedPlayerId: null });
    expect(page.container.querySelectorAll('option')).toHaveLength(120);
    expect(page.container.querySelector('option:last-child')!.textContent).toContain('250:');
    expect(page.container.querySelector('path.party')!.getAttribute('d')!.match(/M/g)!.length).toBeGreaterThan(1);
    await page.rerender({ buckets: [buckets[0], buckets[2]], playerSeries: null, selectedPlayerId: null });
    expect(page.container.querySelector('path.party')!.getAttribute('d')!.match(/M/g)).toHaveLength(2);
    await page.rerender({ buckets, playerSeries: [series], selectedPlayerId: 'self' });
    expect(screen.getByRole('img', { name: /Fight DPS/ })).toBeTruthy();
    const details = page.container.querySelector('details')!;
    details.open = true;
    await fireEvent.change(screen.getByLabelText('Interval'), { target: { value: '1' } });
    expect(page.container.textContent?.replace(/\s+/g, ' ')).toContain('Party 150 DPS · Local — DPS · Player 50 DPS');
    await page.rerender({ buckets, playerSeries: null, selectedPlayerId: 'self' });
    expect(page.container.querySelector('path.player')?.getAttribute('d')).toBe('');
    expect(page.container.textContent).not.toContain('snapshot series cap');
    setDebugMode(true);
    await fireEvent.change(screen.getByLabelText('Interval'), { target: { value: '0' } });
    expect(page.container.textContent).toContain('snapshot series cap');
  });
});

describe('extended_visual_collection_preserves_request_bounds', () => {
  it('collects gear, classes, targets and visit rows without unscoped Realm loot or numeric tick requests', async () => {
    const snapshot = homeSnapshot();
    snapshot.character!.equipment = [{ slot: 0, item_type: 500, enchant: snapshot.character!.equipment[0].enchant },
      { slot: 4, item_type: 999, enchant: snapshot.character!.equipment[0].enchant }];
    snapshot.fights[0].players[0].class_id = 501;
    const window: HomeLiveWindow = { streamId: snapshot.stream_id, visitKey: 'visit:1', visitStarted: snapshot.at, runId: snapshot.run!.id,
      fights: [{ ...snapshot.fights[0], target_type: 502 }], drops: [bag('visit', 255, [503], { bag_type: 504, source_enemy: 505 })],
      events: [{ id: 'kill', session_id: snapshot.session!.id, at: snapshot.at, provisional: true,
        kind: { type: 'realm_event_killed', boss_key: 'observed', object_type: 506 } }], partial: false, reasons: [] };
    const types = homeVisualTypes(snapshot, null, window);
    expect(types).toEqual(expect.arrayContaining([100, 500, 501, 502, 503, 504, 505, 506]));
    expect(types).not.toContain(999);
    expect(characterVisualTypes(snapshot.character)).toEqual([100, 500]);
    const getObjectVisuals = vi.fn(async (types: number[]) => ({ client_fingerprint: 'a'.repeat(64), objects: types.map(object_type => ({ object_type, name: null, sprite: null, bag: null })) }));
    const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: () => () => {} }, () => {});
    try {
      loader.update(types); await flush();
      snapshot.fights[0].players[0].damage += 999;
      loader.update(homeVisualTypes(snapshot, null, window)); await flush();
      expect(getObjectVisuals).toHaveBeenCalledTimes(1);
      snapshot.run = null; snapshot.area_kind = 'realm'; snapshot.drops = [bag('unscoped', 255, [600])];
      expect(homeVisualTypes(snapshot, null)).not.toContain(600);
    } finally { loader.dispose(); }
  });

  it('serializes 128-type pages, ignores retired responses and refreshes the fingerprint generation', async () => {
    const pending = deferred<{ client_fingerprint: string; objects: [] }>();
    let invalidate = () => {};
    const release = vi.fn(), states: HomeVisuals[] = [];
    const getObjectVisuals = vi.fn(async (_types: number[]) => ({ client_fingerprint: 'b'.repeat(64), objects: [] })).mockImplementationOnce(() => pending.promise);
    const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: callback => { invalidate = callback; return release; } }, value => states.push(value));
    try {
      loader.update([1]); await flush();
      loader.update(Array.from({ length: 260 }, (_, i) => i + 100));
      expect(getObjectVisuals).toHaveBeenCalledTimes(1);
      pending.resolve({ client_fingerprint: 'a'.repeat(64), objects: [] }); await flush();
      expect(getObjectVisuals.mock.calls.map(([types]) => types.length)).toEqual([1, 128, 128, 4]);
      expect(states.filter(state => state.phase === 'ready').map(state => state.client_fingerprint)).toEqual(['b'.repeat(64)]);
      invalidate(); await flush();
      expect(getObjectVisuals.mock.calls.map(([types]) => types.length)).toEqual([1, 128, 128, 4, 128, 128, 4]);
    } finally { loader.dispose(); }
    expect(release).toHaveBeenCalledOnce();
  });
});

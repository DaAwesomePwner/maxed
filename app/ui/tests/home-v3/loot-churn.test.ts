import { render } from '@testing-library/svelte';
import RealmHome from '../../src/lib/home/RealmHome.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { subscribeLive } from '../../src/lib/bridge/live';
import { expect, it, vi } from 'vitest';
import { homeSnapshot } from '../home/fixtures';
import { realmSnapshot } from '../home-v2/fixtures';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { lootItems, lootView } from '../../src/lib/home/v2/loot';
import { homeLoot, rankedDrops, bestDrop, overflowSummary } from '../../src/lib/home/v3/loot-rank';
import { lootCounts } from '../../src/lib/home/v3/screen-data';
import { objectVisual, visualIndex } from '../../src/lib/bridge/visual-index';
import { realmView } from '../../src/lib/home/realm';
import { dungeonObservations, dungeonView } from '../../src/lib/home/dungeon';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import * as old from './loot-rank-oracle';
import * as oldLoot from './loot-oracle';
import { emptyDrops } from '../../src/lib/home/drop-data';
import type { DropView } from '../../src/lib/bindings/DropView';

const visuals = () => ({ ...emptyHomeVisuals(), objects: ['UT', 'ST', 'T14', '9', null, 'T9007199254740993', 'bogus'].map((item_tier, object_type) =>
  ({ object_type, item_tier, name: `Synthetic ${object_type}`, sprite: null, bag: null })) });
const bag = (id: string, types = [0, 2]): DropView => ({ ...homeSnapshot().drops[0], id, contents_known: true,
  contents: types.map((item_type, slot) => ({ item_type, slot, quantity: 1, taken_observed_at: null, enchant: { state: 'missing' as const, slots: null, applied: null, ids: [] } })), items: types });

it('reuses every heavy projection without reading drops again, and invalidates changed/new/removed bags and visuals', () => {
  const a = bag('a'), b = bag('b', [1]), drops = [a, b], catalog = visuals();
  const contents = a.contents;
  const read = vi.fn(() => contents);
  Object.defineProperty(a, 'contents', { get: read });
  const initial = homeLoot(drops, catalog), items = lootItems(a, catalog);
  const evidence = lootView({ drops, visuals: catalog, partial: false, reasons: [], title: 'Test' });
  const overflow = overflowSummary(drops);
  read.mockClear();
  expect(homeLoot(drops, catalog)).toBe(initial);
  expect(bestDrop(drops, catalog)).toBe(initial.best);
  expect(lootCounts(drops, catalog)).toBe(initial.counts);
  expect(rankedDrops(drops)).toBe(initial.ranked);
  expect(rankedDrops(initial.bags)).toBe(initial.bags);
  expect(overflowSummary(drops)).toBe(overflow);
  expect(lootItems(a, catalog)).toBe(items);
  expect(lootView({ drops, visuals: catalog, partial: true, reasons: ['New reason'], title: 'Test' }).drops).toBe(evidence.drops);
  expect(read).not.toHaveBeenCalled();
  for (const next of [[{ ...a, bag_rank: 255 }, b], [...drops, bag('c')], [b]]) {
    expect(homeLoot(next, catalog)).not.toBe(initial);
    expect(bestDrop(next, catalog)).toEqual(old.bestDrop(next, catalog));
  }
  const changedVisuals = { ...catalog, objects: catalog.objects.map(row => ({ ...row, item_tier: 'ST' })) };
  expect(homeLoot(drops, changedVisuals)).not.toBe(initial);
  expect(lootItems(a, changedVisuals)).not.toBe(items);
  expect(bestDrop(drops, changedVisuals)).toEqual(old.bestDrop(drops, changedVisuals));
});

it('matches the documented ranking, items, counts and evidence on randomized inputs', () => {
  let seed = 40;
  const random = (n: number) => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % n; };
  const catalog = visuals();
  for (let trial = 0; trial < 150; trial++) {
    const drops = Array.from({ length: random(45) }, (_, i) => {
      const row = bag(`bag-${random(25)}`, Array.from({ length: random(8) }, () => random(9)));
      return { ...row, bag_rank: [null, -1, 0, 40, 90, 255, NaN, Infinity][random(8)], bag_type: random(5),
        at: ['bad', '-1', '0', '001', '9007199254740993', '9007199254740994'][random(6)],
        contents_known: !!random(2), contents: random(3) ? row.contents.map(item => ({ ...item,
          quantity: [0, 1, 3, -2, 1.5, 2][random(6)] })) : [],
        source_estimated: !!random(2), issues: i % 7 === 0 ? ['midstream' as const] : [] };
    });
    const before = structuredClone(drops);
    expect(rankedDrops(drops)).toEqual(old.rankedDrops(drops));
    expect(bestDrop(drops, catalog)).toEqual(old.bestDrop(drops, catalog));
    expect(overflowSummary(drops)).toEqual(old.overflowSummary(drops));
    for (const drop of drops) expect(lootItems(drop, catalog)).toEqual(oldLoot.lootItems(drop, catalog));
    const items = [...new Map(drops.map(row => [row.id, row])).values()].flatMap(row => oldLoot.lootItems(row, catalog));
    expect(lootCounts(drops, catalog)).toEqual({ bags: new Set(drops.map(row => row.id)).size,
      ut: items.filter(item => item.tier === 'UT').length, st: items.filter(item => item.tier === 'ST').length, items: items.length });
    for (const partial of [false, true]) for (const reasons of [[], ['Synthetic coverage reason']]) {
      const input = { drops, visuals: catalog, partial, reasons, title: 'Test' };
      expect(lootView(input)).toEqual(oldLoot.lootView(input));
    }
    expect(drops).toEqual(before);
  }
});

it('retains first visual matches and last tier entries and invalidates a replaced catalog', () => {
  const first = visuals(), last = { ...first.objects[0], item_tier: 'ST' };
  const duplicate = { ...first, objects: [...first.objects, last] };
  expect(objectVisual(duplicate.objects, 0)).toBe(first.objects[0]);
  expect(visualIndex(duplicate.objects)).toBe(visualIndex(duplicate.objects));
  expect(lootItems(bag('a', [0]), duplicate)[0].tier).toBe('ST');
});

it('keeps realm drop identity through numeric/fame ticks but scopes changed visit boundaries', () => {
  const snapshot = realmSnapshot(), drop = { ...bag('a'), run_id: null, session_id: snapshot.session!.id, at: '200' };
  const window = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, visitKey: 'visit', visitStarted: '100', drops: [drop],
    realmPeriods: [{ started: '100', ended: null as string | null, fame: 1 }] };
  const first = realmView(snapshot, window).drops;
  expect(first).toBe(window.drops);
  expect(realmView({ ...snapshot, at: '400' }, { ...window, realmPeriods: [{ ...window.realmPeriods[0], fame: 2 }] }).drops).toBe(first);
  expect(realmView(snapshot, { ...window, realmPeriods: [{ started: '100', ended: '150', fame: 1 }] }).drops).toEqual([]);
  expect(realmView({ ...snapshot, stream_id: 'other' }, window).drops).toEqual([]);
  expect(realmView({ ...snapshot, session: { ...snapshot.session!, id: 'other-session' } }, window).drops).toEqual([]);
});

it('keeps dungeon drop identity and rechecks run/session scope', () => {
  const snapshot = homeSnapshot(), first = dungeonObservations(snapshot, null).drops;
  expect(dungeonObservations({ ...snapshot, at: '9000000' }, null).drops).toBe(first);
  expect(dungeonView({ ...snapshot, at: '9000000' }).drops).toBe(dungeonView(snapshot).drops);
  expect(dungeonObservations({ ...snapshot, run: { ...snapshot.run!, session_id: 'other' } }, null).drops).toEqual([]);
});

it('preserves published visit drops across numeric updates and emits new revisions without changing retained history', () => {
  const source = homeSnapshot(); let window = emptyHomeLiveWindow();
  const controller = createHomeLiveWindow(value => { window = value; });
  controller.snapshot(source);
  const first = window.drops;
  controller.snapshot({ ...source, sequence: String(BigInt(source.sequence) + 1n) });
  expect(window.drops).toBe(first);
  const revised = { ...source.drops[0], contents_known: !source.drops[0].contents_known };
  controller.snapshot({ ...source, sequence: String(BigInt(source.sequence) + 2n), drops: [revised] });
  expect(window.drops).not.toBe(first);
  expect(window.drops[0]).toBe(revised);
  expect(first[0]).toBe(source.drops[0]);
  controller.dispose();
});

it('freezes shared empty drops and cached loot projections in development', () => {
  const drops = [bag('a')], catalog = visuals(), result = homeLoot(drops, catalog);
  const rows = lootItems(drops[0], catalog);
  const evidence = lootView({ drops, visuals: catalog, partial: false, reasons: [], title: 'Test' });
  expect(() => emptyDrops.push(drops[0])).toThrow();
  expect(() => result.ranked.pop()).toThrow();
  expect(() => result.bags.push(drops[0])).toThrow();
  expect(() => { result.counts.ut++; }).toThrow();
  expect(() => rows.pop()).toThrow();
  expect(() => { rows[0].tier = 'ST'; }).toThrow();
  expect(() => evidence.drops.pop()).toThrow();
});

it('revises a same-ID best bag from UT to ST through subscription, visit window and RealmHome', async () => {
  const source = realmSnapshot(); source.sequence = '0'; source.at = '100';
  const transport = createFixtureTransport(source), catalog = visuals();
  let window = emptyHomeLiveWindow();
  const page = render(RealmHome, { snapshot: source, window, visuals: catalog });
  const live = createHomeLiveWindow(value => { window = value; void page.rerender({ window }); });
  let receivedRevision: import('../../src/lib/bindings/DropView').DropView | null = null;
  const connection = subscribeLive(createClient(transport), { snapshot: live.snapshot, event: envelope => {
    if ('drop_updated' in envelope.event) receivedRevision = envelope.event.drop_updated;
    live.event(envelope);
  }, batch: live.batch, state: live.state });
  const settle = async () => { for (let i = 0; i < 40; i++) await Promise.resolve(); };
  try {
    await settle();
    transport.publishEngineEvent({ stream_id: source.stream_id, sequence: '1', at: '100', event: {
      map_entered: { name: 'Synthetic Realm', kind: 'realm', at: '100', realm_instance_key: 'realm-a' } } });
    const drop = { ...bag('same-id', [0]), run_id: null, session_id: source.session!.id, at: '150' };
    transport.publishEngineEvent({ stream_id: source.stream_id, sequence: '2', at: '150', event: { bag_dropped: drop } });
    await settle();
    expect(page.container.querySelector('.best-card')?.getAttribute('data-tier')).toBe('UT');
    const revised = { ...drop, items: [1], contents: bag('same-id', [1]).contents };
    transport.publishEngineEvent({ stream_id: source.stream_id, first_sequence: '3', last_sequence: '3', events: [
      { stream_id: source.stream_id, sequence: '3', at: '160', event: { drop_updated: revised } }] });
    await settle();
    // Fixture transport clones at the wire boundary, just like native JSON.
    expect(receivedRevision).toEqual(revised);
    expect(window.drops[0]).toBe(receivedRevision);
    expect(page.container.querySelector('.best-card')?.getAttribute('data-drop-id')).toBe('same-id');
    expect(page.container.querySelector('.best-card')?.getAttribute('data-tier')).toBe('ST');
    expect(page.container.querySelector('.best-card')?.textContent).toContain('Synthetic 1');
  } finally { connection.dispose(); live.dispose(); page.unmount(); }
});

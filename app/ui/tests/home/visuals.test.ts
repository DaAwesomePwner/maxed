import { itemHoverParams } from '../../src/lib/components/item-hover';
import { render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import Home from '../../src/routes/Home.svelte';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import ObjectLabel from '../../src/lib/home/ObjectLabel.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createHomeVisuals, homeVisualTypes, type HomeVisuals } from '../../src/lib/home/visuals';
import type { VisualPage } from '../../src/lib/bindings/VisualPage';
import { deferred, flush, homeSnapshot, homeSummary } from './fixtures';
import { flushHomeRender } from './shell-fixture';

const disposals: (() => void)[] = [];
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.useRealTimers(); vi.restoreAllMocks(); });
const sprite = { key: 'synthetic', sheet: 'characters.png', x: 0, y: 0, width: 8, height: 8 };
const catalog = (types: number[]): VisualPage => ({ client_fingerprint: 'a'.repeat(64),
  objects: types.map(object_type => ({ object_type, name: `Catalog name ${object_type}`, sprite, bag: null })) });

it('Dungeon resolves target, source items and loot in one batch, without querying on numeric updates', async () => {
  const snapshot = homeSnapshot();
  snapshot.fights[0].players[0].sources[0].estimated = true;
  const types = homeVisualTypes(snapshot, null);
  const pageData = catalog(types);
  pageData.objects.find(row => row.object_type === 300)!.name = 'Catalog enemy';
  pageData.objects.find(row => row.object_type === 200)!.name = 'Catalog sword';
  pageData.objects.find(row => row.object_type === snapshot.drops[0].bag_type)!.name = 'Catalog bag';
  const client = createClient(createFixtureTransport(snapshot, pageData));
  const get = vi.spyOn(client, 'getObjectVisuals');
  const page = render(Home, { snapshot, client }); await flushHomeRender();
  expect(within(screen.getByRole('tablist', { name: 'Encounters' })).getByText('Catalog enemy')).toBeTruthy();
  expect(screen.getAllByText('Catalog sword').length).toBeGreaterThan(0);
  expect(screen.getByRole('img', { name: 'Catalog bag' })).toBeTruthy();
  expect(screen.getByRole('img', { name: 'Catalog enemy' }).getAttribute('data-fallback')).toBe('false');
  expect(page.container.querySelector('img')?.getAttribute('src')).toBe(`http://maxed-sprite.localhost/${'a'.repeat(64)}/characters.png/0-0-8-8.png`);
  expect(get.mock.calls).toEqual([[types]]);
  expect(page.container.textContent).not.toMatch(/Target type|Item type|Bag type/);
  for (let sequence = 13; sequence < 20; sequence++) {
    const next = structuredClone(snapshot); next.sequence = String(sequence); next.fights[0].players[0].dps = sequence;
    await page.rerender({ snapshot: next }); await flushHomeRender();
  }
  expect(screen.getByRole('rowheader', { name: /\bYou$/ }).closest('tr')!
    .querySelector('td:nth-child(5)')?.textContent).toBe('19');
  expect(get).toHaveBeenCalledOnce();
});

it('best bag and recent-run previews use catalog names and sprites too', async () => {
  const summary = homeSummary();
  const visuals: HomeVisuals = { ...catalog(homeVisualTypes(null, summary)), phase: 'ready' };
  const page = render(NexusHome, { snapshot: homeSnapshot(), history: { phase: 'ready', summary, message: '', retryAvailable: true }, choice: 'session', retry: vi.fn(), visuals });
  await flushHomeRender();
  const best = screen.getByRole('heading', { name: 'Best bag' }).closest('section')!;
  const recent = page.container.querySelector('[data-row-id]')!;
  for (const container of [best, recent]) {
    expect(within(container as HTMLElement).getByRole('img', { name: `Catalog name ${summary.best_drop!.bag_type}` })).toBeTruthy();
    expect(container.textContent).toContain(`Catalog name ${summary.best_drop!.items[0]}`);
    expect(container.querySelector('[role="img"][data-fallback="false"]')).toBeTruthy();
    expect(container.textContent).not.toMatch(/Bag type|Item type/);
  }
});

it.each(['enemy', 'item', 'bag'] as const)('catalog-missing %s has a readable fallback, while a failed lookup is explicitly unavailable', async kind => {
  const page = render(ObjectLabel, { type: 65432, kind, visuals: { ...catalog([]), phase: 'ready' } });
  await flushHomeRender();
  expect(screen.getByText(`${kind[0].toUpperCase()}${kind.slice(1)}`)).toBeTruthy();
  expect(page.container.textContent).not.toContain('65432');
  if (kind === 'item') {
    const tile = page.container.querySelector('[data-item-hover]')!;
    expect(tile.getAttribute('data-item-hover')).toBe('65432');
    expect(page.container.querySelector('[title]')).toBeNull();
    expect(itemHoverParams(tile)).toEqual({ type: 65432, enchant: null, name: undefined, tier: undefined });
    const known = catalog([65432]);
    known.objects[0].item_tier = 'UT';
    await page.rerender({ type: 65432, kind, visuals: { ...known, phase: 'ready' } });
    expect(itemHoverParams(tile)).toEqual({ type: 65432, enchant: null, name: 'Catalog name 65432', tier: 'UT' });
  } else {
    expect(page.container.querySelector('[title]')?.getAttribute('title')).toBe('Catalog ID: 65432');
    expect(page.container.querySelector('[data-item-hover]')).toBeNull();
  }
  await page.rerender({ type: 65432, kind, visuals: { ...catalog([]), phase: 'unavailable' } }); await flushHomeRender();
  expect(screen.queryByText(`Unknown ${kind}`)).toBeNull();
  expect(page.container.textContent).toContain(`${kind[0].toUpperCase()}${kind.slice(1)}`);
});

it('batches large visible sets into serialized pages of at most 128, deduplicates and ignores row order', async () => {
  const getObjectVisuals = vi.fn(async (types: number[]) => catalog(types));
  const states: HomeVisuals[] = [];
  const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: () => () => {} }, next => states.push(next));
  disposals.push(loader.dispose);
  const types = Array.from({ length: 260 }, (_, i) => i);
  loader.update([...types, 0, 1]); await flush();
  expect(getObjectVisuals.mock.calls.map(([types]) => types.length)).toEqual([128, 128, 4]);
  expect(states.at(-1)).toMatchObject({ phase: 'ready', objects: catalog(types).objects });
  loader.update([...types].reverse()); await flush();
  expect(getObjectVisuals).toHaveBeenCalledTimes(3);
});

it('rejects retired catalog responses, retries after invalidation and releases its listener', async () => {
  const pending = deferred<VisualPage>(), release = vi.fn();
  let invalidate = () => {};
  const getObjectVisuals = vi.fn().mockReturnValueOnce(pending.promise).mockImplementation(async (types: number[]) => catalog(types));
  const states: HomeVisuals[] = [];
  const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: callback => { invalidate = callback; return release; } }, next => states.push(next));
  disposals.push(loader.dispose);
  loader.update([1]); await flush();
  expect(getObjectVisuals).toHaveBeenCalledOnce();
  loader.update([2]); invalidate(); pending.resolve(catalog([1])); await flush();
  expect(states.filter(state => state.phase === 'ready').map(state => state.objects.map(row => row.object_type))).toEqual([[2]]);
  expect(getObjectVisuals).toHaveBeenCalledTimes(2);
  loader.dispose(); expect(release).toHaveBeenCalledOnce();
  invalidate(); await flush(); expect(getObjectVisuals).toHaveBeenCalledTimes(2);
});

it('retains resolved visuals during incremental lookups and fetches only missing types', async () => {
  const pending = deferred<VisualPage>();
  const getObjectVisuals = vi.fn(async (types: number[]) => catalog(types));
  const states: HomeVisuals[] = [];
  const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: () => () => {} }, value => states.push(value));
  disposals.push(loader.dispose);
  loader.update([1]); await flush();
  getObjectVisuals.mockReturnValueOnce(pending.promise);
  loader.update([1, 2]); await flush();
  expect(states.at(-1)?.objects).toEqual(catalog([1]).objects);
  expect(getObjectVisuals.mock.calls).toEqual([[[1]], [[2]]]);
  pending.resolve(catalog([2])); await flush();
  expect(states.at(-1)).toMatchObject({ phase: 'ready', objects: catalog([1, 2]).objects });
  loader.update([2]); await flush();
  expect(states.at(-1)?.objects).toEqual(catalog([2]).objects);
  expect(getObjectVisuals).toHaveBeenCalledTimes(2);
});

it.each(['ready', 'failure'] as const)('clears invalidated catalog names, sprites and fingerprint through %s', async outcome => {
  const pending = deferred<VisualPage>();
  let invalidate = () => {};
  const getObjectVisuals = vi.fn(async (types: number[]) => catalog(types));
  const states: HomeVisuals[] = [];
  const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: callback => { invalidate = callback; return () => {}; } }, value => states.push(value));
  disposals.push(loader.dispose);
  loader.update([1]); await flush();
  getObjectVisuals.mockReturnValueOnce(pending.promise);
  invalidate(); await flush();
  expect(states.at(-1)).toEqual({ client_fingerprint: '', objects: [], phase: 'loading' });
  expect(getObjectVisuals.mock.calls).toEqual([[[1]], [[1]]]);
  const next = { ...catalog([1]), client_fingerprint: 'b'.repeat(64) };
  if (outcome === 'ready') pending.resolve(next);
  else pending.reject(new Error('Synthetic catalog unavailable'));
  await flush();
  expect(states.at(-1)).toEqual(outcome === 'ready' ? { ...next, phase: 'ready' }
    : { client_fingerprint: '', objects: [], phase: 'unavailable' });
  loader.update([1, 2]);
  expect(states.at(-1)).toEqual(outcome === 'ready' ? { ...next, phase: 'loading' }
    : { client_fingerprint: '', objects: [], phase: 'loading' });
  await flush();
});

it('trims cached types when they move away and fetches them again when they return', async () => {
  const getObjectVisuals = vi.fn(async (types: number[]) => catalog(types));
  const states: HomeVisuals[] = [];
  const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: () => () => {} }, value => states.push(value));
  disposals.push(loader.dispose);
  loader.update([1, 2]); await flush();
  loader.update([2]); await flush();
  expect(states.at(-1)?.objects).toEqual(catalog([2]).objects);
  loader.update([1, 2]); await flush();
  expect(getObjectVisuals.mock.calls).toEqual([[[1, 2]], [[1]]]);
  expect(states.at(-1)?.objects).toEqual(catalog([1, 2]).objects);
});

it('does not mix cached visuals with a new fingerprint discovered during an incremental read', async () => {
  const getObjectVisuals = vi.fn(async (types: number[]) => catalog(types));
  const states: HomeVisuals[] = [];
  const loader = createHomeVisuals({ getObjectVisuals, onVisualsChanged: () => () => {} }, value => states.push(value));
  disposals.push(loader.dispose);
  loader.update([1]); await flush();
  getObjectVisuals.mockImplementation(async types => ({ ...catalog(types), client_fingerprint: 'b'.repeat(64) }));
  loader.update([1, 2]); await flush();
  expect(getObjectVisuals.mock.calls).toEqual([[[1]], [[2]], [[1]]]);
  expect(states.at(-1)).toEqual({ ...catalog([1, 2]), client_fingerprint: 'b'.repeat(64), phase: 'ready' });
});

it('memoizes fight visual traversal by array identity and invalidates revised fights', () => {
  const fight = homeSnapshot().fights[0], players = fight.players;
  const read = vi.fn(() => players);
  Object.defineProperty(fight, 'players', { get: read });
  const window = { ...emptyHomeLiveWindow(), fights: [fight] };
  const first = homeVisualTypes(null, null, window);
  read.mockClear();
  expect(homeVisualTypes(null, null, { ...window, observedAt: '9000000' })).toEqual(first);
  expect(read).not.toHaveBeenCalled();
  const revised = { ...fight, target_type: 65000 };
  expect(homeVisualTypes(null, null, { ...window, fights: [revised] })).toContain(65000);
});

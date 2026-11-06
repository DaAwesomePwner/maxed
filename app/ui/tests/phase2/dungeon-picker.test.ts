import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import DungeonPicker from '../../src/lib/alerts/DungeonPicker.svelte';
import { createClient, type Commands } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { VisualPage } from '../../src/lib/bindings/VisualPage';
import { dungeonDef } from '../fixtures/dungeon';
import { deferred, flush, homeSnapshot, preferences } from './fixtures';

const sprite = { key: 'synthetic', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 };
beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

function clientWithNativeVisuals(fetch: (types: number[]) => Promise<VisualPage>) {
  const transport = createFixtureTransport(homeSnapshot());
  const invoke = transport.invoke;
  transport.invoke = async <K extends keyof Commands>(command: K, args: Commands[K]['args']): Promise<Commands[K]['result']> => {
    if (command === 'get_object_visuals') {
      const { objectTypes } = args as Commands['get_object_visuals']['args'];
      return await fetch(objectTypes) as Commands[K]['result'];
    }
    return invoke(command, args);
  };
  const client = createClient(transport);
  client.listCatalogDungeonOptions = vi.fn(async () => Array.from({ length: 166 }, (_, i) => dungeonDef({
    name: `Dungeon ${i + 1}`, aliases: [], portal_type: i + 1,
  })));
  return client;
}

it('checks the current fingerprint after confirming identical catalog rows', async () => {
  let fingerprint = 'a', catalogReads = 0;
  const restart = deferred<void>();
  const requests: number[] = [];
  const client = clientWithNativeVisuals(async types => {
    requests.push(types.length);
    if (requests.length === 6) await restart.promise;
    return { client_fingerprint: fingerprint.repeat(64),
      objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) };
  });
  const catalog = client.listCatalogDungeonOptions;
  client.listCatalogDungeonOptions = vi.fn(async () => {
    if (++catalogReads === 2) fingerprint = 'b';
    return catalog();
  });
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(requests).toEqual([128, 0, 38, 0, 0, 128]);
  restart.resolve(); await flush();
  expect(catalogReads).toBe(4);
  const images = [...view.container.querySelectorAll('.choice img.sprite')];
  expect(images).toHaveLength(166);
  expect(images.every(image => image.getAttribute('src')?.includes('b'.repeat(64)))).toBe(true);
  expect(requests).toEqual([128, 0, 38, 0, 0, 128, 0, 38, 0]);
});

it('bounds restarts when the fingerprint changes on every catalog confirmation', async () => {
  let generation = 0, catalogReads = 0;
  const client = clientWithNativeVisuals(async types => ({ client_fingerprint: generation.toString(16).padStart(64, '0'),
    objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) }));
  const catalog = client.listCatalogDungeonOptions;
  client.listCatalogDungeonOptions = vi.fn(async () => {
    if (++catalogReads > 6) throw new Error('Unexpected catalog read beyond the restart bound');
    if (catalogReads % 2 === 0) generation++;
    return catalog();
  });
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(catalogReads).toBe(6);
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(screen.getAllByRole('checkbox')).toHaveLength(166);
});

it('rechecks catalog rows when extraction finishes before the first real-cache visuals page', async () => {
  const restartedCatalog = deferred<void>();
  let replaced = false, catalogReads = 0;
  const client = clientWithNativeVisuals(async types => {
    replaced = true;
    return { client_fingerprint: 'b'.repeat(64),
      objects: types.map(object_type => ({ object_type, sprite: { ...sprite, x: object_type }, name: null, bag: null })) };
  });
  const initial = await client.listCatalogDungeonOptions();
  const replacement = initial.map((row, index) => index ? row : { ...row, name: 'New dungeon', aliases: ['Fresh alias'], portal_type: 500 });
  client.listCatalogDungeonOptions = vi.fn(async () => {
    if (++catalogReads === 3) await restartedCatalog.promise;
    return structuredClone(replaced ? replacement : initial);
  });
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(catalogReads).toBe(3);
  restartedCatalog.resolve(); await flush();
  expect(catalogReads).toBe(4);
  expect(screen.queryByRole('checkbox', { name: 'Dungeon 1' })).toBeNull();
  expect(screen.getByRole('checkbox', { name: 'New dungeon' })).toBeTruthy();
  const images = [...view.container.querySelectorAll('.choice img.sprite')];
  expect(images).toHaveLength(166);
  expect(images.every(image => image.getAttribute('src')?.includes('b'.repeat(64)))).toBe(true);
  await fireEvent.input(screen.getByRole('searchbox', { name: 'Search dungeons' }), { target: { value: 'Fresh alias' } });
  expect(screen.getAllByRole('checkbox')).toHaveLength(1);
  expect(view.container.querySelector('.choice img.sprite')?.getAttribute('src')).toContain('/500-0-8-8.png');
});

it('uses the same two-restart bound when catalog rows keep changing under stable visuals', async () => {
  const client = clientWithNativeVisuals(async types => ({ client_fingerprint: 'b'.repeat(64),
    objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) }));
  const initial = await client.listCatalogDungeonOptions();
  let catalogReads = 0;
  client.listCatalogDungeonOptions = vi.fn(async () => {
    if (++catalogReads > 6) throw new Error('Unexpected catalog read beyond the restart bound');
    return initial.map((row, index) => index ? row : { ...row, aliases: [`Revision ${catalogReads}`] });
  });
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(catalogReads).toBe(6);
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(screen.getAllByRole('checkbox')).toHaveLength(166);
  await fireEvent.input(screen.getByRole('searchbox', { name: 'Search dungeons' }), { target: { value: 'Dungeon 166' } });
  expect(screen.getAllByRole('checkbox')).toHaveLength(1);
});

it('restarts a real-cache load across fingerprints without publishing the mixed map', async () => {
  const restartedChunk = deferred<void>();
  const requests: number[] = [];
  const client = clientWithNativeVisuals(async types => {
    requests.push(types.length);
    const fingerprint = requests.length === 1 ? 'a' : 'b';
    if (requests.length === 5) await restartedChunk.promise;
    return { client_fingerprint: fingerprint.repeat(64),
      objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) };
  });
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: ['Dungeon 1'] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(client.listCatalogDungeonOptions).toHaveBeenCalledTimes(2);
  expect(requests).toEqual([128, 0, 38, 0, 128]);
  restartedChunk.resolve(); await flush();
  const images = [...view.container.querySelectorAll('img.sprite')];
  expect(images).toHaveLength(167);
  expect(images.every(image => image.getAttribute('src')?.includes('b'.repeat(64)))).toBe(true);
  expect(requests).toEqual([128, 0, 38, 0, 128, 0, 0]);
});

it('stops after two fingerprint restarts and leaves the catalog searchable without sprites', async () => {
  const requests: number[] = [];
  const client = clientWithNativeVisuals(async types => {
    requests.push(types.length);
    if (requests.length > 11) throw new Error('Unexpected request beyond the restart bound');
    return { client_fingerprint: requests.length.toString(16).padStart(64, '0'),
      objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) };
  });
  const onEdit = vi.fn();
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit });
  await flush();
  expect(client.listCatalogDungeonOptions).toHaveBeenCalledTimes(3);
  expect(requests).toEqual([128, 0, 38, 0, 128, 0, 38, 0, 128, 0, 38]);
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(screen.getAllByRole('checkbox')).toHaveLength(166);
  await fireEvent.input(screen.getByRole('searchbox', { name: 'Search dungeons' }), { target: { value: 'Dungeon 166' } });
  expect(screen.getAllByRole('checkbox')).toHaveLength(1);
  await fireEvent.click(screen.getByRole('button', { name: 'Select all' })); await flush();
  expect(onEdit).toHaveBeenLastCalledWith({ key_pop_dungeons: ['Dungeon 166'] });
  expect(client.listCatalogDungeonOptions).toHaveBeenCalledTimes(3);
  expect(requests).toHaveLength(11);
});

it('keeps saved selections usable without any portal lookups after a catalog failure', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  client.listCatalogDungeonOptions = vi.fn(async () => { throw new Error('Assets unavailable'); });
  client.getDungeonPortalType = vi.fn(async () => 1);
  client.getObjectVisuals = vi.fn(async () => ({ client_fingerprint: 'a'.repeat(64), objects: [] }));
  const names = Array.from({ length: 256 }, (_, i) => `Saved dungeon ${i}`);
  const onEdit = vi.fn();
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: names }), disabled: false, onEdit });
  await flush();
  expect(client.getDungeonPortalType).not.toHaveBeenCalled();
  expect(client.getObjectVisuals).not.toHaveBeenCalled();
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(screen.getByText('Dungeon catalog unavailable. Check assets in Settings.')).toBeTruthy();
  await fireEvent.input(screen.getByRole('searchbox', { name: 'Search dungeons' }), { target: { value: 'Saved dungeon 255' } });
  expect(screen.getAllByRole('checkbox')).toHaveLength(1);
  await fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
  expect(onEdit).toHaveBeenLastCalledWith({ key_pop_dungeons: names.slice(0, -1) });
});

it('uses the real cache to request and confirm 166 portal types with native sizes [128, 0, 38, 0]', async () => {
  const types = Array.from({ length: 166 }, (_, i) => i + 1);
  const transport = createFixtureTransport(homeSnapshot(), { client_fingerprint: 'a'.repeat(64),
    objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) });
  const invoke = vi.spyOn(transport, 'invoke');
  const client = createClient(transport);
  client.listCatalogDungeonOptions = vi.fn(async () => types.map(portal_type => dungeonDef({
    name: `Dungeon ${portal_type}`, aliases: [], portal_type,
  })));
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  const requests = () => invoke.mock.calls.filter(([command]) => command === 'get_object_visuals')
    .map(([, args]) => (args as { objectTypes: number[] }).objectTypes);
  expect(requests()).toEqual([types.slice(0, 128), [], types.slice(128), []]);
  expect(requests().map(types => types.length)).toEqual([128, 0, 38, 0]);
  expect(client.listCatalogDungeonOptions).toHaveBeenCalledTimes(2);
  expect(view.container.querySelectorAll('.choice img.sprite')).toHaveLength(166);
  const search = screen.getByRole('searchbox', { name: 'Search dungeons' });
  await fireEvent.input(search, { target: { value: 'Dungeon 10' } });
  await fireEvent.input(search, { target: { value: '' } }); await flush();
  expect(requests().map(types => types.length)).toEqual([128, 0, 38, 0]);
  expect(invoke.mock.calls.some(([command]) => command === 'get_dungeon_portal_type')).toBe(false);
});

it('shows synthetic preview catalog dungeons as searchable options without portal artwork', async () => {
  const transport = createFixtureTransport(homeSnapshot(), undefined, undefined, undefined, undefined, undefined, undefined,
    { catalogDungeons: ['Preview dungeon'] });
  const client = createClient(transport);
  expect(await client.listCatalogDungeonOptions()).toEqual([
    { name: 'Preview dungeon', aliases: [], kind: 'dungeon', completion_index: null, portal_type: null,
      difficulty: null, exalt: false, exalt_gateway: false, category: null },
  ]);
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(screen.getByRole('checkbox', { name: 'Preview dungeon' })).toBeTruthy();
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
});

it.each([166, 257])('resolves %i unique portals in sequential 128-type batches and publishes all sprites together', async count => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const types = Array.from({ length: count }, (_, i) => i + 1);
  client.listCatalogDungeonOptions = vi.fn(async () => types.map(portal_type => dungeonDef({
    name: `Dungeon ${portal_type}`, aliases: [], portal_type,
  })));
  client.getDungeonPortalType = vi.fn(async () => 1);
  const pending: ReturnType<typeof deferred<VisualPage>>[] = [];
  const visuals = vi.fn((_types: number[]) => {
    const request = deferred<VisualPage>(); pending.push(request); return request.promise;
  });
  client.getObjectVisuals = visuals;
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  const batches = Math.ceil(count / 128);
  for (let batch = 0; batch < batches; batch++) {
    expect(visuals).toHaveBeenCalledTimes(batch + 1);
    expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
    const chunk = types.slice(batch * 128, (batch + 1) * 128);
    expect(visuals).toHaveBeenNthCalledWith(batch + 1, chunk);
    pending[batch].resolve({ client_fingerprint: 'a'.repeat(64),
      objects: chunk.map(object_type => ({ object_type, sprite, name: null, bag: null })) });
    await flush();
  }
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  expect(visuals).toHaveBeenCalledTimes(batches + 1);
  expect(visuals).toHaveBeenLastCalledWith([]);
  pending[batches].resolve({ client_fingerprint: 'a'.repeat(64), objects: [] }); await flush();
  expect(view.container.querySelectorAll('.choice img.sprite')).toHaveLength(count);
  const search = screen.getByRole('searchbox', { name: 'Search dungeons' });
  await fireEvent.input(search, { target: { value: 'Dungeon 10' } });
  await fireEvent.input(search, { target: { value: '' } }); await flush();
  expect(view.container.querySelectorAll('.choice img.sprite')).toHaveLength(count);
  expect(visuals).toHaveBeenCalledTimes(batches + 1);
  expect(client.getDungeonPortalType).not.toHaveBeenCalled();
});

it.each([0, 1])('discards an obsolete load during chunk %i without publishing or continuing it', async staleChunk => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const types = Array.from({ length: 166 }, (_, i) => i + 1);
  client.listCatalogDungeonOptions = vi.fn(async () => types.map(portal_type => dungeonDef({
    name: `Dungeon ${portal_type}`, aliases: [], portal_type,
  })));
  client.getDungeonPortalType = vi.fn(async () => 1);
  let changed = () => {};
  client.onVisualsChanged = vi.fn(listener => { changed = listener; return () => {}; });
  const obsolete = deferred<VisualPage>();
  let refreshed = false;
  const visuals = vi.fn(async (chunk: number[]): Promise<VisualPage> => {
    if (!refreshed && visuals.mock.calls.length === staleChunk + 1) return obsolete.promise;
    return { client_fingerprint: (refreshed ? 'b' : 'a').repeat(64),
      objects: chunk.map(object_type => ({ object_type, sprite, name: null, bag: null })) };
  });
  client.getObjectVisuals = visuals;
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: [] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(visuals).toHaveBeenCalledTimes(staleChunk + 1);
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(0);
  refreshed = true; changed(); await flush();
  expect(visuals).toHaveBeenCalledTimes(staleChunk + 4);
  expect(view.container.querySelectorAll('.choice img.sprite')).toHaveLength(types.length);
  obsolete.resolve({ client_fingerprint: 'a'.repeat(64),
    objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) });
  await flush();
  expect(visuals).toHaveBeenCalledTimes(staleChunk + 4);
  const images = [...view.container.querySelectorAll('.choice img.sprite')];
  expect(images).toHaveLength(types.length);
  expect(images.every(image => image.getAttribute('src')?.includes('b'.repeat(64)))).toBe(true);
  expect(client.getDungeonPortalType).not.toHaveBeenCalled();
});

it('loads one unique-type visuals batch for 166 catalog rows and reuses sprites across search and chips', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const rows = Array.from({ length: 166 }, (_, i) => dungeonDef({ name: `Dungeon ${i}`, aliases: [],
    portal_type: i % 2 + 1 }));
  client.listCatalogDungeonOptions = vi.fn(async () => rows);
  client.getDungeonPortalType = vi.fn(async () => 1);
  client.getObjectVisuals = vi.fn(async (types: number[]) => ({ client_fingerprint: 'a'.repeat(64),
    objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) }));
  let changed = () => {};
  client.onVisualsChanged = vi.fn(listener => { changed = listener; return () => {}; });
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: ['Dungeon 0'] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(client.getDungeonPortalType).not.toHaveBeenCalled();
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(2);
  expect(client.getObjectVisuals).toHaveBeenNthCalledWith(1, [1, 2]);
  expect(client.getObjectVisuals).toHaveBeenNthCalledWith(2, []);
  expect(client.onVisualsChanged).toHaveBeenCalledOnce();
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(167);
  const search = screen.getByRole('searchbox', { name: 'Search dungeons' });
  await fireEvent.input(search, { target: { value: 'Dungeon 10' } });
  await fireEvent.input(search, { target: { value: '' } }); await flush();
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(2);
  expect(client.getDungeonPortalType).not.toHaveBeenCalled();
  changed(); await flush();
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(4);
  expect(client.getDungeonPortalType).not.toHaveBeenCalled();
});

it('uses one per-name fallback only for a selected name absent from the catalog', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  client.listCatalogDungeonOptions = vi.fn(async () => [
    dungeonDef({ name: 'Known', aliases: [], portal_type: 1 }),
    dungeonDef({ name: 'Known without art', aliases: [], portal_type: null }),
  ]);
  client.getDungeonPortalType = vi.fn(async () => 2);
  client.getObjectVisuals = vi.fn(async (types: number[]) => ({ client_fingerprint: 'a'.repeat(64),
    objects: types.map(object_type => ({ object_type, sprite, name: null, bag: null })) }));
  const view = render(DungeonPicker, { client, preferences: preferences({ key_pop_dungeons: ['Missing', 'Known without art'] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(client.getDungeonPortalType).toHaveBeenCalledExactlyOnceWith('Missing');
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(3);
  expect(client.getObjectVisuals).toHaveBeenCalledWith([1]);
  expect(client.getObjectVisuals).toHaveBeenCalledWith([2]);
  expect(client.getObjectVisuals).toHaveBeenCalledWith([]);
  expect(view.container.querySelectorAll('img.sprite')).toHaveLength(3);
  const search = screen.getByRole('searchbox', { name: 'Search dungeons' });
  await fireEvent.input(search, { target: { value: 'Known' } });
  await fireEvent.input(search, { target: { value: '' } }); await flush();
  expect(client.getDungeonPortalType).toHaveBeenCalledTimes(1);
});

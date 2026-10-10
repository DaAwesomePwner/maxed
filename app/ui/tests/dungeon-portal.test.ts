import { expect, it, vi } from 'vitest';
import { createDungeonPortalSprite, type DungeonPortalSprite } from '../src/lib/bridge/dungeon-portal';
import type { Client } from '../src/lib/bridge/client';

const sprite = { key: 'synthetic', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 };
const page = (type: number) => ({ client_fingerprint: 'a'.repeat(64), objects: [{ object_type: type, sprite, name: `Portal ${type}`, bag: null }] });
const flush = async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); };

it('discards retired dungeon and catalog responses, clears stale art, and releases its listener', async () => {
  let changed = () => {};
  let finishOld!: (type: number | null) => void;
  let finishVisual!: (value: ReturnType<typeof page>) => void;
  const release = vi.fn();
  const client = {
    getDungeonPortalType: vi.fn<Client['getDungeonPortalType']>().mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve; })).mockResolvedValue(2),
    getObjectVisuals: vi.fn<Client['getObjectVisuals']>().mockResolvedValue(page(2)),
    onVisualsChanged: (listener: () => void) => { changed = listener; return release; },
  };
  let current: DungeonPortalSprite | null = null;
  const lookup = createDungeonPortalSprite(client, value => { current = value; });
  lookup.update('Old dungeon'); lookup.update('New dungeon'); await flush();
  expect(current).toMatchObject({ name: 'Portal 2', sprite });
  finishOld(1); await flush();
  expect(current).toMatchObject({ name: 'Portal 2' });
  client.getObjectVisuals.mockImplementationOnce(() => new Promise(resolve => { finishVisual = resolve; }));
  changed(); expect(current).toBeNull(); await flush();
  lookup.update(null); finishVisual(page(2)); await flush();
  expect(current).toBeNull();
  lookup.update('Disposed dungeon'); lookup.dispose(); await flush();
  expect(current).toBeNull(); expect(release).toHaveBeenCalledOnce();
});

it.each([null, -1, 65536, 1.5])('does not request art for an absent or invalid portal type %s', async type => {
  const client = { getDungeonPortalType: async () => type, getObjectVisuals: vi.fn(), onVisualsChanged: () => () => {} };
  const receive = vi.fn();
  const lookup = createDungeonPortalSprite(client, receive);
  lookup.update('Unknown'); await flush();
  expect(receive).toHaveBeenLastCalledWith(null);
  expect(client.getObjectVisuals).not.toHaveBeenCalled(); lookup.dispose();
});

it('does not publish an old sprite after a catalog refresh resolves a new one', async () => {
  let changed = () => {};
  let finishOld!: (value: ReturnType<typeof page>) => void;
  const client = {
    getDungeonPortalType: vi.fn<Client['getDungeonPortalType']>().mockResolvedValueOnce(1).mockResolvedValue(2),
    getObjectVisuals: vi.fn<Client['getObjectVisuals']>().mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve; })).mockResolvedValue(page(2)),
    onVisualsChanged: (listener: () => void) => { changed = listener; return () => {}; },
  };
  const receive = vi.fn();
  const lookup = createDungeonPortalSprite(client, receive);
  lookup.update('Synthetic dungeon'); await flush();
  changed(); await flush();
  expect(receive).toHaveBeenLastCalledWith(expect.objectContaining({ name: 'Portal 2' }));
  finishOld(page(1)); await flush();
  expect(receive).toHaveBeenLastCalledWith(expect.objectContaining({ name: 'Portal 2' }));
  lookup.dispose();
});

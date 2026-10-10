import { expect, it, vi } from 'vitest';
import { createClient, type Transport } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot } from '../home/fixtures';

it('maps first and subsequent pages without swallowing busy, superseded or errors', async () => {
  const invoke = vi.fn().mockResolvedValueOnce({ status: 'busy' })
    .mockResolvedValueOnce({ status: 'superseded' }).mockRejectedValueOnce(new Error('storage failed'));
  const client = createClient({ invoke, listen: vi.fn() } as Transport);
  const scope = { session: { id: 's' } };
  expect(await client.queryScopeLoot(scope, 32)).toEqual({ status: 'busy' });
  expect(invoke).toHaveBeenLastCalledWith('get_scope_loot', { scope, cursor: null, limit: 32 });
  expect(await client.queryScopeLootPage(scope, 'cursor', 32)).toEqual({ status: 'superseded' });
  expect(invoke).toHaveBeenLastCalledWith('get_scope_loot', { scope, cursor: 'cursor', limit: 32 });
  await expect(client.queryScopeLoot(scope, 32)).rejects.toThrow('storage failed');
});
it('fixture history defaults safely and accepts explicit loot pages', async () => {
  const scope = { session: { id: 's' } };
  const page = { items: [], next: null, total_bags: 0 };
  const query = vi.fn().mockResolvedValue({ status: 'ready', value: page });
  const client = createClient(createFixtureTransport(homeSnapshot(), undefined, undefined, undefined, undefined, query));
  expect(await client.queryScopeLoot(scope, 32)).toEqual({ status: 'ready', value: page });
  expect(query).toHaveBeenCalledWith(scope, null, 32);
  const absent = createClient(createFixtureTransport(homeSnapshot()));
  expect(await absent.queryScopeLoot(scope, 32)).toEqual({ status: 'ready', value: page });
  expect(await absent.queryDungeonBests()).toEqual({ status: 'ready', value: [] });
  const strict = createClient(createFixtureTransport(homeSnapshot(), undefined, undefined, undefined, undefined, undefined, undefined, { requireHistoryFixtures: true }));
  await expect(strict.queryScopeLoot(scope, 32)).rejects.toThrow('Scope loot was not supplied');
  await expect(strict.queryDungeonBests()).rejects.toThrow('Dungeon bests were not supplied');
});

it('maps all-time bests and preserves ready, busy, superseded and errors', async () => {
  const bests = [{ dungeon: 'The Shatters', best_clear_micros: 719000000 }];
  const invoke = vi.fn().mockResolvedValueOnce({ status: 'ready', value: bests })
    .mockResolvedValueOnce({ status: 'busy' }).mockResolvedValueOnce({ status: 'superseded' })
    .mockRejectedValueOnce(new Error('storage failed'));
  const client = createClient({ invoke, listen: vi.fn() } as Transport);
  expect(await client.queryDungeonBests()).toEqual({ status: 'ready', value: bests });
  expect(invoke).toHaveBeenLastCalledWith('get_dungeon_bests', undefined);
  expect(await client.queryDungeonBests()).toEqual({ status: 'busy' });
  expect(await client.queryDungeonBests()).toEqual({ status: 'superseded' });
  await expect(client.queryDungeonBests()).rejects.toThrow('storage failed');
  const fixture = createClient(createFixtureTransport(homeSnapshot(), undefined, undefined, undefined, undefined,
    undefined, async () => ({ status: 'ready', value: bests })));
  expect(await fixture.queryDungeonBests()).toEqual({ status: 'ready', value: bests });
});

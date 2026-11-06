import { afterEach, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { dungeonCatalog, dungeonMetadata, loadDungeonCatalog } from '../../src/lib/alerts/dungeon-catalog';
import { allowsAlert } from '../../src/lib/alerts/preferences';
import { dungeonDef } from '../fixtures/dungeon';
import { alert, homeSnapshot, preferences } from './fixtures';

afterEach(() => dungeonCatalog.set(undefined));
it('retries a failed catalog read without requiring an invalidation', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const rows = [dungeonDef({ name: 'Synthetic Vault', difficulty: 6 })];
  client.listCatalogDungeonOptions = vi.fn().mockRejectedValueOnce(new Error('Unavailable')).mockResolvedValue(rows);
  await expect(loadDungeonCatalog(client)).rejects.toThrow('Unavailable');
  expect(get(dungeonCatalog)).toBeUndefined();
  await expect(loadDungeonCatalog(client)).resolves.toEqual(rows);
  expect(get(dungeonCatalog)).toEqual(rows);
  expect(client.listCatalogDungeonOptions).toHaveBeenCalledTimes(2);
});
it('matches metadata with the Rust ASCII case-insensitive semantics', () => {
  const row = dungeonDef({ name: 'Synthetic Vault', difficulty: 6 });
  dungeonCatalog.set([row]);
  expect(dungeonMetadata('sYnThEtIc vAuLt')).toBe(row);
  expect(dungeonMetadata('Unknown')).toBeNull();
  expect(allowsAlert(preferences({ key_pop_min_difficulty: 7 }), alert({ kind: 'key_popped', details: { type: 'key_popped', dungeon: 'SYNTHETIC VAULT' } }))).toBe(false);
});

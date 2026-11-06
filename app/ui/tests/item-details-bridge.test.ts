import { describe, expect, it, vi } from 'vitest';
import type { ItemDetailPage } from '../src/lib/bindings/ItemDetailPage';
import { createItemDetailsCache } from '../src/lib/bridge/item-details';

function page(types: number[], ids: number[], fingerprint = 'a'.repeat(64)): ItemDetailPage {
  return {
    client_fingerprint: fingerprint,
    items: types.map(object_type => ({ object_type, name: 'Synthetic Sword', tier: 'UT', slot_type: 1, slot_label: 'Sword', soulbound: null })),
    enchants: ids.map(id => ({ id, name: `Synthetic Enchant ${id}`, description: 'Synthetic effect' })),
  };
}

describe('item details cache', () => {
  it('fetches once then serves from cache', async () => {
    const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
    const cache = createItemDetailsCache(fetch);
    const first = await cache.get(10, [1, 2]);
    expect(await cache.get(10, [1, 2])).toEqual(first);
    expect(fetch).toHaveBeenCalledExactlyOnceWith([10], [1, 2]);
  });

  it('fetches only what is missing', async () => {
    const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
    const cache = createItemDetailsCache(fetch);
    await cache.get(10, [1]);
    await cache.get(10, [1, 2]);
    expect(fetch.mock.calls).toEqual([[[10], [1]], [[], [2]]]);
  });

  it('keeps enchant order and duplicates as requested', async () => {
    const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
    const result = await createItemDetailsCache(fetch).get(10, [2, 1, 2]);
    expect(result.enchants.map(row => row.id)).toEqual([2, 1, 2]);
    expect(fetch).toHaveBeenCalledWith([10], [2, 1]);
  });

  it('fingerprint change drops rows', async () => {
    let fingerprint = 'a'.repeat(64);
    const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids, fingerprint));
    const cache = createItemDetailsCache(fetch);
    await cache.get(10, [1]);
    fingerprint = 'b'.repeat(64);
    await cache.get(11, [2]);
    await cache.get(10, [1]);
    expect(fetch.mock.calls).toEqual([[[10], [1]], [[11], [2]], [[10], [1]]]);
  });

  it('clear drops rows', async () => {
    const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
    const cache = createItemDetailsCache(fetch);
    await cache.get(10, [1]);
    cache.clear();
    await cache.get(10, [1]);
    expect(fetch).toHaveBeenCalledTimes(2);
    let release!: (value: ItemDetailPage) => void;
    fetch.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
    const pending = cache.get(11, []);
    const rejected = expect(pending).rejects.toThrow('Asset selection changed. Retry the lookup.');
    await vi.waitFor(() => expect(release).toBeDefined());
    cache.clear();
    release(page([11], []));
    await rejected;
    await cache.get(11, []);
    expect(fetch).toHaveBeenCalledTimes(4);
  });

  it('failure is not cached', async () => {
    const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
    fetch.mockRejectedValueOnce(new Error('Synthetic failure'));
    const cache = createItemDetailsCache(fetch);
    await expect(cache.get(10, [1])).rejects.toThrow('Synthetic failure');
    await expect(cache.get(10, [1])).resolves.toMatchObject({ item: { object_type: 10 } });
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it('rejects out-of-range input', async () => {
    const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
    const cache = createItemDetailsCache(fetch);
    for (const type of [-1, 65536, 1.5, NaN]) await expect(cache.get(type, [])).rejects.toThrow();
    for (const ids of [[1.5], [NaN], [-2147483649], [2147483648], Array.from({ length: 129 }, (_, i) => i)]) {
      await expect(cache.get(10, ids)).rejects.toThrow();
    }
    expect(fetch).not.toHaveBeenCalled();
  });
});

it('refetches the full lookup when a missing-only response changes fingerprint', async () => {
  let fingerprint = 'a'.repeat(64);
  const fetch = vi.fn(async (types: number[], ids: number[]) => {
    const result = page(types, ids, fingerprint);
    result.items.forEach(row => { row.name = fingerprint; });
    result.enchants.forEach(row => { row.name = fingerprint; });
    return result;
  });
  const cache = createItemDetailsCache(fetch);
  await cache.get(10, [1]);
  fingerprint = 'b'.repeat(64);
  const result = await cache.get(10, [1, 2]);
  expect(fetch.mock.calls).toEqual([[[10], [1]], [[], [2]], [[10], [1, 2]]]);
  expect(result.item.name).toBe(fingerprint);
  expect(result.enchants.map(row => row.name)).toEqual([fingerprint, fingerprint]);
});

it('evicts the least recently used items and enchants at their caps', async () => {
  const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
  const cache = createItemDetailsCache(fetch);
  for (let type = 0; type < 512; type++) await cache.get(type, []);
  await cache.get(0, []);
  await cache.get(512, []);
  fetch.mockClear();
  await cache.get(0, []);
  expect(fetch).not.toHaveBeenCalled();
  await cache.get(1, []);
  expect(fetch).toHaveBeenCalledExactlyOnceWith([1], []);
  for (let start = 0; start < 2048; start += 128) {
    await cache.get(0, Array.from({ length: 128 }, (_, i) => start + i));
  }
  await cache.get(0, [0]);
  await cache.get(0, [2048]);
  fetch.mockClear();
  await cache.get(0, [0]);
  expect(fetch).not.toHaveBeenCalled();
  await cache.get(0, [1]);
  expect(fetch).toHaveBeenCalledExactlyOnceWith([], [1]);
});

it('serializes concurrent lookups and isolates returned rows', async () => {
  const fetch = vi.fn(async (types: number[], ids: number[]) => page(types, ids));
  const cache = createItemDetailsCache(fetch);
  const [first, second] = await Promise.all([cache.get(10, [1]), cache.get(10, [1])]);
  expect(fetch).toHaveBeenCalledTimes(1);
  first.item.name = 'Changed';
  first.enchants[0].name = 'Changed';
  expect(await cache.get(10, [1])).toEqual(second);
});

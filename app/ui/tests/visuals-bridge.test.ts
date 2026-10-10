import { expect, test, vi } from 'vitest';
import type { VisualPage } from '../src/lib/bindings/VisualPage';
import { createVisualCache, spriteUrl, SPRITE_ORIGIN } from '../src/lib/bridge/visuals';

const page = (types: number[], fingerprint = 'a'.repeat(64)): VisualPage => ({
  client_fingerprint: fingerprint,
  objects: types.map(object_type => ({ object_type, name: `Object ${object_type}`, sprite: null, bag: null })),
});

test('cache reuses rows after checking fingerprint and does not expose mutable cache entries', async () => {
  const fetch = vi.fn(async (types: number[]) => page(types));
  const cache = createVisualCache(fetch);
  const first = await cache.get([1, 1, 2]);
  first.objects[0].name = 'Changed by caller';
  const next = await cache.get([1]);
  expect(fetch.mock.calls.map(([types]) => types)).toEqual([[1, 2], []]);
  expect(next.objects[0].name).toBe('Object 1');
});

test('cache retains at most 256 rows with least recently used eviction', async () => {
  const fetch = vi.fn(async (types: number[]) => page(types));
  const cache = createVisualCache(fetch);
  await cache.get(Array.from({ length: 128 }, (_, i) => i));
  await cache.get(Array.from({ length: 128 }, (_, i) => i + 128));
  await cache.get([0]); // keep zero; one becomes oldest
  await cache.get([256]);
  fetch.mockClear();
  await cache.get([0]); expect(fetch.mock.calls).toEqual([[[]]]);
  fetch.mockClear();
  await cache.get([1]); expect(fetch.mock.calls).toEqual([[[]], [[1]]]);
});

test('fingerprint changes clear cached rows even on an entirely cached request', async () => {
  let fingerprint = 'a'.repeat(64);
  const fetch = vi.fn(async (types: number[]) => page(types, fingerprint));
  const cache = createVisualCache(fetch);
  await cache.get([1]); fingerprint = 'b'.repeat(64);
  const result = await cache.get([1]);
  expect(result.client_fingerprint).toBe(fingerprint);
  expect(fetch.mock.calls.map(([types]) => types)).toEqual([[1], [], [1]]);
});

test('a fingerprint change between metadata and rows cannot combine generations', async () => {
  let generation = 0;
  const cache = createVisualCache(async types => {
    if (types.includes(2)) generation++;
    return page(types, String(generation).repeat(64));
  });
  await cache.get([1]);
  const result = await cache.get([1, 2]);
  expect(result.client_fingerprint).toBe('1'.repeat(64));
  expect(result.objects.map(row => row.object_type)).toEqual([1, 2]);
});

test('invalidation rejects pending old results and native failures do not return stale data', async () => {
  let resolve!: (value: VisualPage) => void;
  const fetch = vi.fn<(types: number[]) => Promise<VisualPage>>()
    .mockImplementationOnce(() => new Promise(done => { resolve = done; }))
    .mockRejectedValueOnce(new Error('unavailable'))
    .mockImplementation(async types => page(types));
  const cache = createVisualCache(fetch);
  const pending = cache.get([1]);
  await Promise.resolve();
  cache.clear(); resolve(page([1]));
  await expect(pending).rejects.toThrow('Asset selection changed');
  await expect(cache.get([1])).rejects.toThrow('unavailable');
  expect((await cache.get([1])).objects).toHaveLength(1);
});

test('bounds and sprite URLs reject invalid types, traversal, generation and content types', async () => {
  const fetch = vi.fn(async (types: number[]) => page(types));
  const cache = createVisualCache(fetch);
  for (const types of [[-1], [65536], [1.5], [NaN], Array.from({ length: 129 }, (_, i) => i)]) await expect(cache.get(types)).rejects.toThrow();
  expect(fetch).not.toHaveBeenCalled();
  const sprite = { key: 'test', sheet: 'characters.png', x: 0, y: 0, width: 1, height: 1 };
  const fingerprint = 'a'.repeat(64);
  expect(spriteUrl(fingerprint, sprite)).toBe(`${SPRITE_ORIGIN}/${fingerprint}/characters.png/0-0-1-1.png`);
  for (const sheet of ['../characters.png', '%2e%2e/characters.png', 'C:/secret.png', 'https://remote/image.png', 'characters.svg']) expect(spriteUrl(fingerprint, { ...sprite, sheet })).toBeNull();
  expect(spriteUrl('unknown', sprite)).toBeNull();
});

test('tile URLs reject invalid and oversized rectangles and change for different tiles and fingerprints', () => {
  const sprite = { key: 'test', sheet: 'characters.png', x: 3, y: 5, width: 8, height: 12 };
  const fp = 'a'.repeat(64);
  for (const changes of [{ x: -1 }, { x: 1.5 }, { y: NaN }, { width: 0 }, { height: 257 }, { x: 16384 }, { y: Number.MAX_SAFE_INTEGER }])
    expect(spriteUrl(fp, { ...sprite, ...changes })).toBeNull();
  expect(spriteUrl(fp, sprite)).not.toBe(spriteUrl(fp, { ...sprite, x: 4 }));
  expect(spriteUrl(fp, sprite)).not.toBe(spriteUrl('b'.repeat(64), sprite));
});

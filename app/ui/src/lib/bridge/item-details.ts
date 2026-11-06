import type { ItemDetail } from '../bindings/ItemDetail';
import type { EnchantInfo } from '../bindings/EnchantInfo';
import type { ItemDetailPage } from '../bindings/ItemDetailPage';

export interface ItemDetailLookup { item: ItemDetail; enchants: EnchantInfo[] }

/** Hover lookups serialize so older responses cannot restore a retired generation. */
export function createItemDetailsCache(fetch: (objectTypes: number[], enchantIds: number[]) => Promise<ItemDetailPage>) {
  const items = new Map<number, ItemDetail>();
  const enchants = new Map<number, EnchantInfo>();
  let fingerprint: string | undefined;
  let epoch = 0;
  let tail: Promise<unknown> = Promise.resolve();
  function store<T>(rows: Map<number, T>, id: number, row: T, cap: number) {
    rows.delete(id);
    rows.set(id, structuredClone(row));
    if (rows.size > cap) rows.delete(rows.keys().next().value!);
  }
  function accept(page: ItemDetailPage) {
    if (page.client_fingerprint !== fingerprint) {
      items.clear(); enchants.clear(); fingerprint = page.client_fingerprint;
    }
    for (const row of page.items) store(items, row.object_type, row, 512);
    for (const row of page.enchants) store(enchants, row.id, row, 2048);
  }
  function read<T>(rows: Map<number, T>, id: number): T {
    const row = rows.get(id);
    if (!row) throw new Error('Item details are unavailable. Retry the lookup.');
    rows.delete(id); rows.set(id, row);
    return structuredClone(row);
  }
  return {
    clear() { epoch++; items.clear(); enchants.clear(); fingerprint = undefined; },
    get(type: number, enchantIds: number[]): Promise<ItemDetailLookup> {
      const requested = [...enchantIds];
      const unique = [...new Set(requested)];
      if (!Number.isInteger(type) || type < 0 || type > 65535) return Promise.reject(new Error('Request an object type in the u16 range'));
      if (unique.length > 128 || unique.some(id => !Number.isInteger(id) || id < -2147483648 || id > 2147483647)) return Promise.reject(new Error('Request at most 128 unique enchant IDs in the i32 range'));
      const token = epoch;
      const work = tail.then(async () => {
        if (token !== epoch) throw new Error('Asset selection changed. Retry the lookup.');
        const missingTypes = items.has(type) ? [] : [type];
        const missingIds = unique.filter(id => !enchants.has(id));
        if (missingTypes.length || missingIds.length) {
          const page = await fetch(missingTypes, missingIds);
          if (token !== epoch) throw new Error('Asset selection changed. Retry the lookup.');
          accept(page);
          // A changed fingerprint can retire rows that were cached before this fetch.
          if (!items.has(type) || unique.some(id => !enchants.has(id))) {
            const complete = await fetch([type], unique);
            if (token !== epoch) throw new Error('Asset selection changed. Retry the lookup.');
            accept(complete);
          }
        }
        return { item: read(items, type), enchants: requested.map(id => read(enchants, id)) };
      });
      tail = work.catch(() => {});
      return work;
    },
  };
}

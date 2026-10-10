import type { DropView } from '../bindings/DropView';

// Bridge/visit inputs and these borrowed results are immutable: replace a row
// or array to revise it. Weak keys retire projections with their input owners.
export const emptyDrops: DropView[] = [];
// Intentional in production too: one cheap freeze protects the shared empty array.
Object.freeze(emptyDrops);
const uniqueCache = new WeakMap<DropView[], DropView[]>();
export function uniqueDrops(drops: DropView[]): DropView[] {
  let result = uniqueCache.get(drops);
  if (!result) {
    const rows = new Map<string, DropView>();
    for (const drop of drops) rows.set(drop.id, drop);
    result = rows.size === drops.length ? drops : [...rows.values()];
    uniqueCache.set(drops, result);
  }
  return result;
}

/** One latest scope per array; keys must encode every predicate dependency. */
export function createDropScope() {
  const cache = new WeakMap<DropView[], { key: string; rows: DropView[] }>();
  return (drops: DropView[], key: string, accepts: (drop: DropView) => boolean): DropView[] => {
    const previous = cache.get(drops);
    if (previous?.key === key) return previous.rows;
    const filtered = drops.filter(accepts);
    const rows = filtered.length === drops.length ? drops : filtered;
    cache.set(drops, { key, rows });
    return rows;
  };
}

/** Shallow-freeze owned cached projections only; never traverse borrowed DTOs. */
export function freezeProjection<T extends object>(value: T): T {
  if (import.meta.env.DEV) Object.freeze(value);
  return value;
}

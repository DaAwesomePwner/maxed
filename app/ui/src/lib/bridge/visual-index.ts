import type { ObjectVisual } from '../bindings/ObjectVisual';

const cache = new WeakMap<ObjectVisual[], { objects: Map<number, ObjectVisual>; tiers: Map<number, string | null> }>();
/** Published visual pages are immutable. Keep find's first-row semantics and
 * the legacy tier Map's last-row semantics even for duplicate catalog rows. */
export function visualIndex(rows: ObjectVisual[]) {
  let index = cache.get(rows);
  if (!index) {
    index = { objects: new Map(), tiers: new Map() };
    for (const row of rows) {
      if (!index.objects.has(row.object_type)) index.objects.set(row.object_type, row);
      index.tiers.set(row.object_type, row.item_tier ?? null);
    }
    cache.set(rows, index);
  }
  return index;
}
export function objectVisual(rows: ObjectVisual[], type: number | null | undefined): ObjectVisual | undefined {
  return type == null ? undefined : visualIndex(rows).objects.get(type);
}

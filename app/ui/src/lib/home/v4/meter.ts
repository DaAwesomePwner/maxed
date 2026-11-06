import type { DamageRow } from '../v3/types';

/** Keep supplied ranks; the gap counts omitted rows before the local player. */
export function topWithSelf(rows: DamageRow[], limit = 6): Array<DamageRow | { gap: number }> {
  const top = rows.slice(0, limit), self = rows.findIndex(row => row.is_local);
  return self >= limit ? [...top, { gap: self - limit }, rows[self]] : top;
}

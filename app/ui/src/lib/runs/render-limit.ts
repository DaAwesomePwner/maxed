/** Rows mounted per step in saved tables. Realistic fights and runs fit in one step;
 * stored payloads allow thousands of rows, which would stall the WebView if mounted at once. */
export const RENDER_STEP = 200;

export type RenderLimit = { key: string; count: number };

/** The mounted row count for `key`; a different key starts again at one step. */
export function renderCount(limit: RenderLimit, key: string): number {
  return limit.key === key ? limit.count : RENDER_STEP;
}

/** The first `count` rows, plus any later rows `keep` pins (the local player). */
export function limitRows<T>(rows: T[], count: number, keep?: (row: T) => boolean): T[] {
  if (rows.length <= count) return rows;
  const pinned = keep ? rows.slice(count).filter(keep) : [];
  return [...rows.slice(0, count), ...pinned];
}

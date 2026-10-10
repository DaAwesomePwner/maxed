export function rankWindow<T extends { rank: number | null; is_local: boolean }>(
  rows: T[], size = 5,
): Array<T | { gap: number }> {
  const count = Number.isSafeInteger(size) && size > 0 ? size : 5;
  const rank = (row: T) => row.rank !== null && Number.isInteger(row.rank) && row.rank > 0
    ? row.rank : Number.MAX_SAFE_INTEGER;
  const ordered = [...rows].sort((a, b) => rank(a) - rank(b));
  const locals = ordered.filter(row => row.is_local);
  const index = locals.length === 1 ? ordered.indexOf(locals[0]) : -1;
  if (ordered.length <= count || index < count) return ordered.slice(0, count);
  const leaders = ordered.slice(0, Math.min(3, count));
  const start = Math.max(leaders.length, index - 1);
  const nearby = ordered.slice(start, index + 2);
  // This is omitted supplied rows, never a guessed census from rank numbers.
  const gap = start - leaders.length;
  return [...leaders, ...(gap > 0 ? [{ gap }] : []), ...nearby];
}

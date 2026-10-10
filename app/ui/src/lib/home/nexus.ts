import type { DropView } from '../bindings/DropView';
import type { FamePoint } from '../bindings/FamePoint';
import type { HomeRunDetail } from '../bindings/HomeRunDetail';
import type { HomeScope } from '../bindings/HomeScope';
import type { RunView } from '../bindings/RunView';
import { formatElapsed } from '../styles/format';
import { fameSegments } from './summary';

export const DUNGEON_PAGE_SIZE = 6;
export const DUNGEON_SERVER_LIMIT = 20;
export const RECENT_PAGE_SIZE = 6;
export const RECENT_SERVER_LIMIT = 10;

export function lootCount(count: number): string {
  if (count < 1000) return `×${count}`;
  const thousands = count < 10000 ? Math.round(count / 100) / 10 : Math.round(count / 1000);
  return `×${thousands}k`;
}

export function clearDuration(micros: number | null | undefined, observedClears: number): string {
  if (observedClears <= 0 || micros == null || !Number.isFinite(micros)
    || micros < 0 || !Number.isSafeInteger(Math.floor(micros))) return '\u2014';
  return formatElapsed('0', String(Math.floor(micros)));
}

export function observedTime(start: string, end: string | null): string {
  if (end === null) return '\u2014';
  const value = formatElapsed(start, end);
  return value === '\u2014' ? '\u2014' : value;
}

export function localTimestamp(at: string | null | undefined): string | null {
  if (!at || !/^\d+$/.test(at)) return null;
  const date = new Date(Number(BigInt(at) / 1000n));
  return Number.isFinite(date.getTime())
    ? date.toLocaleString(undefined, { month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit', timeZoneName: 'short' }) : null;
}

// The local-day range ends at the exclusive next midnight.
export function dayCaption(scope: HomeScope | null): string {
  if (scope && 'today' in scope) {
    const start = localTimestamp(scope.today.range.start), end = localTimestamp(scope.today.range.end);
    if (start && end) return `${start} – ${end}`;
  }
  return '\u2014';
}

export function runResult(run: RunView): string {
  const cleared = run.completed_at !== null || run.outcome === 'cleared' || run.evidence.some(({ evidence }) =>
    evidence === 'victory' || (typeof evidence === 'object' && ('completion_dialogue' in evidence || 'completion_counter' in evidence)));
  const died = run.died_at !== null || run.outcome === 'died' || run.evidence.some(({ evidence }) => evidence === 'death_packet');
  const labels = [...(cleared ? ['Cleared'] : []), ...(died ? ['Died'] : [])];
  if (run.outcome === 'left') labels.push('Left');
  return labels.length ? labels.join(' + ') : run.outcome === 'unknown' ? 'Unknown' : '\u2014';
}

export function bestPreviewBag(drops: DropView[], runId: string): DropView | null {
  const unique = new Map(drops.filter(drop => drop.run_id === runId).map(drop => [drop.id, drop]));
  return [...unique.values()].filter(drop => drop.bag_rank !== null && Number.isFinite(drop.bag_rank))
    .sort((a, b) => (b.bag_rank! - a.bag_rank!) || b.id.localeCompare(a.id))[0] ?? null;
}

export function recentRunRows(runs: RunView[], details: HomeRunDetail[]) {
  const unique = [...new Map(runs.map(run => [run.id, run])).values()].slice(0, RECENT_SERVER_LIMIT);
  return unique.map(run => {
    const matches = details.filter(detail => detail.run_id === run.id);
    const detail = matches.length === 1 ? matches[0] : null;
    const bag = detail ? bestPreviewBag(detail.loot, run.id) : null;
    const preview = bag ? [bag] : [...new Map((detail?.loot ?? []).filter(drop => drop.run_id === run.id)
      .map(drop => [drop.id, drop])).values()].slice(0, 3);
    return { run, detail, bag, preview };
  });
}

/** Segment before sampling so an omitted character switch cannot bridge two lines. */
export function famePreviewSegments(points: FamePoint[]): FamePoint[][] {
  const valid = points.filter(point => /^\d+$/.test(point.at) && Number.isFinite(point.fame) && point.fame >= 0);
  const indexed = fameSegments(valid).flatMap((segment, group) => segment.map(point => ({ point, group })));
  const sampled = indexed.length <= 120 ? indexed : Array.from({ length: 120 }, (_, i) => indexed[Math.round(i * (indexed.length - 1) / 119)]);
  const segments: FamePoint[][] = [];
  let previous = -1;
  for (const { point, group } of sampled) {
    if (group !== previous) segments.push([]);
    segments[segments.length - 1].push(point);
    previous = group;
  }
  return segments;
}

export function clockTime(at: string | null | undefined): string | null {
  if (!at || !/^\d+$/.test(at)) return null;
  const date = new Date(Number(BigInt(at) / 1000n));
  return Number.isFinite(date.getTime()) ? date.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : null;
}

import type { TimeRange } from '../bindings/TimeRange';
import type { RunSort } from '../bindings/RunSort';
import type { RunView } from '../bindings/RunView';
import type { RunRowDetail } from '../bindings/RunRowDetail';
import type { RunListSummary } from '../bindings/RunListSummary';
import type { KpiCell } from '../home/v3/types';
import { localDayRange } from '../home/scope';
import { formatElapsed, formatStat, formatTimeDelta } from '../styles/format';
import { plural } from '../loot/presentation';
import { coverageExplanation } from './damage';

export type RangePreset = 'all' | '30d' | '7d' | 'today';
export function presetRange(preset: RangePreset, now: Date): TimeRange | null {
  if (preset === 'all') return null;
  const first = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (preset === '30d' ? 29 : preset === '7d' ? 6 : 0));
  return { start: localDayRange(first).start, end: localDayRange(now).end };
}
export function presetFor(range: TimeRange | null, now: Date): RangePreset | null {
  if (range === null) return 'all';
  return (['30d', '7d', 'today'] as const).find(preset => {
    const candidate = presetRange(preset, now)!;
    return range.start === candidate.start && range.end === candidate.end;
  }) ?? null;
}
export const runDate = (at: string) => new Date(Number(BigInt(at) / 1000n));
export const shortDate = (at: string) => runDate(at).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
export const signedFame = (value: number | null) => value === null ? '—' : `${value > 0 ? '+' : ''}${formatStat(value)}`;
export const clearTime = (micros: number | null | undefined) => micros == null ? '—' : formatElapsed('0', String(Math.round(micros)));
const dayKey = (date: Date) => `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
export function dayGroups(rows: RunView[], details: Map<string, RunRowDetail>, now: Date, sort: RunSort = 'newest'): Array<{
  key: string; label: string | null; runs: number; cleared: number; fame: number | null; rows: RunView[];
}> {
  if (sort === 'duration_ascending' || sort === 'duration_descending') {
    return [{ key: 'duration', label: null, runs: rows.length, cleared: 0, fame: null, rows }];
  }
  const today = dayKey(now), yesterday = dayKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1));
  const groups = new Map<string, ReturnType<typeof dayGroups>[number]>();
  for (const row of rows) {
    const date = runDate(row.started), key = dayKey(date);
    let group = groups.get(key);
    if (!group) {
      group = { key, label: key === today ? 'Today' : key === yesterday ? 'Yesterday'
        : date.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' }), runs: 0, cleared: 0, fame: null, rows: [] };
      groups.set(key, group);
    }
    group.rows.push(row); group.runs++;
    if (row.outcome === 'cleared') group.cleared++;
    const fame = details.get(row.id)?.fame_delta;
    if (fame != null) group.fame = (group.fame ?? 0) + fame;
  }
  return [...groups.values()];
}
export function listKpis(summary: RunListSummary | null): KpiCell[] {
  const minutes = summary ? Math.floor(summary.time_in_runs_micros / 60_000_000) : null;
  return [
    { key: 'runs', label: 'Runs', value: formatStat(summary?.runs ?? null), sub: summary
      ? `${formatStat(summary.cleared)} cleared · ${summary.runs ? Math.round(summary.cleared / summary.runs * 100) + '%' : '—'}` : '—' },
    { key: 'deaths', label: 'Deaths', value: formatStat(summary?.died ?? null), tone: 'bad', sub: summary?.last_death
      ? `last: ${summary.last_death.dungeon}, ${shortDate(summary.last_death.at)}` : 'last: —' },
    { key: 'time', label: 'Time in dungeons', value: minutes === null ? '—' : `${Math.floor(minutes / 60)}h ${minutes % 60}m`,
      sub: `avg ${summary?.runs ? clearTime(summary.time_in_runs_micros / summary.runs) : '—'} per run` },
    { key: 'pbs', label: 'Dungeon records', tooltip: 'Fastest clear per dungeon', value: formatStat(summary?.personal_bests ?? null) },
    { key: 'fame', label: 'Run fame', value: signedFame(summary?.fame_delta ?? null), tone: 'fame',
      sub: summary ? `observed in ${formatStat(summary.fame_observed_runs)} ${plural(summary.fame_observed_runs, 'run')}` : '—' },
  ];
}
export function vsBest(detail: RunRowDetail | undefined): { text: string; title: string; tone: 'good' | 'plain' } | null {
  if (detail?.clear_micros == null) return null;
  const best = detail.is_personal_best ? detail.previous_best_clear_micros : detail.best_clear_micros;
  if (best == null) return null;
  const delta = BigInt(Math.round(detail.clear_micros - best));
  const title = formatTimeDelta(delta);
  const text = delta !== 0n && delta > -1_000_000n && delta < 1_000_000n ? `${delta < 0n ? '−' : '+'}<1s` : title;
  return { text, title, tone: detail.is_personal_best ? 'good' : 'plain' };
}

export function partialReasons(run: RunView, detail: RunRowDetail | undefined): string | null {
  const reasons = new Set<string>();
  if (!run.complete_capture) reasons.add('Run capture is incomplete.');
  for (const issue of run.issues) reasons.add(coverageExplanation[issue]);
  // Child uncertainty is aggregated into a boolean; individual reasons are not
  // available in the list reply.
  if (detail?.share_partial) reasons.add('Damage share is based on partial observations or estimated damage.');
  return reasons.size ? [...reasons].join(' ') : null;
}

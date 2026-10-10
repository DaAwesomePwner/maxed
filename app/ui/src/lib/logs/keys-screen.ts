import type { EventSummary } from '../bindings/EventSummary';
import type { TimeRange } from '../bindings/TimeRange';
import type { AlertPreferences } from '../bindings/AlertPreferences';
import type { KpiCell } from '../home/v3/types';
import { perDay } from './logs-format';
import { shortWhen } from '../loot/loot-format';
import { runDate } from '../runs/runs-list';

export function mutedDungeons(prefs: AlertPreferences, all: string[]): Set<string> {
  if (prefs.key_pop_dungeons !== null) return new Set(all.filter(dungeon => !prefs.key_pop_dungeons!.includes(dungeon)));
  return new Set(prefs.key_pop_excluded_dungeons);
}

export function keyKpis(summary: EventSummary | null, range: TimeRange | null, muted: Set<string>, now: Date): KpiCell[] {
  const known = summary !== null && summary.by_kind.portal_opened > 0;
  const count = summary?.by_kind.portal_opened ?? 0;
  const most = summary?.dungeons.reduce<EventSummary['dungeons'][number] | null>((best, row) => !best || row.pops > best.pops ? row : best, null);
  const last = summary?.last_local_pop;
  const start = range?.start ?? summary?.first_at;
  const flagged = summary?.flagged_since;
  return [
    { key: 'pops', label: 'Key pops', value: known ? count.toLocaleString('en-US') : '—', sub: known ? perDay(count, range, summary.first_at, now) : null },
    { key: 'mine', label: 'Opened by you', value: known && flagged != null && (!range || BigInt(range.end) > BigInt(flagged)) ? summary.local_pops.toLocaleString('en-US') : '—', tone: 'fame',
      sub: last ? `last: ${last.dungeon}, ${shortWhen(last.last_at, now)}` : null,
      tooltip: flagged && start && BigInt(flagged) > BigInt(start) ? `Counted since ${runDate(flagged).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' })}` : null },
    { key: 'most', label: 'Most popped', value: known && most ? most.dungeon : '—', sub: known && most ? `${most.pops.toLocaleString('en-US')} pops · ${Math.round(most.pops / count * 100)}% of all` : null },
    { key: 'dungeons', label: 'Dungeons', value: known ? summary.dungeons.length.toLocaleString('en-US') : '—', sub: known ? `${muted.size} muted for alerts` : null },
  ];
}

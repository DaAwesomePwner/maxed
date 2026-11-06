import { keyPopAllows } from '../alerts/preferences';
import { dungeonMetadata } from '../alerts/dungeon-catalog';
import type { DungeonDef } from '../bindings/DungeonDef';
import type { EventSummary } from '../bindings/EventSummary';
import type { TimeRange } from '../bindings/TimeRange';
import type { AlertPreferences } from '../bindings/AlertPreferences';
import type { KpiCell } from '../home/v3/types';
import { perDay } from './logs-format';
import { shortWhen } from '../loot/loot-format';
import { runDate } from '../runs/runs-list';

export function mutedDungeons(prefs: AlertPreferences, all: string[], catalog?: DungeonDef[]): Set<string> {
  return new Set([...new Set([...all, ...(catalog?.map(row => row.name) ?? []), ...(prefs.key_pop_dungeons === null ? prefs.key_pop_excluded_dungeons : [])])]
    .filter(dungeon => !keyPopAllows(prefs, dungeon, dungeonMetadata(dungeon, catalog))));
}

export function keyKpis(summary: EventSummary | null, range: TimeRange | null, muted: Set<string>, now: Date, preferences?: AlertPreferences | null): KpiCell[] {
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
    { key: 'dungeons', label: 'Dungeons', value: known ? summary.dungeons.length.toLocaleString('en-US') : '—', sub: preferences?.key_popped === 'off' ? 'Key-pop alerts off' : preferences?.key_pop_dungeons?.length === 0 ? 'No dungeons selected' : known ? `${muted.size} ${preferences?.key_pop_exalt_only || preferences?.key_pop_min_difficulty != null ? 'muted or filtered for alerts' : 'muted for alerts'}` : null },
  ];
}

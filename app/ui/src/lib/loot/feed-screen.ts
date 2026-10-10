import type { DropSummary } from '../bindings/DropSummary';
import type { DropTotals } from '../bindings/DropTotals';
import type { LootDropRow } from '../bindings/LootDropRow';
import type { KpiCell } from '../home/v3/types';
import { localDayRange } from '../home/scope';
import { runDate } from '../runs/runs-list';
import { plural } from './presentation';
import { shortWhen, whiteRate } from './loot-format';
const count = (value: number | null | undefined) => value == null ? '—' : value.toLocaleString('en-US');
export function feedKpis(summary: DropSummary | null): KpiCell[] {
  const t = summary?.totals, known = t && t.bags > 0, items = known && t.items_counted === true;
  const tooltip = t && !t.items_counted ? 'Narrow the date range to count items' : null;
  return [
    { key: 'bags', label: 'Bags', value: known ? count(t.bags) : '—', sub: known ? `in ${count(t.runs)} runs · ${t.runs ? (t.bags / t.runs).toFixed(1) : '—'} per run` : null },
    { key: 'whites', label: 'White bags', value: known ? count(t.whites) : '—', sub: t?.last_white ? `last: ${t.last_white.dungeon ?? 'Outside a run'}, ${shortWhen(t.last_white.at, new Date())}` : null },
    { key: 'rate', label: 'White rate', value: known ? whiteRate(t.whites, t.runs_ended) : '—', sub: t ? t.runs_ended === null ? 'Outside a run: counts only' : `over ${count(t.runs_ended)} ended runs` : null },
    { key: 'ut_st', label: 'UT and ST items', value: items && t.ut_items != null && t.st_items != null ? count(t.ut_items + t.st_items) : '—', sub: items ? `${count(t.ut_items)} UT · ${count(t.st_items)} ST` : null, tooltip },
    { key: 'enchanted', label: 'Enchanted items', value: items ? count(t.enchanted_items) : '—', sub: items && t.best_enchants != null ? `best: ${count(t.best_enchants)} ${plural(t.best_enchants, 'enchant')}` : null, tooltip },
  ];
}
export function dayGroups(rows: LootDropRow[], now: Date) {
  const groups = new Map<string, { key: string; label: string; range: ReturnType<typeof localDayRange>; rows: LootDropRow[] }>();
  for (const row of rows) {
    const date = runDate(row.drop.at);
    let range: ReturnType<typeof localDayRange>;
    try { range = localDayRange(date); }
    catch {
      // The first representable timestamp can fall on a local day starting before the epoch.
      const end = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1).getTime();
      if (!Number.isFinite(end) || end <= 0) continue;
      range = { start: '0', end: String(BigInt(end) * 1000n) };
    }
    const key = range.start;
    if (!groups.has(key)) {
      const when = shortWhen(row.drop.at, now);
      groups.set(key, { key, range, label: when === 'Today' || when === 'Yesterday' ? when : date.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' }), rows: [] });
    }
    groups.get(key)!.rows.push(row);
  }
  return [...groups.values()];
}
export function dayLine(totals: DropTotals | null): string {
  if (!totals) return '';
  return [`${count(totals.bags)} ${totals.bags === 1 ? 'bag' : 'bags'}`,
    totals.whites ? `${count(totals.whites)} ${totals.whites === 1 ? 'white' : 'whites'}` : null,
    totals.items_counted && totals.ut_items ? `${count(totals.ut_items)} UT` : null].filter(Boolean).join(' · ');
}
export function headerLine(totals: DropTotals | null): string | null {
  if (!totals?.bags) return null;
  return `${count(totals.bags)} ${totals.bags === 1 ? 'bag' : 'bags'} seen${totals.first_at ? ` · since ${runDate(totals.first_at).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}` : ''}`;
}

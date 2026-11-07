import { compareItems } from './ranking';
import type { DropItem } from '../bindings/DropItem';
import type { TimeRange } from '../bindings/TimeRange';
import type { Timestamp } from '../bindings/Timestamp';
import { runDate } from '../runs/runs-list';

export type TierClass = 'ut' | 'st' | '';
export function tierClass(tier: string | null | undefined): TierClass {
  return tier === 'UT' ? 'ut' : tier === 'ST' ? 'st' : '';
}
export function bestItem(items: DropItem[], tierOf: (type: number) => string | null): DropItem | null {
  const key = (item: DropItem) => ({ tier: tierOf(item.item_type), itemType: item.item_type, slot: item.slot });
  return items.reduce<DropItem | null>((best, item) => !best || compareItems(key(item), key(best)) < 0 ? item : best, null);
}
const day = (date: Date) => date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
const key = (date: Date) => `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
export function rangeLabel(range: TimeRange): string {
  return `${day(runDate(range.start))} – ${day(runDate(String(BigInt(range.end) - 1n)))}`;
}
export function shortWhen(at: Timestamp, now: Date): string {
  const date = runDate(at);
  return key(date) === key(now) ? 'Today'
    : key(date) === key(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1)) ? 'Yesterday' : day(date);
}
export function clockWhen(at: Timestamp, now: Date): string {
  const date = runDate(at);
  return `${key(date) === key(now) ? 'today' : day(date)}, ${date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })}`;
}
export function whiteRate(whites: number, runsEnded: number | null): string {
  return runsEnded === null || runsEnded === 0 ? '—' : `${(whites / runsEnded * 100).toFixed(1)}%`;
}

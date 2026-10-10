import type { ChatRow } from '../bindings/ChatRow';
import type { WhisperPartner } from '../bindings/WhisperPartner';
import { runDate } from '../runs/runs-list';
import { dayLabel } from './logs-format';
import type { ChatPage } from '../bindings/ChatPage';
import { parseRoute, routeHref } from '../navigation/routes';

export function routablePartner(name: string): boolean {
  return parseRoute(routeHref({ kind: 'logs', view: 'chat', channel: 'whisper', partner: name })).kind === 'logs';
}

export function reachesLoadedRows(pages: ChatRow[][], page: ChatPage): boolean {
  if (!page.next) return true;
  const oldest = newestRows(page.rows).at(-1)?.message;
  const newest = newestRows(pages.flat())[0]?.message;
  return !!oldest && !!newest && (BigInt(oldest.at) < BigInt(newest.at)
    || oldest.at === newest.at && oldest.id <= newest.id);
}

export function newestRows(rows: ChatRow[]): ChatRow[] {
  return [...new Map(rows.map(row => [row.message.id, row])).values()].sort((a, b) => {
    const delta = BigInt(b.message.at) - BigInt(a.message.at);
    return delta < 0n ? -1 : delta > 0n ? 1 : a.message.id < b.message.id ? 1 : a.message.id > b.message.id ? -1 : 0;
  });
}
export function threadItems(pages: ChatRow[][], now: Date): Array<{ type: 'day'; label: string } | { type: 'line'; row: ChatRow; outgoing: boolean }> {
  const items: ReturnType<typeof threadItems> = [];
  let previous = '';
  for (const row of newestRows(pages.flat()).reverse()) {
    const message = row.message, date = runDate(message.at);
    const key = JSON.stringify([date.getFullYear(), date.getMonth(), date.getDate(), message.run_id ?? null, message.map ?? null]);
    if (key !== previous) items.push({ type: 'day', label: `${dayLabel(message.at, now)} · ${message.map ?? '—'}` });
    items.push({ type: 'line', row, outgoing: message.direction === 'outgoing' });
    previous = key;
  }
  return items;
}
export function excerpt(partner: WhisperPartner): string {
  return `${partner.last_direction === 'outgoing' ? 'you: ' : ''}${partner.last_text}`;
}

/** Replace the refreshed time window, retaining only the previously loaded older tail. */
export function appendNewest(pages: ChatRow[][], newest: ChatRow[]): ChatRow[] {
  const boundary = newestRows(newest).at(-1)?.message;
  if (!boundary) return [];
  const older = pages.flat().filter(({ message }) => BigInt(message.at) < BigInt(boundary.at)
    || message.at === boundary.at && message.id < boundary.id);
  return newestRows([...older, ...newest]).slice(0, 500);
}

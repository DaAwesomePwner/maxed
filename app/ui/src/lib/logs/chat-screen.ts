import type { ChatRow } from '../bindings/ChatRow';
import type { ChatOverview } from '../bindings/ChatOverview';
import type { WhisperPartner } from '../bindings/WhisperPartner';
import { channelTag, clockTime, dayLabel, type ChannelTag } from './logs-format';

export type ChatItem = { type: 'place'; key: string; label: string; count: number; runId: string | null }
  | { type: 'line'; row: ChatRow };

export function placeGroups(rows: ChatRow[], now: Date): ChatItem[] {
  const result: ChatItem[] = [];
  for (let start = 0; start < rows.length;) {
    const first = rows[start].message;
    let end = start + 1;
    while (end < rows.length && (rows[end].message.run_id ?? null) === (first.run_id ?? null) && (rows[end].message.map ?? null) === (first.map ?? null)) end++;
    const oldest = rows.slice(start, end).reduce((a, b) => BigInt(a.message.at) < BigInt(b.message.at) ? a : b).message;
    result.push({ type: 'place', key: first.id, label: `${dayLabel(oldest.at, now)} · ${clockTime(oldest.at)} · ${first.map ?? '—'}`, count: end - start, runId: first.run_id ?? null });
    for (const row of rows.slice(start, end)) result.push({ type: 'line', row });
    start = end;
  }
  return result;
}

export function chatSubline(overview: ChatOverview | null, retentionDays: number | null): string | null {
  if (!overview?.lines) return null;
  return `${overview.lines.toLocaleString('en-US')} chat lines${retentionDays === null ? '' : ` · kept ${retentionDays} days`}`;
}

export function senderSuggestions(rows: ChatRow[], partners: WhisperPartner[], prefix: string): Array<{ name: string; channel: ChannelTag }> {
  const seen = new Set<string>();
  return [...rows.map(row => ({ name: row.message.sender, channel: channelTag(row.message) })),
    ...partners.map(partner => ({ name: partner.name, channel: 'whisper' as const }))].filter(item => {
    const key = item.name.toLowerCase();
    if (seen.has(key) || !key.startsWith(prefix.toLowerCase())) return false;
    seen.add(key); return true;
  }).slice(0, 8);
}

export function copyText(row: ChatRow): string {
  return `[${clockTime(row.message.at)}] ${row.message.sender}: ${row.message.text}${row.message.truncated ? '…' : ''}`;
}

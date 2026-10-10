import type { ChatMessageView } from '../bindings/ChatMessageView';
import type { Timestamp } from '../bindings/Timestamp';
import type { TimeRange } from '../bindings/TimeRange';
import { runDate } from '../runs/runs-list';

export type ChannelTag = 'public' | 'guild' | 'party' | 'whisper' | 'npc' | 'unknown';
export const channelTagLabel: Record<ChannelTag, string> = {
  public: 'Public', guild: 'Guild', party: 'Party', whisper: 'Whisper', npc: 'NPC', unknown: 'Unknown',
};
export function channelTag(message: ChatMessageView): ChannelTag {
  return message.sender_kind === 'npc' ? 'npc' : message.channel;
}
export function clockTime(at: Timestamp): string {
  return runDate(at).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
}
const dayKey = (date: Date) => `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
export function dayLabel(at: Timestamp, now: Date): string {
  const date = runDate(at);
  if (dayKey(date) === dayKey(now)) return 'Today';
  if (dayKey(date) === dayKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1))) return 'Yesterday';
  return date.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' });
}
export function perDay(count: number, range: TimeRange | null, firstAt: Timestamp | null, now: Date): string {
  const start = range?.start ?? firstAt;
  if (start === null) return '—';
  const duration = Number(BigInt(range?.end ?? String(now.getTime() * 1000)) - BigInt(start)) / 86_400_000_000;
  return duration > 0 ? `${(count / Math.max(1, duration)).toFixed(1)} per day` : '—';
}
export function countLine(count: number | null, noun: string, since: Timestamp | null): string | null {
  if (count === null || count === 0) return null;
  const date = since === null ? '' : ` · since ${runDate(since).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}`;
  return `${count.toLocaleString('en-US')} ${noun}${date}`;
}

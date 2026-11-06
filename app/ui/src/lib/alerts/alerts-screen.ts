import type { AlertPreferences } from '../bindings/AlertPreferences';
import type { AlertFilter } from '../bindings/AlertFilter';
import type { AlertKind } from '../bindings/AlertKind';
import type { AlertLoudness } from '../bindings/AlertLoudness';
import type { AlertSummary } from '../bindings/AlertSummary';
import type { AlertView } from '../bindings/AlertView';
import type { TimeRange } from '../bindings/TimeRange';
import type { KpiCell } from '../home/v3/types';
import { perDay } from '../logs/logs-format';
import { shortWhen } from '../loot/loot-format';
import { alertTitle } from './presentation';
import { alertDestination } from './destination';

export type AlertTile = 'all' | 'white' | 'pb' | 'keys' | 'messages';
export const tiles: AlertTile[] = ['all', 'white', 'pb', 'keys', 'messages'];
export const kindLabel: Record<AlertKind, string> = { white_bag: 'White bag', personal_best: 'Personal best', key_popped: 'Key popped', whisper: 'Whisper', guild_message: 'Guild message', party_message: 'Party message', death: 'Death', flawless_run: 'Flawless run' };
export const kindGlyph: Record<AlertKind, string> = { white_bag: 'bag', personal_best: 'trophy', key_popped: 'portal', whisper: 'bubble', guild_message: 'bubble', party_message: 'bubble', death: 'skull', flawless_run: 'shield' };
export function tileFilter(tile: AlertTile): Pick<AlertFilter, 'kind' | 'kinds'> {
  return { kind: tile === 'white' ? 'white_bag' : tile === 'pb' ? 'personal_best' : tile === 'keys' ? 'key_popped' : null,
    kinds: tile === 'messages' ? ['whisper', 'guild_message', 'party_message'] : [] };
}
export function alertKpis(summary: AlertSummary | null, range: TimeRange | null, mutedCount: number, now: Date, preferences?: AlertPreferences | null): KpiCell[] {
  const known = !!summary?.total;
  const count = (kind: AlertKind) => summary?.by_kind.find(row => row.kind === kind)?.count ?? 0;
  const value = (n: number) => known ? n.toLocaleString('en-US') : '—';
  const last = summary?.last_white;
  return [
    { key: 'all', label: 'All alerts', value: value(summary?.total ?? 0), sub: known ? perDay(summary!.total, range, summary!.first_at, now) : null },
    { key: 'white', label: 'White bags', value: value(count('white_bag')), sub: last ? last.dungeon ? `last: ${last.dungeon}, ${shortWhen(last.at, now)}` : `last: ${shortWhen(last.at, now).toLowerCase()}` : '—' },
    { key: 'pb', label: 'Personal bests', value: value(count('personal_best')), sub: 'best DPS and clear times', tone: 'fame' },
    { key: 'keys', label: 'Keys popped', value: value(count('key_popped')), sub: preferences?.key_popped === 'off' ? 'Key-pop alerts off' : preferences?.key_pop_dungeons?.length === 0 ? 'No dungeons selected' : `${mutedCount < 0 ? '—' : mutedCount} dungeons muted` },
    { key: 'messages', label: 'Messages', value: value(count('whisper') + count('guild_message') + count('party_message')), sub: known ? `${count('whisper')} whisper · ${count('guild_message')} guild · ${count('party_message')} party` : null },
  ];
}
export function alertHeading(alert: AlertView): string {
  return alert.details?.type === 'chat' || alert.details?.type === 'personal_best' || alert.details?.type === 'run_time_personal_best' || alert.details?.type === 'key_popped'
    ? alertTitle(alert) : kindLabel[alert.kind];
}
export function shownAs(presentation: AlertLoudness | null | undefined): string {
  return presentation ? ({ banner: 'Banner', toast: 'Toast', quiet_log_only: 'Log only', off: 'Off' })[presentation] : '—';
}
export function whyItFired(alert: AlertView): string {
  const detail = alert.details;
  const reason = alert.kind === 'white_bag' ? 'White bag observed'
    : alert.kind === 'personal_best' ? detail?.type === 'run_time_personal_best' ? `A new clear-time personal best in ${detail.dungeon}` : detail?.type === 'personal_best' ? 'A new DPS personal best was recorded' : 'A personal best was recorded'
    : alert.kind === 'key_popped' ? `${detail?.type === 'key_popped' ? detail.dungeon + ' key' : 'A key'} was seen opening`
    : ['whisper', 'guild_message', 'party_message'].includes(alert.kind) ? `${kindLabel[alert.kind]}${detail?.type === 'chat' ? ` from ${detail.sender}` : ''} was observed`
    : alert.kind === 'death' ? 'Your character’s death was observed' : 'A flawless run was recorded';
  return `${reason}. ${alert.presentation ? `This alert was recorded as ${shownAs(alert.presentation)}.` : 'Its presentation is unknown.'}`;
}
export function alertAction(alert: AlertView): { label: string; href?: string; mute?: string } | null {
  const detail = alert.details;
  if (alert.kind === 'key_popped') return detail?.type === 'key_popped' ? { label: 'Mute', mute: detail.dungeon } : null;
  if (detail?.type === 'chat') {
    const partner = detail.sender.length > 0 && new TextEncoder().encode(detail.sender).length <= 32 && !/[\u0000-\u001f\u007f-\u009f]/.test(detail.sender);
    return { label: 'Open chat', href: detail.channel === 'whisper' ? partner ? `#/logs/chat/whisper/${encodeURIComponent(detail.sender)}` : '#/logs/chat/whisper' : detail.channel === 'guild' || detail.channel === 'party' ? `#/logs/chat/${detail.channel}` : '#/logs' };
  }
  const href = alertDestination(alert) ?? (alert.kind === 'flawless_run' && alert.run_id && alert.run_id.length <= 256 && !/[\u0000-\u001f\u007f]/.test(alert.run_id) ? `#/runs/${encodeURIComponent(alert.run_id)}` : null);
  return href ? { label: detail?.type === 'personal_best' ? 'View fight' : 'View run', href } : null;
}

import type { InstanceEventView } from '../bindings/InstanceEventView';
import type { ChatMessageView } from '../bindings/ChatMessageView';

/** Local HH:MM for a microsecond Timestamp string; '—' when unparsable. */
export function chatTime(at: string): string {
  if (!/^\d+$/.test(at)) return '—';
  const date = new Date(Number(BigInt(at) / 1000n));
  return Number.isFinite(date.getTime()) ? date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }) : '—';
}

/** "From X" / "To X" for whispers; the sender otherwise. */
export function chatLineLabel(message: ChatMessageView): string {
  if (message.channel !== 'whisper') return message.sender;
  if (message.direction === 'incoming') return `From ${message.sender}`;
  if (message.direction === 'outgoing') return `To ${message.counterpart ?? 'Unknown player'}`;
  return `${message.sender} → ${message.counterpart ?? 'Unknown player'}`;
}

export function eventLabel(view: InstanceEventView): string {
  const kind = view.kind;
  const name = (value: string | null | undefined) => value || 'Unknown player';
  let text: string;
  switch (kind.type) {
    case 'player_death': text = `${name(kind.player_name)} died${kind.killed_by ? ` to ${kind.killed_by}` : ''}`; break;
    case 'player_left': text = `${name(kind.player_name)} left`; break;
    case 'portal_opened': text = `${name(kind.opener)} opened ${kind.dungeon ?? 'a portal'}`; break;
    case 'realm_event_killed': text = `${kind.boss_key} killed${kind.killer ? ` by ${kind.killer}` : ''}`; break;
  }
  return view.provisional ? `${text} (unconfirmed)` : text;
}

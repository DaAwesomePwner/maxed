import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';
import { defaultLogsPosition, type ListState } from '../navigation/state';
import { invalidatesLogsHistory } from './invalidation';
import { invalidatesAlertHistory } from '../alerts/invalidation';

/** Narrow occurrences using the active view, without guessing stored search/hidden matches. */
export function matchesPendingView(route: Route | undefined, state: Pick<ListState, 'logsPosition'> | undefined, envelope: EventEnvelope): boolean {
  if (invalidatesAlertHistory(route, envelope)) return true;
  if (!invalidatesLogsHistory(route, envelope) || route?.kind !== 'logs') return false;
  const position = state?.logsPosition ?? defaultLogsPosition();
  if ('chat' in envelope.event) {
    return route.channel === 'whisper' && !route.runId
      || position.chat.filter.show_npc || envelope.event.chat.sender_kind !== 'npc';
  }
  if ('instance_event' in envelope.event) {
    const event = envelope.event.instance_event;
    const filter = route.view === 'keys' ? position.keys.filter : position.events.filter;
    if (filter.kinds.length && !filter.kinds.includes(event.kind.type)) return false;
    if (position.range && (BigInt(event.at) < BigInt(position.range.start) || BigInt(event.at) >= BigInt(position.range.end))) return false;
    if (event.kind.type === 'portal_opened') {
      if (filter.dungeon && event.kind.dungeon !== filter.dungeon) return false;
      if (filter.opener_local != null && event.kind.opener_is_local !== filter.opener_local) return false;
    }
    return true;
  }
  return false;
}

/** Counts accepted matching envelopes, not rows or gaps in engine sequence numbers. */
export function createLogsPending() {
  let entry = '', sequences: string[] = [];
  return {
    count(key: string) { return key === entry ? sequences.length : 0; },
    accept(key: string, sequence: string) {
      if (entry !== key) { entry = key; sequences = []; }
      sequences.push(sequence);
      return sequences.length;
    },
    applied(key: string, required: string | null) {
      if (key === entry && required !== null) sequences = sequences.filter(sequence => BigInt(sequence) > BigInt(required));
      return key === entry ? sequences.length : 0;
    },
    reset() { entry = ''; sequences = []; },
  };
}

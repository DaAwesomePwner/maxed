import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';

/** Source revisions use the shared durable history checkpoint. */
export function invalidatesAlertHistory(route: Route | undefined, envelope: EventEnvelope): boolean {
  if (route?.kind !== 'alerts' && !(route?.kind === 'logs' && route.view === 'alerts')) return false;
  if ('chat' in envelope.event) {
    const chat = envelope.event.chat;
    return chat.sender_kind === 'player' && !chat.is_local &&
      (chat.channel === 'guild' || chat.channel === 'party' ||
        (chat.channel === 'whisper' && chat.direction === 'incoming'));
  }
  return Object.keys(envelope.event).some(kind => SOURCES.has(kind));
}

// Quiet occurrences have no AlertFired; these source facts can create or enrich them.
const SOURCES = new Set(['alert_fired', 'fight_ended', 'bag_dropped', 'drop_updated',
  'instance_event', 'player_died', 'character_merged', 'character_updated', 'run_updated', 'run_ended']);

import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { HistoryQuery } from './history';

export function queryInvalidates(query: HistoryQuery, event: EventEnvelope): boolean {
  const e = event.event;
  switch (query.kind) {
    case 'chat-overview':
      return 'chat' in e && (query.request.run_id == null || e.chat.run_id === query.request.run_id);
    case 'chat-sender': return 'chat' in e;
    case 'whisper-partners': return 'chat' in e && e.chat.channel === 'whisper';
    case 'event-summary':
      return 'instance_event' in e && (query.request.filter.run_id == null || e.instance_event.run_id === query.request.filter.run_id);
    case 'run':
      if ('character_updated' in e || 'character_merged' in e) return true;
      if ('run_started' in e) return e.run_started.id === query.id;
      if ('run_updated' in e) return e.run_updated.id === query.id;
      if ('run_ended' in e) return e.run_ended.id === query.id;
      if ('bag_dropped' in e) return e.bag_dropped.run_id === query.id;
      if ('drop_updated' in e) return e.drop_updated.run_id === query.id;
      if ('fight_started' in e) return e.fight_started.run_id === query.id;
      if ('fight_updated' in e) return e.fight_updated.run_id === query.id;
      if ('fight_ended' in e) return e.fight_ended.run_id === query.id;
      if ('player_died' in e) return e.player_died.run_id === query.id;
      return 'instance_event' in e && e.instance_event.run_id === query.id;
    // Primary lists remain covered by their route's invalidation predicate.
    default: return false;
  }
}

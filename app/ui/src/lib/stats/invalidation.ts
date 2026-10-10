import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';

/** Fame updates alone do not re-read Stats: the open visit's fame refreshes at the next map change. */
export function invalidatesStatsHistory(route: Route | undefined, envelope: EventEnvelope): boolean {
  if (route?.kind !== 'stats') return false;
  const event = envelope.event;
  return 'map_entered' in event || 'run_ended' in event || 'session_updated' in event;
}

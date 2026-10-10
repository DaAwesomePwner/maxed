import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';

/** Called synchronously by both the subscription and Shell watermark owner. */
export function invalidatesRunHistory(route: Route | undefined, envelope: EventEnvelope): boolean {
  if (!route || !['runs', 'run', 'fight', 'player'].includes(route.kind)) return false;
  const event = envelope.event;
  if ('run_started' in event || 'run_updated' in event || 'run_ended' in event
    || 'bag_dropped' in event || 'drop_updated' in event || 'player_died' in event
    || 'character_merged' in event || 'character_updated' in event) return true;
  if ('instance_event' in event) {
    const runId = event.instance_event.run_id;
    return runId != null && (route.kind === 'runs' || (route.kind === 'run' && route.id === runId));
  }
  if (route.kind === 'runs') return false;
  const fight = 'fight_started' in event ? event.fight_started : 'fight_updated' in event ? event.fight_updated
    : 'fight_ended' in event ? event.fight_ended : null;
  if (!fight) return false;
  if (route.kind === 'run') return fight.run_id === route.id;
  if (route.kind === 'fight') return fight.id === route.id;
  return route.kind === 'player' && fight.id === route.fightId;
}

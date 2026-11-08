import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { FightRelationship, Route } from '../navigation/routes';

// Only enrich invalidation scope; keep the URL and saved navigation entry unchanged.
export function runInvalidationRoute(
  route: Route | undefined, relationship: Pick<FightRelationship, 'fightId' | 'runId'> | null,
): Route | undefined {
  if (route?.kind !== 'player' || route.runId != null || relationship?.fightId !== route.fightId
    || relationship.runId == null) return route;
  return { ...route, runId: relationship.runId };
}

/** Called synchronously by both the subscription and Shell watermark owner. */
export function invalidatesRunHistory(route: Route | undefined, envelope: EventEnvelope): boolean {
  if (!route || !['runs', 'run', 'fight', 'player', 'run-player'].includes(route.kind)) return false;
  const event = envelope.event;
  if (route.kind === 'run-player') {
    if ('character_merged' in event) return true;
    if ('run_started' in event) return event.run_started.id === route.runId;
    if ('run_updated' in event) return event.run_updated.id === route.runId;
    if ('run_ended' in event) return event.run_ended.id === route.runId;
    if ('fight_started' in event) return event.fight_started.run_id === route.runId;
    if ('fight_updated' in event) return event.fight_updated.run_id === route.runId;
    if ('fight_ended' in event) return event.fight_ended.run_id === route.runId;
    if ('player_died' in event) return event.player_died.run_id === route.runId;
    if ('instance_event' in event) return event.instance_event.run_id === route.runId;
    if ('player_observed' in event) return event.player_observed.run_id === route.runId;
    if ('scope_timeline_updated' in event) return event.scope_timeline_updated.run_id === route.runId;
    return false;
  }
  if ('player_observed' in event || 'scope_timeline_updated' in event) {
    const notice = 'player_observed' in event ? event.player_observed : event.scope_timeline_updated;
    return route.kind === 'player' && route.runId != null && notice.run_id === route.runId;
  }
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

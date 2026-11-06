import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { PartyChangedNotice } from '../bridge/client';
import type { Route } from '../navigation/routes';

/** Saved views consume engine-event envelopes, including those delivered in batches. */
export function invalidatesParty(route: Route | undefined, envelope: EventEnvelope): boolean {
  const event = envelope.event;
  if (route?.kind === 'party-players' || route?.kind === 'party-player') return 'run_ended' in event;
  if (route?.kind !== 'party-run') return false;
  if ('run_ended' in event) return event.run_ended.id === route.runId;
  if ('fight_started' in event) return event.fight_started.run_id === route.runId;
  if ('fight_updated' in event) return event.fight_updated.run_id === route.runId;
  if ('fight_ended' in event) return event.fight_ended.run_id === route.runId;
  return false;
}

/** Live refreshes on every roster notice, including a reset or a different scope. */
export function partyNoticeInvalidates(route: Route | undefined, _notice: PartyChangedNotice): boolean {
  return route?.kind === 'party-live';
}

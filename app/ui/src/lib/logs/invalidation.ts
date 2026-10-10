import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';

/** Called synchronously by the Shell watermark owner, like the runs and loot predicates. */
export function invalidatesLogsHistory(route: Route | undefined, envelope: EventEnvelope): boolean {
  if (route?.kind !== 'logs') return false;
  const event = envelope.event;
  if ('chat' in event) {
    if (route.view !== 'chat') return false;
    const message = event.chat;
    if (route.runId && message.run_id !== route.runId) return false;
    // Partner and sender routes still refresh on channel occurrences: hidden state
    // and the conversation list may change even when this sender is not selected.
    return route.channel === 'all' || route.channel === message.channel;
  }
  if ('instance_event' in event) {
    const view = event.instance_event;
    if (route.runId && view.run_id !== route.runId) return false;
    if (route.view === 'events') return true;
    return route.view === 'keys' && view.kind.type === 'portal_opened';
  }
  return false;
}

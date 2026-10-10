import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';

/** Called for each accepted event by the Shell history watermark owner. */
export function invalidatesLootHistory(route: Route | undefined, envelope: EventEnvelope): boolean {
  if (!route || (route.kind !== 'loot' && route.kind !== 'item')) return false;
  const event = envelope.event;
  const drop = 'bag_dropped' in event ? event.bag_dropped : 'drop_updated' in event ? event.drop_updated : null;
  if (route.kind === 'item') return 'run_ended' in event || (drop !== null
    && (drop.contents.some(item => item.item_type === route.type) || drop.items.includes(route.type)));
  return drop !== null || 'run_ended' in event;
}

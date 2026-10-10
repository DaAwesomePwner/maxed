import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';
import { invalidatesRunHistory } from '../runs/invalidation';
import { invalidatesAlertHistory } from '../alerts/invalidation';
import { invalidatesStatsHistory } from '../stats/invalidation';
import { invalidatesLogsHistory } from '../logs/invalidation';
import { invalidatesLootHistory } from '../loot/invalidation';

export function invalidatesSavedHistory(route: Route | undefined, event: EventEnvelope): boolean {
  return invalidatesRunHistory(route, event)
    || invalidatesAlertHistory(route, event)
    || invalidatesLootHistory(route, event)
    || invalidatesLogsHistory(route, event)
    || invalidatesStatsHistory(route, event);
}

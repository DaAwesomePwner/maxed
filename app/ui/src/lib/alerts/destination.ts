import type { AlertView } from '../bindings/AlertView';
import { routeHref } from '../navigation/routes';

const validId = (value: string | null | undefined): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= 256 && !/[\u0000-\u001f\u007f]/.test(value);

/** Use only the occurrence's saved typed relationship, never the current run. */
export function alertDestination(alert: AlertView): string | null {
  if (alert.kind === 'personal_best') {
    const details = alert.details;
    if (details?.type === 'run_time_personal_best') {
      return validId(alert.run_id) ? routeHref({ kind: 'run', id: alert.run_id }) : null;
    }
    if (details?.type !== 'personal_best' || !validId(details.fight_id)) return null;
    if (alert.run_id !== null && !validId(alert.run_id)) return null;
    return routeHref({ kind: 'fight', id: details.fight_id, runId: alert.run_id ?? undefined });
  }
  if (!validId(alert.run_id)) return null;
  if (alert.kind === 'death') return routeHref({ kind: 'run', id: alert.run_id, point: 'death' });
  if (alert.kind === 'white_bag') return routeHref({ kind: 'run', id: alert.run_id });
  return null;
}

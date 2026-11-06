import type { DamageRow } from '../home/v3/types';
import { routeHref } from '../navigation/routes';

export function runPlayerHref(runId: string, row: DamageRow): string | null {
  return row.player_id ? routeHref({ kind: 'run-player', runId, playerId: row.player_id }) : null;
}

export function fightPlayerHref(fight: { id: string; players: { object_id: number }[] }, row: DamageRow, runId?: string): string | null {
  if (!row.player_id || row.object_id === null
    || fight.players.filter(player => player.object_id === row.object_id).length !== 1) return null;
  return routeHref({ kind: 'player', fightId: fight.id, objectId: row.object_id, runId });
}

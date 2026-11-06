import type { DropView } from '../../bindings/DropView';
import type { FightView } from '../../bindings/FightView';
import { knownNumber } from '../evidence';
import type { RealmKillRow } from '../realm';

export interface EventRow {
  key: string; targetType: number | null; name: string; at: string | null;
  state: 'live' | 'killed' | 'announced'; dps: number | null; rank: number | null; players: number | null; share: number | null;
  bags: Array<{ dropId: string; bagType: number; estimated: boolean }>;
}

const timestamp = (at: string | null) => at !== null && /^\d+$/.test(at) ? BigInt(at) : null;

/** Names are resolved from catalog visuals by the screen, never from history labels. */
export function eventRows(kills: RealmKillRow[], liveFight: FightView | null, drops: DropView[]): EventRow[] {
  const rows = kills.map<EventRow>(kill => {
    const announced = kill.evidence === 'realm_announcement';
    const self = announced ? null : kill.self;
    return { key: kill.id, targetType: kill.targetType, name: 'Unknown target', at: kill.at,
      state: announced ? 'announced' : 'killed', dps: knownNumber(self?.dps), rank: knownNumber(self?.rank),
      players: null, share: self?.share ?? null, bags: [] };
  }).sort((a, b) => {
    const left = timestamp(a.at) ?? -1n, right = timestamp(b.at) ?? -1n;
    return left > right ? -1 : left < right ? 1 : a.key.localeCompare(b.key);
  });
  for (const drop of drops) {
    const at = timestamp(drop.at);
    if (drop.source_enemy === null || at === null) continue;
    // Newest-first order also makes equal-time ties deterministic.
    const kill = rows.find(row => row.targetType === drop.source_enemy
      && timestamp(row.at) !== null && timestamp(row.at)! <= at);
    kill?.bags.push({ dropId: drop.id, bagType: drop.bag_type, estimated: drop.source_estimated });
  }
  if (liveFight && !kills.some(kill => kill.fightId === liveFight.id)) {
    const locals = liveFight.players.filter(player => player.is_local), self = locals.length === 1 ? locals[0] : null;
    rows.unshift({ key: `fight:${liveFight.id}`, targetType: liveFight.target_type ?? liveFight.boss_type,
      name: 'Unknown target', at: null, state: 'live', dps: knownNumber(self?.dps), rank: knownNumber(self?.rank),
      players: liveFight.players.length || null, share: self?.share ?? null, bags: [] });
  }
  return rows;
}

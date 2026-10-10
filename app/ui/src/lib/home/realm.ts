import { createDropScope, emptyDrops, uniqueDrops } from './drop-data';
import type { FightView } from '../bindings/FightView';
import type { KillEvidence } from '../bindings/KillEvidence';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import { focusClock, isRealmFightActive, snapshotFight, verifiedSelf } from '../shell/rail';
import { knownNumber } from './evidence';
import type { HomeLiveWindow, RealmKillSummary } from './v2/contracts';
import { emptyHomeLiveWindow } from './v2/live';
import { inRealmVisit, realmVisitElapsed, realmVisitFame, realmVisitPeriod } from './v2/visit';

export interface RealmKillRow {
  id: string;
  targetType: number | null;
  bossKey: string | null;
  at: string;
  evidence: KillEvidence;
  provisional: boolean;
  self: PlayerDamage | null;
  partial: boolean;
  killer: string | null;
  fightId: string | null;
  started: string | null;
  ended: string | null;
}

export const realmKillLabels: Record<KillEvidence, string> = {
  announced_death_removed: 'Death announcement and removal',
  hp_zero_removed: 'HP zero and removal',
  victory_quest_target: 'Victory on quest target',
  victory_sole_boss: 'Victory on sole boss',
  realm_announcement: 'Realm announcement',
};

const timestamp = (value: string | null) => value !== null && /^\d+$/.test(value) ? BigInt(value) : null;
const unique = <T extends { id: string }>(rows: T[]): T[] => [...new Map(rows.map(row => [row.id, row])).values()];

const scopeDrops = createDropScope();
const REALM_KILL_MERGE_US = 10_000_000n;
function mergeKillAnnouncements(fought: RealmKillRow[], announced: RealmKillRow[], window: HomeLiveWindow): RealmKillRow[] {
  const distance = (a: RealmKillRow, b: RealmKillRow): bigint | null => {
    if (a.targetType === null || a.targetType !== b.targetType) return null;
    // A short trip can fit inside the matching tolerance; separate entries are
    // still distinct observations even when they belong to the same realm visit.
    if (realmVisitPeriod(window, a.at) !== realmVisitPeriod(window, b.at)) return null;
    const left = timestamp(a.at), right = timestamp(b.at);
    if (left === null || right === null) return null;
    const delta = left >= right ? left - right : right - left;
    return delta <= REALM_KILL_MERGE_US ? delta : null;
  };
  const nearest = (row: RealmKillRow, candidates: RealmKillRow[]): RealmKillRow | null => {
    let selected: RealmKillRow | null = null, best: bigint | null = null, tied = false;
    for (const candidate of candidates) {
      const delta = distance(row, candidate);
      if (delta === null) continue;
      if (best === null || delta < best) { selected = candidate; best = delta; tied = false; }
      else if (delta === best) tied = true;
    }
    return tied ? null : selected;
  };
  const consumed = new Set<string>();
  const merged = fought.map(row => {
    const match = nearest(row, announced);
    if (!match || nearest(match, fought)?.id !== row.id) return row;
    consumed.add(match.id);
    return { ...row, bossKey: match.bossKey, killer: match.killer };
  });
  return [...merged, ...announced.filter(row => !consumed.has(row.id))];
}

/** Snapshot metrics override retained visit rows; both obey the current scope. */
export function realmView(snapshot: LiveSnapshot | null, window: HomeLiveWindow = emptyHomeLiveWindow(), previousId: string | null = null) {
  const realm = snapshot?.area_kind === 'realm' ? snapshot : null;
  const visitAvailable = realm !== null && window.streamId === realm.stream_id
    && window.visitKey !== null && window.visitStarted !== null && window.runId === null;
  const sessionId = realm?.session?.id;
  const scoped = (row: { session_id: string; run_id?: string | null }, at: string) =>
    visitAvailable && row.session_id === sessionId && row.run_id == null && inRealmVisit(window, at);
  const fight = snapshotFight(realm, previousId, window);
  // A retained fight is shown after it stops being the active engagement.
  const fightStatus = fight === null ? null : fight.ended !== null || fight.kill ? 'ended' as const
    : isRealmFightActive(fight, focusClock(realm, window)) ? 'live' as const : 'idle' as const;
  const self = verifiedSelf(realm?.character ?? null, fight);
  const allPlayers = [...(fight?.players ?? [])].sort((a, b) =>
    (knownNumber(b.damage) ?? -1) - (knownNumber(a.damage) ?? -1) || a.player_id.localeCompare(b.player_id));
  const players = allPlayers.slice(0, 5);
  if (self && !players.some(row => row.player_id === self.player_id)) players.splice(4, 1, self);
  const fought: RealmKillRow[] = [], announced: RealmKillRow[] = [];
  for (const row of unique<FightView | RealmKillSummary>(window.realmKills ?? window.fights)) {
    if (!scoped(row, row.started) || !['boss', 'event_boss', 'miniboss', 'quest'].includes(row.target_kind ?? '') || !row.kill
      || row.kill.evidence === 'realm_announcement' || !inRealmVisit(window, row.kill.at)) continue;
    fought.push({ id: `fight:${row.id}`, targetType: row.target_type, bossKey: null, at: row.kill.at,
      evidence: row.kill.evidence, provisional: row.kill.provisional,
      self: 'self' in row
        ? realm?.character?.identity_verified && row.self?.player_id === realm.character.id ? row.self : null
        : verifiedSelf(realm?.character ?? null, row),
      partial: 'partial' in row ? row.partial : fightPartial(row),
      killer: null, fightId: row.id, started: row.started, ended: row.ended });
  }
  for (const row of unique(window.realmAnnouncements ?? window.events)) {
    if (!scoped(row, row.at) || row.kind.type !== 'realm_event_killed') continue;
    announced.push({ id: `announcement:${row.id}`, targetType: row.kind.object_type ?? null,
      bossKey: row.kind.boss_key, at: row.at, evidence: 'realm_announcement', provisional: true,
      self: null, partial: true, killer: row.kind.killer ?? null, fightId: null, started: null, ended: null });
  }
  const kills = mergeKillAnnouncements(fought, announced, window);
  kills.sort((a, b) => {
    const left = timestamp(a.at)!, right = timestamp(b.at)!;
    return left > right ? -1 : left < right ? 1 : a.id.localeCompare(b.id);
  });
  const score = realm?.instance?.realm_score;
  const scoreKnown = score != null && knownNumber(score.current) !== null
    && Number.isFinite(score.max) && score.max > 0;
  const ratio = scoreKnown ? score.current / score.max : null;
  const percent = ratio !== null && Number.isFinite(ratio * 100) ? ratio * 100 : null;
  const visitFame = window.realmPeriods !== undefined ? realmVisitFame(window) : knownNumber(realm?.instance?.visit_fame_delta);
  return {
    realm, fight, self, players, omittedPlayers: allPlayers.length - players.length,
    damageMax: Math.max(0, ...players.map(row => knownNumber(row.damage) ?? 0)),
    fightStatus,
    fightPartial: fight !== null && fightPartial(fight),
    visitFame,
    visitElapsed: visitAvailable ? realmVisitElapsed(window, realm!.at) : null,
    hasAwayTime: (window.realmPeriods?.length ?? 0) > 1,
    visitAvailable, visitKey: visitAvailable ? window.visitKey : null,
    visitStarted: visitAvailable ? window.visitStarted : null,
    drops: visitAvailable ? scopeDrops(uniqueDrops(window.drops),
      JSON.stringify([sessionId, window.visitStarted, window.realmPeriods?.map(({ started, ended }) => [started, ended])]),
      row => scoped(row, row.at)) : emptyDrops, kills,
    partial: !visitAvailable || window.partial,
    reasons: visitAvailable ? window.reasons : ['Visit observations unavailable until the next observed entry'],
    score: scoreKnown ? score : null, percent,
    scoreFill: ratio === null ? null : Math.min(1, ratio) * 100,
  };
}

function fightPartial(fight: FightView): boolean {
  return !fight.complete_capture || fight.issues.length > 0
    || fight.players.some(row => row.sources.some(source => source.estimated));
}

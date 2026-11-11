import { objectVisual } from '../bridge/visual-index';
import { inRealmVisit } from '../home/v2/visit';
import type { CaptureState } from '../bindings/CaptureState';
import type { CharacterView } from '../bindings/CharacterView';
import type { FightView } from '../bindings/FightView';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { RunPlayerTotal } from '../bindings/RunPlayerTotal';
import type { ObjectVisual } from '../bindings/ObjectVisual';
import type { HomeLiveWindow } from '../home/v2/contracts';
import { formatElapsed } from '../styles/format';

export const captureLabels: Record<CaptureState, string> = {
  Stopped: 'Stopped', NpcapMissing: 'Npcap Missing', NpcapIncompatible: 'Npcap Incompatible',
  PermissionDenied: 'Permission Denied', WaitingForGame: 'Waiting for Game', Live: 'Live', Recovering: 'Recovering', Failed: 'Failed',
};
export const captureGuidance: Record<CaptureState, string> = {
  Stopped: 'Start capture, then enter a realm.',
  NpcapMissing: 'Install Npcap separately, then retry capture.',
  NpcapIncompatible: 'Repair or update Npcap, then retry capture.',
  PermissionDenied: 'Npcap is restricting access. Repair its installation to allow capture without administrator access.',
  WaitingForGame: 'Enter a realm. Live begins when game traffic is decoded.',
  Live: 'Decoded game traffic is available.',
  Recovering: 'Capture is recovering. You can stop and retry.',
  Failed: 'Capture failed. Check Npcap and your capture adapter, then retry.',
};
const time = (value: string) => /^\d+$/.test(value) ? BigInt(value) : 0n;
export function isBossFight(fight: FightView): boolean {
  return fight.target_type != null && fight.target_kind != null && fight.target_kind !== 'ordinary';
}
const tier = (fight: FightView) => fight.target_kind === 'boss' || fight.target_kind === 'event_boss' ? 3
  : fight.target_kind === 'miniboss' ? 2 : 1;
// Microseconds, matching snapshot.at and the engine hit timestamps.
export const REALM_FOCUS_IDLE_US = 10_000_000n;
type FocusContext = { area: LiveSnapshot['area_kind']; at: string; character: CharacterView | null };
export function selectBossFight(fights: FightView[], previousId: string | null = null, context?: FocusContext): FightView | null {
  if (context?.area === 'realm') return selectRealmFight(fights, previousId, context);
  const candidates = fights.filter(isBossFight).filter(fight => context?.area !== 'dungeon'
    || fight.ended === null || (fight.kill && !fight.kill.provisional));
  const previous = candidates.find(fight => fight.id === previousId);
  if (previous?.ended === null) return previous;
  const latest = (rows: FightView[], prioritizeTier: boolean) => rows.reduce<FightView | null>((selected, fight) => {
    if (!selected) return fight;
    const priority = prioritizeTier ? tier(fight) - tier(selected) : 0;
    const a = time(prioritizeTier ? fight.last_hit ?? fight.started : fight.ended ?? fight.started);
    const b = time(prioritizeTier ? selected.last_hit ?? selected.started : selected.ended ?? selected.started);
    return priority > 0 || (priority === 0 && (a > b || (a === b && fight.id < selected.id))) ? fight : selected;
  }, null);
  const killed = context?.area === 'dungeon' ? latest(candidates.filter(fight => fight.kill && !fight.kill.provisional), false) : null;
  const active = candidates.filter(fight => fight.ended === null
    && (!killed || tier(fight) > tier(killed) || time(fight.last_hit ?? fight.started) >= time(killed.kill!.at)));
  return latest(active, true)
    ?? killed ?? previous ?? latest(candidates, false);
}
const recentAt = (at: string | null | undefined, now: bigint) =>
  at != null && time(at) <= now && now - time(at) < REALM_FOCUS_IDLE_US;
/** Open, unkilled and hit by anyone within the idle window at `at`. */
export function isRealmFightActive(fight: FightView, at: string): boolean {
  return fight.ended === null && !fight.kill && recentAt(fight.last_hit, time(at));
}
function selectRealmFight(fights: FightView[], previousId: string | null, context: FocusContext): FightView | null {
  const now = time(context.at);
  const recent = (at: string | null | undefined) => recentAt(at, now);
  const candidates = fights.filter(isBossFight);
  const active = candidates.filter(fight => isRealmFightActive(fight, context.at));
  const latest = (rows: FightView[], at: (fight: FightView) => string, prioritizeTier = false) =>
    rows.reduce<FightView | null>((selected, fight) => {
      if (!selected) return fight;
      const priority = prioritizeTier ? tier(fight) - tier(selected) : 0;
      const a = time(at(fight)), b = time(at(selected));
      return priority > 0 || (priority === 0 && (a > b || (a === b && fight.id < selected.id))) ? fight : selected;
    }, null);
  // A historical local total is not an active engagement. Other players cannot
  // refresh this deadline, and tier cannot outrank a newer verified local hit.
  const engaged = active.filter(fight => verifiedSelf(context.character, fight) && recent(fight.local_last_hit));
  const local = latest(engaged, fight => fight.local_last_hit!);
  if (local) return local;
  const quest = latest(active.filter(fight => fight.is_current_quest), fight => fight.last_hit!);
  if (quest) return quest;
  const previous = active.find(fight => fight.id === previousId);
  if (previous) return previous; // Stable while bystanders alternate hits.
  // With nothing active, keep showing the last focused encounter (killed or
  // idle) until another boss-tier target engages, rather than blanking it.
  return latest(active, fight => fight.last_hit!, true)
    ?? candidates.find(fight => fight.id === previousId)
    ?? latest(candidates.filter(fight => fight.ended !== null), fight => fight.ended!);
}
const scopedCache = new WeakMap<LiveSnapshot, WeakMap<object, FightView[]>>();
const noWindow = {}, noFights: FightView[] = [];
/** Borrowed immutable result; callers that sort must copy first. */
// Published snapshots and windows are immutable; new content has a new identity.
export function scopedFights(snapshot: LiveSnapshot | null, window: HomeLiveWindow | null = null): FightView[] {
  if (!snapshot) return noFights;
  let cache = scopedCache.get(snapshot);
  if (!cache) { cache = new WeakMap(); scopedCache.set(snapshot, cache); }
  const key = window ?? noWindow, cached = cache.get(key);
  if (cached) return cached;
  const matched = window?.streamId === snapshot.stream_id && window.runId === (snapshot.run?.id ?? null);
  const visit = matched ? window?.visitStarted : null;
  const merged = new Map(snapshot.fights.map(row => [row.id, row]));
  for (const row of matched ? window!.fights : []) {
    // Event updates can arrive before the next snapshot, including quest-only
    // changes with an unchanged last_hit. Sequence, not hit time, owns freshness.
    const sequence = window?.fightSequences?.[row.id];
    const full = merged.get(row.id);
    if (!full) merged.set(row.id, row);
    else if (sequence != null && time(sequence) > time(snapshot.sequence)) {
      // Window rows are bounded to five players, so keep the full snapshot row
      // for display and take only the fresher focus and lifecycle fields.
      merged.set(row.id, { ...full, last_hit: row.last_hit, local_last_hit: row.local_last_hit,
        is_current_quest: row.is_current_quest, kill: row.kill, ended: row.ended, end_reason: row.end_reason });
    }
  }
  const rows = [...merged.values()];
  const result = rows.filter(fight => snapshot.area_kind === 'dungeon'
    ? snapshot.run !== null && fight.run_id === snapshot.run.id && fight.session_id === snapshot.run.session_id
    : snapshot.area_kind === 'realm' && fight.run_id === null && fight.session_id === snapshot.session?.id
      && (visit == null || inRealmVisit(window!, fight.started)));
  cache.set(key, result);
  return result;
}
/** The newest capture time known to the shared window or the snapshot. */
export function focusClock(snapshot: LiveSnapshot | null, window: HomeLiveWindow | null = null): string {
  return window?.streamId === snapshot?.stream_id && window?.runId === (snapshot?.run?.id ?? null)
    && time(window?.observedAt ?? '0') > time(snapshot?.at ?? '0') ? window!.observedAt! : snapshot?.at ?? '0';
}
export function snapshotFight(snapshot: LiveSnapshot | null, previousId: string | null = null, window: HomeLiveWindow | null = null): FightView | null {
  if (snapshot?.area_kind === 'dungeon' && snapshot.run?.ended != null) return null;
  const fights = scopedFights(snapshot, window);
  const observedAt = focusClock(snapshot, window);
  return selectBossFight(fights, previousId, snapshot ? { area: snapshot.area_kind, at: observedAt, character: snapshot.character } : undefined);
}
export function verifiedSelf(character: CharacterView | null, fight: FightView | null) {
  const candidates = fight?.players.filter((player) => player.is_local) ?? [];
  return character?.identity_verified && candidates.length === 1 && candidates[0].player_id === character.id ? candidates[0] : null;
}
export function verifiedRunSelf(snapshot: LiveSnapshot | null): RunPlayerTotal | null {
  const totals = snapshot?.run_totals, character = snapshot?.character;
  if (!snapshot?.run || !character?.identity_verified || snapshot.run.character_id !== character.id || !totals) return null;
  const candidates = totals.players.filter(row => row.is_local);
  if (candidates.length !== 1 || candidates[0].player_id !== character.id) return null;
  // The local projection must agree with the unique full-set row before it is used.
  if (totals.local && (!totals.local.is_local || totals.local.player_id !== character.id)) return null;
  return totals.local ?? candidates[0];
}

/** Character projection: identity, class and equipment only. It never selects a fight. */
export function characterRailView(snapshot: LiveSnapshot | null, visuals: ObjectVisual[] = [], stale = false) {
  const character = snapshot?.character ?? null;
  const classVisual = objectVisual(visuals, character?.class_id);
  return {
    stale: stale && Boolean(character || snapshot?.map || snapshot?.run),
    character, snapshot, visuals,
    name: character?.name ?? (character ? 'Character' : 'No Character'),
    className: classVisual?.name ?? 'Class',
    sprite: classVisual?.sprite ?? null,
    equipment: ['Weapon', 'Ability', 'Armor', 'Ring'].map((label, slot) => {
      const equipped = character?.equipment.find((item) => item.slot === slot);
      const itemType = equipped?.item_type ?? null;
      const visual = itemType === null ? undefined : objectVisual(visuals, itemType);
      const enchant = itemType === null ? null : equipped?.enchant ?? null;
      return { slot, label, itemType, name: visual?.name ?? null, sprite: visual?.sprite ?? null, enchant };
    }),
    provisional: character !== null && !character.identity_verified,
    died: character?.died_at !== null && character?.died_at !== undefined,
  };
}
export type CharacterRailView = ReturnType<typeof characterRailView>;

/** Focus projection: the one place the shared boss/quest/local-hit fight is selected for the rail. */
export function focusRailView(base: CharacterRailView, previousId: string | null = null, window: HomeLiveWindow | null = null) {
  const { snapshot, visuals, character } = base;
  const fight = snapshotFight(snapshot, previousId, window);
  const wholeRun = snapshot?.area_kind === 'dungeon' && snapshot.run !== null && fight === null;
  const local = wholeRun ? verifiedRunSelf(snapshot) : verifiedSelf(character, fight);
  const targetType = fight?.target_type ?? fight?.boss_type;
  return {
    stale: base.stale,
    snapshot,
    map: snapshot?.map ?? 'Location',
    fight, wholeRun,
    target: fight ? objectVisual(visuals, targetType)?.name ?? 'Target' : wholeRun ? 'Dungeon Total' : 'No Boss',
    dps: local?.dps ?? null,
    rank: local?.rank ?? null,
    elapsed: snapshot?.run ? formatElapsed(snapshot.run.started, snapshot.run.ended ?? snapshot.at) : null,
  };
}
export type FocusRailView = ReturnType<typeof focusRailView>;

/** Both projections merged, for callers and tests that want the whole rail. */
export function railView(snapshot: LiveSnapshot | null, visuals: ObjectVisual[] = [], stale = false, previousId: string | null = null, window: HomeLiveWindow | null = null) {
  const character = characterRailView(snapshot, visuals, stale);
  return { ...character, ...focusRailView(character, previousId, window) };
}
export type RailView = ReturnType<typeof railView>;

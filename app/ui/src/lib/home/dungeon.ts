import { createDropScope, emptyDrops, uniqueDrops } from './drop-data';
import type { DropView } from '../bindings/DropView';
import type { FightView } from '../bindings/FightView';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { InstanceEventView } from '../bindings/InstanceEventView';
import type { RunPlayerTotal } from '../bindings/RunPlayerTotal';
import type { HomeLiveWindow } from './v2/contracts';
import { coverageLabels, damageUncertain, knownNumber } from './evidence';
import { scopedFights, snapshotFight, verifiedSelf, verifiedRunSelf } from '../shell/rail';
import { formatClock, formatElapsed } from '../styles/format';

export interface DungeonView {
  fight: FightView | null;
  self: DungeonPlayer | null;
  wholeRun: boolean;
  observedPlayers: number | null;
  personalDeaths: number | null;
  drops: DropView[];
  partial: boolean;
}

const timestamp = (value: string): bigint | null => /^\d+$/.test(value) ? BigInt(value) : null;
const byTime = (a: string, b: string) => {
  const left = timestamp(a), right = timestamp(b);
  return left === right ? 0 : left === null ? -1 : right === null ? 1 : left < right ? -1 : 1;
};
const byId = (a: string, b: string) => a < b ? -1 : a > b ? 1 : 0;
const unique = <T extends { id: string }>(rows: T[]): T[] => [...new Map(rows.map(row => [row.id, row])).values()];

const observationDrops = createDropScope();
const viewDrops = createDropScope();

/** Departures at or after completion are exits from a finished run. */
export function dungeonEvents(events: InstanceEventView[], completedAt: string | null | undefined): InstanceEventView[] {
  return events.filter(event => event.kind.type !== 'player_left' || completedAt == null || BigInt(event.at) < BigInt(completedAt));
}

/** Snapshot metrics stay complete; newer event lifecycle fields take precedence. */
export function dungeonObservations(snapshot: LiveSnapshot | null, window: HomeLiveWindow | null) {
  const run = snapshot?.run;
  const matched = Boolean(run && window?.runId === run.id && window.streamId === snapshot?.stream_id);
  const scoped = <T extends { run_id?: string | null; session_id: string }>(rows: T[]) =>
    run ? rows.filter(row => row.run_id === run.id && row.session_id === run.session_id) : [];
  const fights = [...scopedFights(snapshot, window)]
    .sort((a, b) => byTime(b.started, a.started) || byId(a.id, b.id));
  const drops = run ? uniqueDrops(observationDrops(matched ? window!.drops : snapshot?.drops ?? emptyDrops,
    JSON.stringify([run.id, run.session_id]), row => row.run_id === run.id && row.session_id === run.session_id)) : emptyDrops;
  const events = dungeonEvents(unique(scoped(matched ? window!.events : [])), run?.completed_at);
  const reasons = [...new Set([
    ...(!matched ? ['Run event window unavailable; earlier observations may be missing.'] : window!.reasons),
    ...(matched && !window!.visitKey ? ['Run entry was not observed.'] : []),
    ...((snapshot?.instance?.events_dropped ?? 0) > 0 ? ['Some instance events were omitted.'] : []),
    ...[...(snapshot?.issues ?? []), ...(run?.issues ?? [])].map(issue => coverageLabels[issue]),
    ...(run && !run.complete_capture ? ['Run capture is incomplete.'] : []),
  ])];
  const partial = !matched || !window!.visitKey || window!.partial || reasons.length > 0;
  const count = (type: 'player_left' | 'player_death') => {
    const rows = events.filter(event => event.kind.type === type);
    const names = rows.sort((a, b) => byTime(a.at, b.at) || byId(a.id, b.id))
      .map(row => row.kind.type === 'player_death' || row.kind.type === 'player_left' ? row.kind.player_name ?? 'Player' : 'Player');
    return { value: rows.length > 0 || !partial ? rows.length : null, provisional: rows.some(row => row.provisional), names };
  };
  return { fights, drops, events, partial, reasons, left: count('player_left'), deaths: count('player_death') };
}

/** Header name lists keep notification order; repeated names collapse with a count. */
export function dungeonNames(names: string[]): string[] {
  const counts = new Map<string, number>();
  for (const name of names) counts.set(name, (counts.get(name) ?? 0) + 1);
  return [...counts].map(([name, count]) => count > 1 ? `${name} \u00d7${count}` : name);
}

export type DungeonPlayer = PlayerDamage | RunPlayerTotal;

/** Never derive rank/share from the visible subset. A distant self replaces row five. */
export function dungeonPlayers<T extends DungeonPlayer>(players: T[], self: T | null, page = 0) {
  const sorted = [...players].sort((a, b) => (knownNumber(b.damage) ?? -1) - (knownNumber(a.damage) ?? -1)
    || byId(a.player_id, b.player_id));
  if (self && sorted.findIndex(row => row.player_id === self.player_id) >= 5) {
    sorted.splice(sorted.findIndex(row => row.player_id === self.player_id), 1);
    sorted.splice(4, 0, self);
  }
  const pages = Math.max(1, Math.ceil(sorted.length / 5));
  const index = Math.min(Math.max(0, page), pages - 1);
  const rows = sorted.slice(index * 5, index * 5 + 5);
  return { rows, page: index, pages, omitted: Math.max(0, sorted.length - rows.length) };
}

export const dungeonRunSelf = verifiedRunSelf;

export function dungeonTabs(fights: FightView[], current: FightView | null, selected: FightView | null, page: number) {
  const pinned = unique([...(current ? [current] : []), ...(selected ? [selected] : [])]);
  const others = fights.filter(fight => !pinned.some(row => row.id === fight.id));
  const size = Math.max(1, 3 - pinned.length);
  const pages = Math.max(1, Math.ceil(others.length / size));
  const index = Math.min(Math.max(0, page), pages - 1);
  return { rows: [...pinned, ...others.slice(index * size, index * size + size)], pages, page: index };
}

export function dungeonClock(at: string | undefined): string | null {
  return formatClock(at, [], { hour: '2-digit' });
}

export type DungeonMarkerDetail =
  | { type: 'boss'; objectType: number | null; outcome: 'live' | 'killed' | 'ended' }
  | { type: 'bag'; bagType: number; bagRank: number | null; contentsKnown: boolean; items: { itemType: number; quantity: number }[] }
  | { type: 'death'; name: string; level: number | null; killer: string | null; classId: number | null; graveType: number | null; local: boolean }
  | { type: 'left'; name: string | null; classId: number | null }
  | { type: 'portal'; dungeon: string | null; opener: string | null; portalType: number | null }
  | { type: 'realm_kill'; objectType: number | null; killer: string | null };

export interface DungeonMarker {
  id: string;
  at: string;
  until: string | null;
  label: string;
  objectType: number | null;
  kind: 'enemy' | 'bag' | null;
  event: 'death' | 'left' | 'portal' | 'realm_kill' | null;
  provisional: boolean;
  detail: DungeonMarkerDetail;
}

// Markers reflect notifications; departures do not establish a cause.
function eventMarker(event: InstanceEventView): DungeonMarker {
  const kind = event.kind;
  const label = kind.type === 'player_left' ? `${kind.player_name ?? 'Player'} Departure`
    : kind.type === 'player_death' ? `${kind.player_name} Death`
    : kind.type === 'portal_opened' ? `Portal: ${kind.dungeon ?? '—'}`
    : 'Realm Kill';
  const discriminator = { player_death: 'death', player_left: 'left', portal_opened: 'portal', realm_event_killed: 'realm_kill' } as const;
  const detail: DungeonMarkerDetail = kind.type === 'player_death'
    ? { type: 'death', name: kind.player_name, level: kind.level ?? null, killer: kind.killed_by ?? null,
      classId: kind.class_id ?? null, graveType: kind.grave_type ?? null, local: kind.is_local }
    : kind.type === 'player_left' ? { type: 'left', name: kind.player_name ?? null, classId: kind.class_id ?? null }
    : kind.type === 'portal_opened' ? { type: 'portal', dungeon: kind.dungeon ?? null, opener: kind.opener ?? null, portalType: kind.portal_type ?? null }
    : { type: 'realm_kill', objectType: kind.object_type ?? null, killer: kind.killer ?? null };
  const objectType = detail.type === 'death' ? detail.graveType : detail.type === 'portal' ? detail.portalType
    : detail.type === 'realm_kill' ? detail.objectType : null;
  return { id: `event:${event.id}`, at: event.at, until: null, label, objectType, kind: null,
    event: discriminator[kind.type], provisional: event.provisional, detail };
}

export function bossOutcome(fight: FightView): 'live' | 'killed' | 'ended' {
  return (fight.kill && !fight.kill.provisional) || fight.end_reason === 'killed' ? 'killed' : fight.ended === null ? 'live' : 'ended';
}

function bagItems(drop: DropView): { itemType: number; quantity: number }[] {
  if (!drop.contents_known) return [];
  return drop.contents.length > 0
    ? [...drop.contents].sort((a, b) => a.slot - b.slot).map(item => ({ itemType: item.item_type, quantity: item.quantity }))
    : drop.items.map(itemType => ({ itemType, quantity: 1 }));
}

/** Boss-tier fights become spans; ordinary encounters stay off the run track. */
export function dungeonTimeline(fights: FightView[], drops: DropView[], events: InstanceEventView[]): DungeonMarker[] {
  return [
    ...unique(fights).filter(fight => fight.target_type != null && (fight.target_kind === 'boss' || fight.target_kind === 'event_boss')).map((fight): DungeonMarker => {
      const outcome = bossOutcome(fight);
      return { id: `fight:${fight.id}`, at: fight.started, until: fight.kill && !fight.kill.provisional ? fight.kill.at : fight.ended,
        label: outcome === 'live' ? 'Active Encounter' : 'Ended Encounter', objectType: fight.target_type ?? fight.boss_type,
        kind: 'enemy', event: null, provisional: !fight.complete_capture || fight.issues.length > 0,
        detail: { type: 'boss', objectType: fight.target_type ?? fight.boss_type, outcome } };
    }),
    ...unique(drops).map((drop): DungeonMarker => ({ id: `drop:${drop.id}`, at: drop.at, until: null,
      label: 'Bag', objectType: drop.bag_type, kind: 'bag', event: null, provisional: drop.source_estimated || drop.issues.length > 0,
      detail: { type: 'bag', bagType: drop.bag_type, bagRank: drop.bag_rank, contentsKnown: drop.contents_known, items: bagItems(drop) } })),
    ...unique(events).map(eventMarker),
  ].sort((a, b) => byTime(a.at, b.at) || byId(a.id, b.id));
}

/** About 20px, matched to the 24px point target so merged targets never overlap. */
export const MARKER_MERGE_PX = 24;

export interface DungeonCluster { id: string; x: number; markers: DungeonMarker[] }

/** Chains points closer than the threshold, so neighbouring cluster centres stay at least that far apart. */
export function dungeonClusters(points: { marker: DungeonMarker; x: number }[], threshold = MARKER_MERGE_PX): DungeonCluster[] {
  const sorted = points.filter(point => Number.isFinite(point.x))
    .sort((a, b) => a.x - b.x || byTime(a.marker.at, b.marker.at) || byId(a.marker.id, b.marker.id));
  const groups: { first: number; last: number; markers: DungeonMarker[] }[] = [];
  for (const point of sorted) {
    const group = groups.at(-1);
    if (group && point.x - group.last < threshold) { group.last = point.x; group.markers.push(point.marker); }
    else groups.push({ first: point.x, last: point.x, markers: [point.marker] });
  }
  return groups.map(({ first, last, markers }) => {
    markers.sort((a, b) => byTime(a.at, b.at) || byId(a.id, b.id));
    return { id: markers.length === 1 ? markers[0].id : `cluster:${markers[0].id}`, x: (first + last) / 2, markers };
  });
}

export interface MarkerContext { start: string; end: string; name: (objectType: number | null) => string | null }
export interface MarkerSummary { title: string; lines: string[] }

const counted = (name: string, quantity: number) => quantity > 1 ? `${name} ×${quantity}` : name;

/** Tooltip text from notification detail. Missing fields read as a dash, never as zero. */
export function markerSummary(marker: DungeonMarker, context: MarkerContext): MarkerSummary {
  const at = formatElapsed(context.start, marker.at);
  const detail = marker.detail;
  switch (detail.type) {
    case 'boss': {
      const outcome = detail.outcome === 'live' ? 'Live' : detail.outcome === 'killed' ? 'Killed' : 'Ended';
      return { title: `${context.name(detail.objectType) ?? 'Boss'} · ${at}`,
        lines: [`Duration ${formatElapsed(marker.at, marker.until ?? context.end)}`, outcome] };
    }
    case 'bag': {
      const items = detail.items.map(item => counted(context.name(item.itemType) ?? 'Unknown Item', item.quantity));
      return { title: `${context.name(detail.bagType) ?? 'Bag'} · ${at}`,
        lines: !detail.contentsKnown ? ['Contents —'] : items.length ? items : ['Empty'] };
    }
    case 'death':
      return { title: `${detail.name}${detail.local ? ' (You)' : ''} died · ${at}`,
        lines: [`Level ${detail.level ?? '—'}`, `Killed by ${detail.killer ?? '—'}`] };
    case 'left':
      return { title: `${detail.name ?? 'Player'} left · ${at}`, lines: [] };
    case 'portal':
      return { title: `Portal: ${detail.dungeon ?? context.name(detail.portalType) ?? '—'} · ${at}`,
        lines: detail.opener ? [`Opened by ${detail.opener}`] : [] };
    case 'realm_kill':
      return { title: `${context.name(detail.objectType) ?? 'Realm Boss'} killed · ${at}`,
        lines: detail.killer ? [`By ${detail.killer}`] : [] };
  }
}

/** Axis geometry is derived from typed timestamps; invalid/out-of-run times stay unavailable. */
export function dungeonPosition(at: string, start: string, end: string): number | null {
  const value = timestamp(at), from = timestamp(start), until = timestamp(end);
  if (value === null || from === null || until === null || until <= from || value < from || value > until) return null;
  return Number((value - from) * 10000n / (until - from)) / 100;
}

export function dungeonView(snapshot: LiveSnapshot | null, previousId: string | null = null, window: HomeLiveWindow | null = null): DungeonView {
  const run = snapshot?.run;
  const fight = run ? snapshotFight(snapshot, previousId, window) : null;
  const drops = run ? viewDrops(snapshot!.drops, run.id, value => value.run_id === run.id) : emptyDrops;
  const wholeRun = fight === null;
  const self = wholeRun ? dungeonRunSelf(snapshot) : verifiedSelf(snapshot?.character ?? null, fight);
  const partial = !snapshot || !run || !run.complete_capture || snapshot.issues.length > 0
    || (snapshot.character?.issues.length ?? 0) > 0
    || run.issues.length > 0 || Boolean(fight && (!fight.complete_capture || fight.issues.length > 0
      || fight.players.some((player) => player.sources.some(damageUncertain))))
    || drops.some((drop) => drop.issues.length > 0 || drop.source_estimated);
  // A run death is one local event, even when copied into multiple fight rows.
  const deathObserved = run && (run.died_at !== null || run.outcome === 'died'
    || run.evidence.some((value) => value.evidence === 'death_packet'));
  const completeRun = run?.complete_capture && run.issues.length === 0 && snapshot?.issues.length === 0
    && snapshot.character?.identity_verified && snapshot.character.id === run.character_id;
  return {
    fight, self, wholeRun, observedPlayers: (wholeRun ? snapshot?.run_totals?.players.length : fight?.players.length) || null,
    personalDeaths: deathObserved ? 1 : completeRun ? 0 : null, drops, partial,
  };
}

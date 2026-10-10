import type { Client } from '../bridge/client';
import type { DropView } from '../bindings/DropView';
import type { FightView } from '../bindings/FightView';
import type { CharacterView } from '../bindings/CharacterView';
import type { HomeLiveWindow } from './v2/contracts';
import type { HomeSummary } from '../bindings/HomeSummary';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { VisualPage } from '../bindings/VisualPage';
import { objectVisualTypes } from '../bridge/visuals';
import { dungeonView } from './dungeon';

export interface HomeVisuals extends VisualPage { phase: 'loading' | 'ready' | 'unavailable' }
export const emptyHomeVisuals = (): HomeVisuals => ({ client_fingerprint: '', objects: [], phase: 'unavailable' });

/** Also usable by the persistent rail without collecting unrelated Home rows. */
export function characterVisualTypes(character: CharacterView | null): number[] {
  return objectVisualTypes(character ? [character.class_id,
    ...character.equipment.filter(item => item.slot >= 0 && item.slot < 4).map(item => item.item_type)] : []);
}

const dropTypesCache = new WeakMap<DropView[], number[]>();
function dropVisualTypes(drops: DropView[]): number[] {
  const previous = dropTypesCache.get(drops);
  if (previous) return previous;
  const types = new Set<number | null>();
  for (const drop of drops) {
    types.add(drop.bag_type); types.add(drop.source_enemy);
    for (const item of drop.contents) types.add(item.item_type);
    if (drop.contents_known) for (const item of drop.items) types.add(item);
  }
  const result = objectVisualTypes([...types]);
  dropTypesCache.set(drops, result);
  return result;
}

const fightTypesCache = new WeakMap<FightView[], number[]>();
function fightVisualTypes(fights: FightView[]): number[] {
  const previous = fightTypesCache.get(fights);
  if (previous) return previous;
  const types: (number | null)[] = [];
  for (const value of fights) {
    types.push(value.target_type ?? value.boss_type);
    for (const player of value.players) {
      types.push(player.class_id);
      for (const source of player.sources) if (source.estimated) types.push(source.item_type);
    }
  }
  const result = objectVisualTypes(types);
  fightTypesCache.set(fights, result);
  return result;
}
const realmFightsCache = new WeakMap<FightView[], { session: string | undefined; rows: FightView[] }>();
function realmFights(snapshot: LiveSnapshot): FightView[] {
  const previous = realmFightsCache.get(snapshot.fights), session = snapshot.session?.id;
  if (previous && previous.session === session) return previous.rows;
  const rows = snapshot.fights.filter(value => value.run_id === null && value.session_id === session && value.ended === null);
  realmFightsCache.set(snapshot.fights, { session, rows });
  return rows;
}

/** Collect rendered/retained rows only; the optional window must already be scoped. */
export function homeVisualTypes(snapshot: LiveSnapshot | null, summary: HomeSummary | null, window?: HomeLiveWindow | null): number[] {
  const types: (number | null)[] = [];
  const drop = (value: DropView) => {
    types.push(value.bag_type, value.source_enemy);
    types.push(...value.contents.map(item => item.item_type));
    if (value.contents_known) types.push(...value.items);
  };
  const fight = (value: FightView) => {
    types.push(value.target_type ?? value.boss_type);
    for (const player of value.players) {
      types.push(player.class_id);
      for (const source of player.sources) if (source.estimated) types.push(source.item_type);
    }
  };
  if (snapshot) {
    types.push(...characterVisualTypes(snapshot.character));
    const view = dungeonView(snapshot);
    if (view.fight) fight(view.fight);
    if (snapshot.area_kind === 'realm') {
      types.push(1796);
      types.push(snapshot.instance?.quest_target?.object_type ?? null);
      types.push(...fightVisualTypes(realmFights(snapshot)));
    }
    for (const player of snapshot.run_totals?.players ?? []) types.push(player.class_id ?? null);
    types.push(...dropVisualTypes(view.drops));
  }
  if (window) {
    types.push(...fightVisualTypes(window.fights));
    types.push(...dropVisualTypes(window.drops));
    for (const row of window.realmKills ?? []) types.push(row.target_type);
    for (const row of window.realmAnnouncements ?? []) {
      if (row.kind.type === 'realm_event_killed') types.push(row.kind.object_type ?? null);
    }
    for (const event of window.events) {
      const kind = event.kind;
      if (kind.type === 'realm_event_killed') types.push(kind.object_type ?? null);
      else if (kind.type === 'portal_opened') types.push(kind.portal_type ?? null);
      else {
        types.push(kind.class_id ?? null);
        if (kind.type === 'player_death') types.push(kind.grave_type ?? null);
      }
    }
  }
  if (summary) {
    if (summary.top_fight) fight(summary.top_fight);
    if (summary.best_drop) drop(summary.best_drop);
    for (const run of summary.recent_runs.slice(0, 10)) {
      const matches = summary.recent_details.filter(detail => detail.run_id === run.id);
      if (matches.length === 1) matches[0].loot.slice(0, 3).forEach(drop);
    }
  }
  return objectVisualTypes(types);
}

export { createDungeonPortal } from '../bridge/dungeon-portal';

/** Reuses the shell Client's fingerprint-keyed cache. One bounded IPC at a time,
 * one latest desired key, and no per-row subscriptions or queries. */
export function createHomeVisuals(client: Pick<Client, 'getObjectVisuals' | 'onVisualsChanged'>, receive: (value: HomeVisuals) => void) {
  let types: number[] = [], key: string | null = null, revision = 0;
  let active = false, pending = false, disposed = false;
  async function pump() {
    if (active || disposed || !pending) return;
    active = true; pending = false;
    const request = revision, requested = types;
    const result: HomeVisuals = { ...emptyHomeVisuals(), phase: 'ready' };
    try {
      for (let offset = 0; offset < requested.length; offset += 128) {
        const page = await client.getObjectVisuals(requested.slice(offset, offset + 128));
        if (disposed || request !== revision) return;
        if (offset > 0 && result.client_fingerprint !== page.client_fingerprint) throw new Error('Catalog changed during lookup');
        result.client_fingerprint = page.client_fingerprint;
        result.objects.push(...page.objects);
      }
      if (!disposed && request === revision) receive(result);
    } catch {
      if (!disposed && request === revision) receive(emptyHomeVisuals());
    } finally {
      active = false;
      void pump();
    }
  }
  function refresh() {
    if (disposed) return;
    revision++; pending = true;
    receive({ ...emptyHomeVisuals(), phase: 'loading' });
    void pump();
  }
  const release = client.onVisualsChanged(refresh);
  return {
    update(next: number[]) {
      if (disposed) return;
      const normalized = objectVisualTypes(next), nextKey = normalized.join(',');
      if (nextKey === key) return;
      key = nextKey; types = normalized; refresh();
    },
    retry: refresh,
    dispose() { if (disposed) return; disposed = true; revision++; release(); },
  };
}

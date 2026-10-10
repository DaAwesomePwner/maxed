// Frozen 71352f6 app/ui/src/lib/home/v2/live.ts behavior oracle.
// Only import paths changed; preserve pre-batching bound/trim/focus semantics.
import type { CaptureStatus } from '../../src/lib/bindings/CaptureStatus';
import type { CharacterView } from '../../src/lib/bindings/CharacterView';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { InstanceEventView } from '../../src/lib/bindings/InstanceEventView';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { LiveState } from '../../src/lib/bridge/live';
import { scopedFights, selectBossFight, verifiedSelf } from '../../src/lib/shell/rail';
import { RECOVERY_GRACE_MS } from '../../src/lib/home/mode';
import type { HomeLiveWindow, RealmKillSummary } from '../../src/lib/home/v2/contracts';
import { inRealmVisit } from '../../src/lib/home/v2/visit';

export const emptyHomeLiveWindow = (): HomeLiveWindow => ({
  streamId: null, visitKey: null, visitStarted: null, runId: null,
  fights: [], drops: [], events: [], partial: true, reasons: ['Visit entry was not observed.'],
});
type Row<T> = { value: T; sequence: bigint };
const compare = (a: { at: string; id: string }, b: { at: string; id: string }) =>
  BigInt(a.at) < BigInt(b.at) ? -1 : BigInt(a.at) > BigInt(b.at) ? 1 : a.id < b.id ? -1 : a.id > b.id ? 1 : 0;

/** Presentation only. The shell feeds ordered callbacks, never a last-event effect. */
export function createHomeLiveWindow(receive: (window: HomeLiveWindow) => void) {
  let window = emptyHomeLiveWindow();
  const fights = new Map<string, Row<FightView>>();
  const kills = new Map<string, Row<RealmKillSummary>>();
  const announcements = new Map<string, Row<InstanceEventView>>();
  const drops = new Map<string, Row<DropView>>();
  const events = new Map<string, Row<InstanceEventView>>();
  type RealmVisit = { window: HomeLiveWindow; fights: Map<string, Row<FightView>>;
    kills: Map<string, Row<RealmKillSummary>>; announcements: Map<string, Row<InstanceEventView>>;
    drops: Map<string, Row<DropView>>; events: Map<string, Row<InstanceEventView>>; focused: string | null };
  const visits = new Map<string, RealmVisit>();
  let instanceKey: string | null = null, visitsTruncated = false;
  let fameEntry: number | null | undefined, fameSequence = -1n;
  let session: string | null = null, character: CharacterView | null = null;
  let verifiedCharacterId: string | null = null, establishingIdentity = false;
  let area: LiveSnapshot['area_kind'] = 'unknown';
  let entrySequence: bigint | null = null, snapshotSequence = -1n;
  let selected: string | null = null, disposed = false, paused = false;
  let focused: string | null = null;
  let observedAt = '0';
  let snapshotFights: FightView[] = [];
  let explicitSelection = false;
  let captureState: CaptureStatus['state'] | null = null;
  let phase: LiveState['phase'] | null = null;
  let recovery: ReturnType<typeof setTimeout> | undefined;
  const reason = (text: string) => {
    window = { ...window, partial: true,
      reasons: window.reasons.includes(text) ? window.reasons : [...window.reasons, text] };
  };
  function reset(message: string, streamId = window.streamId, keepVisits = false) {
    if (!keepVisits) { visits.clear(); visitsTruncated = false; }
    instanceKey = null; fameEntry = undefined; fameSequence = -1n;
    clearTimeout(recovery); recovery = undefined;
    fights.clear(); kills.clear(); announcements.clear(); drops.clear(); events.clear(); selected = null; focused = null; snapshotFights = []; observedAt = '0'; explicitSelection = false; entrySequence = null;
    if (!keepVisits) establishingIdentity = false;
    window = { ...emptyHomeLiveWindow(), streamId, reasons: [message] };
  }
  function observeCharacter(next: CharacterView | null, sequence = -1n, sameArea = true) {
    const switched = next?.identity_verified && verifiedCharacterId !== null && next.id !== verifiedCharacterId;
    if (switched || (!establishingIdentity && character?.id !== next?.id)) {
      reset('Character changed; visit entry was not observed.');
    }
    if (next && sameArea && area === 'realm' && window.realmPeriods?.length && sequence >= fameSequence) {
      const fame = next.fame != null && next.fame >= 0 ? next.fame : null;
      if (fameEntry === undefined) fameEntry = fame;
      updateFame(fame !== null && fameEntry !== null && fame >= fameEntry ? fame - fameEntry : null, sequence);
    }
    character = next;
    if (next?.identity_verified) {
      verifiedCharacterId = next.id;
      establishingIdentity = false;
    }
  }
  function updateFame(fame: number | null, sequence: bigint) {
    if (!window.realmPeriods?.length || sequence < fameSequence) return;
    fameSequence = sequence;
    window = { ...window, realmPeriods: window.realmPeriods.map((period, i, rows) =>
      i === rows.length - 1 ? { ...period, fame } : period) };
  }
  function leaveRealm(at: string) {
    if (area !== 'realm' || instanceKey === null) return;
    window = { ...window, realmPeriods: window.realmPeriods?.map(period => period.ended === null ? { ...period, ended: at } : period) };
    visits.set(instanceKey, { window, fights: new Map(fights), kills: new Map(kills), announcements: new Map(announcements), drops: new Map(drops), events: new Map(events), focused });
    if (visits.size > 8) { visits.delete(visits.keys().next().value!); visitsTruncated = true; }
  }
  function enterMap(envelope: EventEnvelope, map: Extract<EventEnvelope['event'], { map_entered: unknown }>['map_entered']) {
    leaveRealm(map.at);
    reset('', window.streamId, true); paused = false; area = map.kind;
    character = null; establishingIdentity = true;
    entrySequence = BigInt(envelope.sequence);
    instanceKey = area === 'realm' ? map.realm_instance_key ?? null : null;
    const saved = instanceKey === null ? undefined : visits.get(instanceKey);
    if (saved) {
      visits.delete(instanceKey!);
      window = saved.window; focused = saved.focused;
      for (const [id, row] of saved.fights) fights.set(id, row);
      for (const [id, row] of saved.kills) kills.set(id, row);
      for (const [id, row] of saved.announcements) announcements.set(id, row);
      for (const [id, row] of saved.drops) drops.set(id, row);
      for (const [id, row] of saved.events) events.set(id, row);
    } else {
      window = { ...window, visitKey: `${envelope.stream_id}:${envelope.sequence}`,
        visitStarted: map.at, partial: false, reasons: [] };
    }
    if (area === 'realm') {
      window = { ...window, realmPeriods: [...(window.realmPeriods ?? []), { started: map.at, ended: null, fame: null }] };
      if (instanceKey === null) reason('Realm instance key is missing; this entry is a partial visit.');
      if (!saved && visitsTruncated) reason('Earlier realm visits were omitted from the bounded session window.');
      if (window.realmPeriods!.length > 128) {
        window = { ...window, realmPeriods: window.realmPeriods!.slice(-128) };
        window = { ...window, visitStarted: window.realmPeriods![0].started };
        reason('Visit segments truncated; earlier observations were omitted.');
      }
    }
  }
  function emit() {
    if (disposed) return;
    // Sequence-only revisions do not invalidate immutable loot projections.
    let sameDrops = window.drops.length === drops.size, index = 0;
    if (sameDrops) for (const row of drops.values()) {
      if (window.drops[index++] !== row.value) { sameDrops = false; break; }
    }
    window = { ...window, focusedFightId: focused, observedAt,
      realmKills: [...kills.values()].map(row => row.value),
      realmAnnouncements: [...announcements.values()].map(row => row.value),
      fightSequences: Object.fromEntries([...fights].map(([id, row]) => [id, row.sequence.toString()])), fights: [...fights.values()].map(row => row.value),
      drops: sameDrops ? window.drops : Array.from(drops.values(), row => row.value), events: [...events.values()].map(row => row.value) };
    receive(window);
  }
  function put<T extends { id: string }>(rows: Map<string, Row<T>>, value: T, sequence: bigint) {
    if ((rows.get(value.id)?.sequence ?? -1n) <= sequence) rows.set(value.id, { value, sequence });
  }
  function trim<T extends { id: string }>(rows: Map<string, Row<T>>, cap: number,
    at: (value: T) => string, protectedIds: string[] = [],
    message = 'Observation window truncated; older rows were omitted.') {
    if (rows.size <= cap) return;
    const ordered = [...rows.values()].map(row => row.value)
      .sort((a, b) => compare({ at: at(a), id: a.id }, { at: at(b), id: b.id }));
    for (const row of ordered) {
      if (rows.size <= cap) break;
      if (!protectedIds.includes(row.id)) rows.delete(row.id);
    }
    if (ordered.length > cap) reason(message);
  }
  function putFight(fight: FightView, sequence: bigint) {
    put(fights, boundedFight(fight, fight.ended === null || selected === fight.id), sequence);
    if (area !== 'realm' || (kills.get(fight.id)?.sequence ?? -1n) > sequence) return;
    if (!fight.kill || fight.kill.evidence === 'realm_announcement'
      || !['boss', 'event_boss', 'miniboss', 'quest'].includes(fight.target_kind ?? '')
      || !inRealmVisit(window, fight.kill.at)) {
      kills.delete(fight.id); return;
    }
    // Retain the unique local candidate even before identity is verified. The
    // view verifies it against the current character before displaying damage.
    const locals = fight.players.filter(row => row.is_local);
    const self = locals.length === 1 ? { ...locals[0], sources: [] } : null;
    put(kills, { id: fight.id, session_id: fight.session_id, run_id: fight.run_id,
      target_type: fight.target_type, target_kind: fight.target_kind, started: fight.started,
      ended: fight.ended, kill: fight.kill, self,
      partial: !fight.complete_capture || fight.issues.length > 0
        || fight.players.some(row => row.sources.some(source => source.estimated)) }, sequence);
  }
  function putEvent(event: InstanceEventView, sequence: bigint) {
    put(events, event, sequence);
    if (area !== 'realm' || (announcements.get(event.id)?.sequence ?? -1n) > sequence) return;
    if (event.kind.type === 'realm_event_killed') put(announcements, event, sequence);
    else announcements.delete(event.id);
  }
  function boundedFight(fight: FightView, keepSeries: boolean): FightView {
    const local = verifiedSelf(character, fight);
    const players = fight.players.slice(0, 5);
    if (local && !players.some(row => row.player_id === local.player_id)) players.splice(4, 1, local);
    let buckets = fight.buckets;
    let series = keepSeries ? fight.player_series ?? null : null;
    if (fight.ended !== null && buckets.length > 120) {
      const original = buckets;
      const indexes = Array.from({ length: 120 }, (_, i) => Math.floor(i * (original.length - 1) / 119));
      buckets = indexes.map((index, i) => {
        const bucket = original[index], previous = indexes[i - 1];
        // Do not bridge a skipped unknown-local interval with a confident line.
        const unknown = i > 0 && original.slice(previous + 1, index + 1).some(row => row.local === null);
        return unknown ? { ...bucket, local: null } : bucket;
      });
      series = series?.filter(row => row.points.length === original.length)
        .map(row => ({ ...row, points: indexes.map(index => row.points[index]) })) ?? null;
    }
    return { ...fight, players, buckets, player_series: series };
  }
  function bound() {
    const candidates = area === 'realm' && window.visitKey === null ? snapshotFights : [...fights.values()].map(row => row.value);
    const current = selectBossFight(candidates, focused, { area, at: observedAt, character })?.id;
    focused = current ?? null;
    if (!explicitSelection && current) selected = current;
    trim(fights, 64, value => value.started, [selected, current].filter((id): id is string => id != null));
    // Separate caps keep ordinary traffic out of visit history. These compact
    // records also travel with the existing bounded same-instance visit cache.
    trim(kills, 500, value => value.kill!.at, [], 'Realm kill history truncated at 500 kills; earlier kills were omitted.');
    trim(announcements, 500, value => value.at, [], 'Realm announcement history truncated at 500 announcements; earlier announcements were omitted.');
    trim(drops, area === 'realm' ? 512 : 128, value => value.at, [], area === 'realm'
      ? 'Realm loot history truncated at 512 bags; earlier bags were omitted.'
      : 'Observation window truncated; older rows were omitted.');
    trim(events, 256, value => value.at);
    for (const [id, row] of fights) {
      if (row.value.ended !== null && id !== selected && row.value.player_series != null) {
        fights.set(id, { ...row, value: { ...row.value, player_series: null } });
      }
    }
  }
  function accepts(value: { session_id: string; run_id?: string | null }, at: string) {
    if (value.session_id !== session) return false;
    if (window.runId !== null) return value.run_id === window.runId;
    return area === 'realm' && window.visitKey !== null && value.run_id == null
      && inRealmVisit(window, at);
  }
  function capture(value: CaptureStatus) {
    if (disposed) return;
    captureState = value.state;
    if (!['Live', 'Recovering', 'WaitingForGame'].includes(value.state)) {
      reset('Capture is not live; visit observations were retired.'); paused = true; emit();
    } else if (value.state !== 'Live' && !paused) {
      paused = true;
      reason('Stale observations during capture recovery.');
      recovery = setTimeout(() => { reset('Capture recovery exceeded the observation grace period.'); emit(); }, RECOVERY_GRACE_MS);
      emit();
    }
  }
  return {
    snapshot(value: LiveSnapshot) {
      if (disposed) return;
      const changed = window.streamId !== value.stream_id || session !== (value.session?.id ?? null);
      if (changed) {
        reset('Visit entry was not observed.', value.stream_id); snapshotSequence = -1n;
        character = null; verifiedCharacterId = null;
      }
      const sequence = BigInt(value.sequence);
      // A delayed prior-map snapshot cannot retire a resumed visit or seed its fame.
      if (sequence < snapshotSequence || (entrySequence !== null && sequence < entrySequence)) return;
      session = value.session?.id ?? null; observeCharacter(value.character, sequence, value.area_kind === area);
      if (value.session?.ended != null) { reset('Session ended.'); paused = true; emit(); return; }
      if (value.capture.state !== 'Live') { capture(value.capture); return; }
      if (captureState !== null && captureState !== 'Live') return;
      if (paused) { reset('Capture resumed; earlier visit observations are incomplete.'); paused = false; }
      clearTimeout(recovery); recovery = undefined;
      snapshotSequence = sequence;
      const runId = value.area_kind === 'dungeon' ? value.run?.id ?? null : null;
      if (runId !== window.runId || value.area_kind !== area) {
        if (area === 'realm' && instanceKey !== null) {
          // Snapshots can overtake map events. Retain the last known realm
          // evidence, excluding unobserved time and fame after departure.
          reason('Realm departure was first observed in a snapshot; the segment boundary is incomplete.');
          // Bounds are exclusive, so include the last observed microsecond.
          leaveRealm((BigInt(observedAt) + 1n).toString());
        }
        reset(runId !== null ? 'Current run snapshot has partial timeline coverage.' : 'Visit entry was not observed.', window.streamId, true);
        window = { ...window, runId };
      }
      area = value.area_kind;
      observedAt = value.at;
      if (area === 'realm' && entrySequence !== null && sequence >= fameSequence && value.instance?.visit_fame_delta !== undefined) {
        updateFame(value.instance.visit_fame_delta, sequence);
        // Anchor later character events to the engine's authoritative per-entry delta.
        fameEntry = value.character?.fame != null && value.instance.visit_fame_delta !== null
          ? value.character.fame - value.instance.visit_fame_delta : null;
      }
      const scoped = runId !== null || (area === 'realm' && entrySequence !== null && sequence >= entrySequence);
      // Mid-visit snapshots may establish shared focus without claiming visit membership.
      snapshotFights = !scoped && area === 'realm' ? scopedFights(value) : [];
      if (scoped) {
        const activeIds = new Set(value.fights.map(fight => fight.id));
        for (const [id, row] of fights) {
          if (row.value.ended === null && row.sequence <= sequence && !activeIds.has(id)) fights.delete(id);
        }
        for (const fight of value.fights) {
          if (accepts(fight, fight.started)) putFight(fight, sequence);
        }
        for (const drop of value.drops) {
          // Realm snapshots can revise a received bag, never seed visit membership.
          if (accepts(drop, drop.at) && (runId !== null || drops.has(drop.id))) put(drops, drop, sequence);
        }
      }
      if ((value.instance?.events_dropped ?? 0) > 0) reason('Capture omitted instance events.');
      bound(); emit();
    },
    event(envelope: EventEnvelope) {
      if (disposed) return;
      if (window.streamId !== envelope.stream_id) {
        reset('Source changed; visit entry was not observed.', envelope.stream_id);
        session = null; character = null; verifiedCharacterId = null; snapshotSequence = -1n;
      }
      const event = envelope.event, sequence = BigInt(envelope.sequence);
      if ('capture_state_changed' in event) { capture(event.capture_state_changed); return; }
      if ('session_updated' in event) {
        if (session !== event.session_updated.id || event.session_updated.ended !== null) {
          reset('Session changed; visit entry was not observed.'); character = null; verifiedCharacterId = null;
        }
        session = event.session_updated.id;
      }
      if ('character_updated' in event) {
        observeCharacter(event.character_updated, sequence);
      }
      if ('map_entered' in event) enterMap(envelope, event.map_entered);
      if ('run_started' in event && event.run_started.session_id === session) {
        if ((window.runId !== null && window.runId !== event.run_started.id) || area !== event.run_started.area_kind) {
          leaveRealm(event.run_started.started);
          reset('Run changed without an observed map entry.', window.streamId, true);
        }
        area = event.run_started.area_kind; window = { ...window, runId: event.run_started.id };
      }
      if (!paused) {
        const fight = 'fight_started' in event ? event.fight_started : 'fight_updated' in event ? event.fight_updated : 'fight_ended' in event ? event.fight_ended : null;
        const drop = 'bag_dropped' in event ? event.bag_dropped : 'drop_updated' in event ? event.drop_updated : null;
        if (fight && accepts(fight, fight.started)) putFight(fight, sequence);
        if (drop && accepts(drop, drop.at)) put(drops, drop, sequence);
        if ('instance_event' in event && accepts(event.instance_event, event.instance_event.at)) putEvent(event.instance_event, sequence);
      }
      if (BigInt(envelope.at) > BigInt(observedAt)) observedAt = envelope.at;
      bound(); emit();
    },
    state(value: LiveState) {
      if (disposed || phase === value.phase) return;
      phase = value.phase;
      if (value.phase === 'live') return;
      reset(value.phase === 'resyncing' ? 'Live sequence gap; visit observations are incomplete.' : 'Live source retired; visit entry was not observed.');
      if (value.phase !== 'resyncing') { session = null; character = null; verifiedCharacterId = null; area = 'unknown'; snapshotSequence = -1n; captureState = null; paused = false; }
      emit();
    },
    capture,
    select(fightId: string | null) {
      if (disposed) return;
      selected = fightId; explicitSelection = fightId !== null; bound(); emit();
    },
    dispose() {
      if (disposed) return;
      reset('Live window disposed.'); emit(); disposed = true; character = null; verifiedCharacterId = null; session = null;
    },
  };
}

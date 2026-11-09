import { writable, type Readable } from 'svelte/store';
import type { DropView } from '../../bindings/DropView';
import type { EventEnvelope } from '../../bindings/EventEnvelope';
import type { FightView } from '../../bindings/FightView';
import type { LiveSnapshot } from '../../bindings/LiveSnapshot';
import type { Outcome } from '../../bindings/Outcome';
import type { RunView } from '../../bindings/RunView';
import { verifiedRunSelf } from '../../shell/rail';
import { knownNumber } from '../evidence';
import type { HomeModeState } from '../mode';
import type { HomeLiveWindow } from '../v2/contracts';
import { emptyHomeLiveWindow } from '../v2/live';
import { rankedDrops } from '../v3/loot-rank';
import { createRecapPresentation } from './recap-presentation';

export type RecapSource = { kind: 'run'; streamId: string; runId: string }
  | { kind: 'visit'; streamId: string; visitKey: string; enteredSequence: string | null };
export function storedVisitId(source: Extract<RecapSource, { kind: 'visit' }>): string | null {
  return source.enteredSequence === null ? null : `${source.streamId}:visit:${source.enteredSequence}`;
}
export interface RecapSummary {
  kind: 'run' | 'visit'; title: string; outcome: Outcome | null; startedAt: string; endedAt: string | null;
  completedAt?: string | null;
  localDps: number | null; rank: number | null; players: number | null;
  fights: Pick<FightView, 'id' | 'target_type' | 'kill'>[] | null;
  best: { drop: DropView } | null; runId: string | null;
}
export interface RecapState { source: RecapSource; summary: RecapSummary; watermark: string }
type Remembered = { source: RecapSource; snapshot: LiveSnapshot | null; window: HomeLiveWindow;
  snapshotStale: boolean; run: RunView | null; endedAt: string | null; watermark: string | null };

function summarize(last: Remembered): RecapState {
  const { snapshot, window, source, run } = last;
  // Identity fields survive a hide. Only a snapshot of this source is retained;
  // totals need fresh evidence, and cannot be reconstructed from bounded rows.
  const totals = last.snapshotStale ? null : snapshot;
  const self = source.kind === 'run' ? verifiedRunSelf(totals) : null;
  const drop = window.partial ? null : rankedDrops(window.drops)[0];
  return { source, watermark: last.watermark!, summary: {
    kind: source.kind,
    title: (source.kind === 'run' ? run?.dungeon : snapshot?.instance?.context.realm_name ?? snapshot?.map) ?? '—',
    outcome: source.kind === 'run' ? run?.outcome ?? null : null,
    // live.ts sets visitStarted only from observed map entries, including the
    // oldest retained segment after truncation. Partial rows do not erase it.
    startedAt: source.kind === 'run' ? run?.started ?? '' : window.visitStarted ?? '',
    endedAt: run?.ended ?? last.endedAt, completedAt: run?.completed_at ?? null,
    // There are no authoritative whole-visit combat totals in the live DTO.
    // A focused fight or a bounded participant list cannot establish them.
    localDps: knownNumber(self?.dps), rank: knownNumber(self?.rank),
    players: source.kind === 'run' ? totals?.run_totals?.players.length || null : null,
    fights: window.partial ? null : window.fights.map(({ id, target_type, kill }) => ({ id, target_type, kill })),
    best: drop ? { drop } : null,
    runId: source.kind === 'run' ? source.runId : null,
  } };
}

/** Shell delivers ordered events and observes windows at area boundaries and batch end, even while hidden. */
export function createRecapTracker(): {
  observe(snapshot: LiveSnapshot | null, window: HomeLiveWindow): void;
  onEvent(envelope: EventEnvelope): void;
  setVisible(visible: boolean): void;
  presentation: ReturnType<typeof createRecapPresentation>;
  dismiss(): void; subscribe: Readable<RecapState | null>['subscribe'];
} {
  const state = writable<RecapState | null>(null);
  const presentation = createRecapPresentation();
  let streamId: string | null = null, last: Remembered | null = null;
  let hasRecap = false, dirty = false, visible = true;
  let observedSnapshot: LiveSnapshot | null = null, transitionSequence = -1n;
  let visitEntry: { sequence: string; at: string } | null = null;
  function enteredSequence(window: HomeLiveWindow): string | null {
    // Realm re-entry restores the presentation key of the first period. Its
    // saved visit ID still belongs to the newly observed MapEntered event.
    return visitEntry && (window.visitKey === `${streamId}:${visitEntry.sequence}`
      || window.realmPeriods?.at(-1)?.started === visitEntry.at) ? visitEntry.sequence : null;
  }
  function clear() { const changed = hasRecap; hasRecap = false; if (changed) state.set(null); }
  function enter(source: RecapSource) {
    presentation.hide(); presentation.back(); dirty = true;
    last = { source, snapshot: null, window: emptyHomeLiveWindow(), snapshotStale: true, run: null, endedAt: null, watermark: null };
  }
  function resetStream(next: string) {
    if (streamId === next) return;
    clear(); presentation.reset(); last = null; streamId = next; observedSnapshot = null; transitionSequence = -1n; visitEntry = null;
  }
  function finish(at: string, sequence: string | null) {
    if (!last) return;
    if (last.endedAt === null) { last.endedAt = at; dirty = true; }
    // A window can supply the departure time before its envelope/snapshot.
    // Never authorize saved reads with the preceding area's sequence.
    if (last.watermark === null && sequence !== null) { last.watermark = sequence; dirty = true; }
  }
  function publish() {
    if (dirty && last && last.endedAt !== null && last.watermark !== null) {
      hasRecap = true;
      const recap = summarize(last);
      state.set(recap);
      presentation.remember(JSON.stringify([recap.source, recap.watermark]));
    }
    dirty = false;
  }
  function matches(window: HomeLiveWindow) {
    return last && window.streamId === last.source.streamId && (last.source.kind === 'run'
      ? window.runId === last.source.runId : window.runId === null && window.visitKey === last.source.visitKey);
  }
  function remember(snapshot: LiveSnapshot) {
    if (!last) return;
    const needsTerminal = last.snapshotStale && last.endedAt !== null && last.source.kind === 'run' && snapshot.run?.ended == null;
    last.snapshot = snapshot; last.snapshotStale = !visible || needsTerminal; dirty = true;
    if (last.source.kind === 'run' && snapshot.run?.id === last.source.runId) {
      // A running snapshot cannot undo a terminal ordered event.
      if (snapshot.run.ended !== null || last.run?.ended == null) last.run = snapshot.run;
      if (snapshot.run.ended !== null) finish(snapshot.run.ended, snapshot.sequence);
    }
  }
  return {
    subscribe: state.subscribe,
    presentation,
    dismiss: presentation.back,
    setVisible(next) {
      if (visible && !next && last) last.snapshotStale = true;
      visible = next;
    },
    onEvent(envelope) {
      resetStream(envelope.stream_id);
      const event = envelope.event;
      if ('map_entered' in event) {
        const map = event.map_entered;
        visitEntry = map.kind === 'realm' ? { sequence: envelope.sequence, at: map.at } : null;
        transitionSequence = BigInt(envelope.sequence);
        if (last?.source.kind === 'visit') { finish(map.at, envelope.sequence); publish(); }
        if (map.kind === 'realm') enter({ kind: 'visit', streamId: envelope.stream_id,
          visitKey: `${envelope.stream_id}:${envelope.sequence}`, enteredSequence: envelope.sequence });
        else if (map.kind === 'dungeon') { presentation.hide(); presentation.back(); last = null; }
        else finish(map.at, envelope.sequence);
      }
      if ('run_started' in event && event.run_started.area_kind === 'dungeon') {
        transitionSequence = BigInt(envelope.sequence);
        enter({ kind: 'run', streamId: envelope.stream_id, runId: event.run_started.id });
        last!.run = event.run_started;
      }
      const run = 'run_ended' in event ? event.run_ended : 'run_updated' in event ? event.run_updated : null;
      if (run && last?.source.kind === 'run' && run.id === last.source.runId) {
        last.run = run; dirty = true;
        if (run.ended !== null) finish(run.ended, envelope.sequence);
      }
      publish();
    },
    observe(snapshot, window) {
      const nextStream = window.streamId ?? snapshot?.stream_id;
      if (nextStream == null) return;
      resetStream(nextStream);
      if (snapshot && snapshot.capture.state !== 'Live') return;
      const fresh = snapshot && snapshot !== observedSnapshot && snapshot.stream_id === streamId
        && BigInt(snapshot.sequence) >= transitionSequence;
      observedSnapshot = snapshot;
      if (fresh) {
        const run = snapshot.run;
        if (last?.source.kind === 'visit' && snapshot.area_kind === 'dungeon') { finish(snapshot.at, snapshot.sequence); publish(); }
        if (last?.source.kind === 'run' && run?.id === last.source.runId) remember(snapshot);
        else if (snapshot.area_kind === 'dungeon' && run?.area_kind === 'dungeon' && run.ended === null) {
          enter({ kind: 'run', streamId: nextStream, runId: run.id }); remember(snapshot);
        } else if (snapshot.area_kind === 'realm') {
          const visitKey = window.visitKey ?? `${nextStream}:${snapshot.sequence}`;
          if (last?.source.kind !== 'visit' || last.source.visitKey !== visitKey) {
            enter({ kind: 'visit', streamId: nextStream, visitKey, enteredSequence: enteredSequence(window) });
          }
          remember(snapshot);
        } else if (snapshot.area_kind === 'dungeon' && !run) { presentation.hide(); presentation.back(); last = null; }
        if (!['realm', 'dungeon'].includes(snapshot.area_kind)) {
          finish(window.streamId === streamId && !matches(window) ? window.visitStarted ?? snapshot.at : snapshot.at, snapshot.sequence);
        }
      }
      // A window can establish a transition before a fresh snapshot (including
      // a whole hidden run). Never pair the prior area's snapshot with it.
      if (window.streamId === streamId && (window.runId !== null || window.visitKey !== null) && !matches(window)
        && (!snapshot || window.observedAt === undefined || BigInt(window.observedAt) >= BigInt(snapshot.at))) {
        if (window.runId !== null) enter({ kind: 'run', streamId: nextStream, runId: window.runId });
        else if (window.realmPeriods?.length || (snapshot?.area_kind === 'realm' && window.observedAt === snapshot.at)) {
          const sameSnapshotEntry = snapshot?.area_kind === 'realm' && window.observedAt === snapshot.at;
          enter({ kind: 'visit', streamId: nextStream, visitKey: window.visitKey!, enteredSequence: enteredSequence(window) });
          if (sameSnapshotEntry) remember(snapshot!);
        } else if (window.visitStarted !== null) finish(window.visitStarted, null);
      }
      if (last && matches(window) && last.window !== window) { last.window = window; dirty = true; }
      publish();
    },
  };
}

export function homeScreen(mode: HomeModeState, recap: RecapState | null, open = false): 'dungeon' | 'realm' | 'recap' | 'nexus' {
  return open && recap !== null ? 'recap' : mode.mode;
}

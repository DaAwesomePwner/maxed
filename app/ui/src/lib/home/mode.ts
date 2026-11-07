import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { LiveState } from '../bridge/live';
import type { HomeScreen } from './v2/contracts';

export type HomeMode = HomeScreen;
export interface HomeModeState {
  mode: HomeMode;
  streamId: string | null;
  runId: string | null;
  uncertainSinceMs: number | null;
  stale: boolean;
}
export const RECOVERY_GRACE_MS = 1500;
export const initialHomeMode = (): HomeModeState => ({
  mode: 'nexus', streamId: null, runId: null, uncertainSinceMs: null, stale: false,
});

/** Call only with snapshots accepted by the shell's ordered subscription. */
export function nextHomeMode(previous: HomeModeState, snapshot: LiveSnapshot | null,
  capture: CaptureStatus | null, nowMs: number): HomeModeState {
  const streamId = snapshot?.stream_id ?? previous.streamId;
  const runId = snapshot?.run?.id ?? (snapshot ? null : previous.runId);
  const changed = streamId !== previous.streamId || runId !== previous.runId;
  const base: HomeModeState = { mode: 'nexus', streamId, runId, uncertainSinceMs: null, stale: false };
  const status = capture ?? snapshot?.capture;
  if (status && !['Live', 'Recovering', 'WaitingForGame'].includes(status.state)) return base;
  if (snapshot?.session?.ended != null || snapshot?.run?.ended != null) return base;
  if (snapshot && ['nexus', 'other_hub'].includes(snapshot.area_kind)) return base;
  if (snapshot?.area_kind === 'realm' && status?.state === 'Live') return { ...base, mode: 'realm' };
  if (snapshot?.area_kind === 'unknown' && !snapshot.run && status?.state === 'Live') return base;
  if (snapshot?.area_kind === 'dungeon' && snapshot.run?.area_kind === 'dungeon'
    && snapshot.run.ended === null && status?.state === 'Live') return { ...base, mode: 'dungeon' };

  // New streams/runs must never borrow the old run's grace or numeric values.
  const uncertainSinceMs = changed ? nowMs : previous.uncertainSinceMs ?? nowMs;
  const retaining = !changed && previous.mode !== 'nexus'
    && nowMs - uncertainSinceMs < RECOVERY_GRACE_MS;
  return { ...base, mode: retaining ? previous.mode : 'nexus', uncertainSinceMs, stale: true };
}

/** Shared observation/grace policy for Home and the rail, independent of route. */
export function createHomeObservation({ pauseOnCaptureRecovery = false }: { pauseOnCaptureRecovery?: boolean } = {}) {
  let mode = initialHomeMode();
  let retained: LiveSnapshot | null = null;
  let awaitingFresh: LiveSnapshot | null = null;
  let observed: LiveSnapshot | null = null;
  let blockedCharacter: string | null = null;
  let lastAccount: string | null = null;
  let awaitingNotice = false;
  let awaitingIdentity = false;
  const observe = (next: LiveSnapshot | null, capture: CaptureStatus | null, state: LiveState | null, now: number) => {
    if (state?.phase === 'connecting' || (next && mode.streamId !== null && next.stream_id !== mode.streamId)) {
      mode = initialHomeMode(); retained = null; awaitingFresh = null; observed = null;
      blockedCharacter = null; lastAccount = null; awaitingNotice = false; awaitingIdentity = false;
    }
    if (state?.phase === 'connecting') return { mode, retained, paused: false, delay: null };
    if (state?.phase === 'failed') {
      mode = { mode: 'nexus', streamId: next?.stream_id ?? mode.streamId,
        runId: next?.run?.id ?? null, uncertainSinceMs: null, stale: true };
      retained = null;
      return { mode, retained, paused: false, delay: null };
    }
    if (blockedCharacter !== null) {
      if (!next?.character?.identity_verified || next.character.id === blockedCharacter) {
        return { mode: initialHomeMode(), retained: null, paused: false, delay: null };
      }
      blockedCharacter = null;
    }
    const fresh = next !== null && next !== observed;
    const resyncing = state?.phase === 'resyncing';
    const waiting = next !== null && awaitingFresh === next && !fresh;
    const projected = nextHomeMode(mode, resyncing || waiting ? null : next, capture, now);
    // The rail retains observations in every area; Home keeps its layout rules.
    const captureRecovery = pauseOnCaptureRecovery
      && ['Recovering', 'WaitingForGame'].includes((capture ?? next?.capture)?.state ?? '');
    const paused = resyncing || waiting || projected.stale || captureRecovery;
    if (paused) awaitingFresh = next;
    if (next && !resyncing) observed = next;
    if (next && (next.stream_id !== retained?.stream_id || next.run?.id !== retained?.run?.id
      || next.character?.id !== retained?.character?.id)) retained = null;
    if (!paused) { retained = next; awaitingFresh = null; }
    else if (!retained) retained = next;
    else if (next?.run?.ended != null && retained.run?.ended !== next.run.ended) {
      // An explicit final run timestamp still freezes its elapsed duration.
      retained = { ...retained, run: next.run };
    }
    mode = projected;
    return { mode, retained, paused: paused && retained !== null, delay: mode.mode !== 'nexus' && mode.uncertainSinceMs !== null
      ? Math.max(1, RECOVERY_GRACE_MS - (now - mode.uncertainSinceMs)) : null };
  };
  return Object.assign(observe, {
    historyChanged() {
      if (awaitingNotice) { awaitingNotice = false; return; }
      this.accountSwitched(); awaitingIdentity = true;
    },
    accountSwitched() {
      blockedCharacter = retained?.character?.id ?? observed?.character?.id ?? blockedCharacter;
      retained = null; awaitingFresh = null;
    },
    onEvent(envelope: EventEnvelope) {
      if (!('local_identity_changed' in envelope.event)) return false;
      const identity = envelope.event.local_identity_changed;
      if (!identity.account_id || !identity.character_id) return false;
      // The first verified event can already belong to B while the retained
      // snapshot still belongs to A. Adopt it and pair its eventual notice too.
      const adopted = lastAccount === null;
      const switched = adopted
        ? retained?.character != null && retained.character.id !== identity.character_id
        : lastAccount !== identity.account_id;
      if (switched) {
        if (retained?.character?.id !== identity.character_id) this.accountSwitched();
      }
      if (switched || adopted) awaitingNotice = !awaitingIdentity;
      awaitingIdentity = false;
      lastAccount = identity.account_id;
      return switched;
    },
  });
}

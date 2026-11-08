import { createVisibilityPause, isPresentationVisible, observePresentationVisibility } from '../../bridge/visibility';
import type { Client } from '../../bridge/client';
import type { DungeonProfile } from '../../bindings/DungeonProfile';
import type { HistoryReply } from '../../bindings/HistoryReply';
import type { HomeBossBaselines } from '../../bindings/HomeBossBaselines';
import type { HomeDungeonStreak } from '../../bindings/HomeDungeonStreak';
import type { RunFightSummary } from '../../bindings/RunFightSummary';
import type { RunRecap } from '../../bindings/RunRecap';
import type { RunStory } from '../../bindings/RunStory';
import type { StoreStatus } from '../../bindings/StoreStatus';
import type { VisitRecap } from '../../bindings/VisitRecap';
import { HISTORY_CATCH_UP, historyControl, historyFailure, historyWaiting } from '../../runs/policy';
import { storedVisitId, type RecapState } from './recap';
import { USUAL_WINDOW_MICROS } from './reads';

export const RECAP_SAVE_TIMEOUT_MS = 5000;
export const RECAP_BUSY_RETRY_MS = 2000;
export type RunRecapData = { kind: 'run'; story: RunStory; extras: RunRecap | null; profile: DungeonProfile | null;
  baselines: HomeBossBaselines | null; streak: HomeDungeonStreak | null; recordsUnavailable: boolean };
export type VisitRecapData = { kind: 'visit'; visit: VisitRecap; baselines: HomeBossBaselines | null };
export type RecapLoad =
  | { status: 'saving' }
  | { status: 'partial'; reason: 'timeout' | 'failed' | 'unavailable'; message: string | null }
  | { status: 'saved'; data: RunRecapData | VisitRecapData };
type Input = { recap: RecapState | null; store: StoreStatus | null; streamId: string | null };
class ReadFailure extends Error {}
const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));

const usualRange = (started: string) => ({ start: String(BigInt(started) - BigInt(USUAL_WINDOW_MICROS)), end: started });
const bossTypes = (fights: RunFightSummary[]) => [...new Set(fights.flatMap(fight => {
  const type = fight.boss_type ?? (fight.target_kind === 'boss' || fight.target_kind === 'event_boss' ? fight.target_type : null);
  return type === null ? [] : [type];
}))].slice(0, 64);
// Match live realm Home's isBossFight selection, including minibosses and quests.
const visitBossTypes = (fights: RunFightSummary[]) => [...new Set(fights.flatMap(fight =>
  fight.target_type != null && fight.target_kind != null && fight.target_kind !== 'ordinary' ? [fight.target_type] : [],
))].slice(0, 64);

/** One fixed save checkpoint per recap; store updates wake the gate, never poll SQL. */
export function createRecapReads(
  client: Pick<Client, 'getRun' | 'getRunRecap' | 'getVisitRecap' | 'getDungeonProfile' | 'getHomeBossBaselines' | 'getHomeDungeonStreak'>,
  receive: (load: RecapLoad | null) => void,
  timeoutMs = RECAP_SAVE_TIMEOUT_MS,
): { update(input: Input): void; retry(): void; dispose(): void } {
  const visibility = createVisibilityPause();
  let remaining = timeoutMs, timerStarted = 0;
  let input: Input | null = null, key: string | null = null, revision = 0, disposed = false;
  let phase: 'waiting' | 'reading' | 'settled' = 'settled';
  let timer: ReturnType<typeof setTimeout> | undefined;
  const current = (attempt: number) => !disposed && attempt === revision && input?.recap !== null;
  function clearTimer() { clearTimeout(timer); timer = undefined; }
  function partial(reason: 'failed' | 'unavailable', message: string | null) {
    phase = 'settled'; clearTimer(); receive({ status: 'partial', reason, message });
  }
  function waiting() {
    if (!input?.recap) return HISTORY_CATCH_UP;
    const message = historyWaiting({ streamId: input.recap.source.streamId, required: input.recap.watermark, store: input.store });
    return message ?? (input.streamId !== null && input.streamId !== input.recap.source.streamId ? HISTORY_CATCH_UP : null);
  }
  async function read<T>(request: () => Promise<HistoryReply<T>>, attempt: number): Promise<T> {
    let retryUsed = false;
    for (;;) {
      if (!current(attempt)) throw new ReadFailure('Recap replaced.');
      if (!await visibility.wait() || !current(attempt)) throw new ReadFailure('Recap replaced.');
      const reply = await request();
      if (!current(attempt)) throw new ReadFailure('Recap replaced.');
      if (reply.status === 'ready') return reply.value;
      const control = historyControl(reply.status, retryUsed);
      if (!control.retry) throw new ReadFailure(control.message);
      // Busy means the attempt window is full; retry once after it drains.
      retryUsed = true; await delay(RECAP_BUSY_RETRY_MS);
    }
  }
  async function optional<T>(request: () => Promise<HistoryReply<T>>, attempt: number): Promise<T | null> {
    try { return await read(request, attempt); } catch { return null; }
  }
  async function load(recap: RecapState, attempt: number) {
    try {
      let data: RunRecapData | VisitRecapData;
      if (recap.source.kind === 'run') {
        const runId = recap.source.runId;
        const story = await read(() => client.getRun(runId), attempt);
        if (!current(attempt)) return;
        if (story === null) { partial('failed', historyFailure(null)); return; }
        let recordsUnavailable = false;
        const extras = await read(() => client.getRunRecap({ run_id: runId }), attempt)
          .catch(() => { recordsUnavailable = true; return null; });
        const { dungeon, character_id, started } = story.run;
        const profile = character_id === null ? null : await optional(() => client.getDungeonProfile({
          dungeon, character_id, range: usualRange(started),
        }), attempt);
        const baselines = character_id === null ? null : await optional(() => client.getHomeBossBaselines({
          character_id, range: usualRange(started), target_types: bossTypes(story.fights),
        }), attempt);
        const streak = await optional(() => client.getHomeDungeonStreak({ dungeon }), attempt);
        data = { kind: 'run', story, extras, profile, baselines, streak, recordsUnavailable };
      } else {
        const visitId = storedVisitId(recap.source);
        if (visitId === null) { partial('unavailable', null); return; }
        const visit = await read(() => client.getVisitRecap({ visit_id: visitId }), attempt);
        if (!current(attempt)) return;
        if (visit === null) { partial('unavailable', null); return; }
        const started = visit.periods[0]?.started, character_id = visit.character_id;
        const baselines = started === undefined || character_id === null ? null
          : await optional(() => client.getHomeBossBaselines({ character_id,
            range: usualRange(started), target_types: visitBossTypes(visit.fights),
          }), attempt);
        data = { kind: 'visit', visit, baselines };
      }
      if (!current(attempt)) return;
      // A store failure or stream change during SQL must not publish stale data.
      if (waiting() !== null) { phase = 'waiting'; pump(); return; }
      phase = 'settled'; clearTimer(); receive({ status: 'saved', data });
    } catch (error) {
      if (current(attempt)) partial('failed', error instanceof ReadFailure ? error.message : historyFailure(error));
    }
  }
  function pump() {
    if (!isPresentationVisible() || !input?.recap || phase === 'settled') return;
    const message = waiting();
    if (message !== null) {
      if (message !== HISTORY_CATCH_UP) { revision++; partial('failed', message); }
      return;
    }
    if (phase === 'reading') return;
    phase = 'reading'; void load(input.recap, revision);
  }
  function start() {
    revision++; clearTimer(); phase = 'waiting'; receive({ status: 'saving' });
    remaining = timeoutMs;
    resumeTimer();
    pump();
  }
  function resumeTimer() {
    if (timer !== undefined || phase === 'settled' || !isPresentationVisible()) return;
    const attempt = revision;
    timerStarted = Date.now();
    timer = setTimeout(() => {
      timer = undefined; remaining = 0;
      if (current(attempt)) receive({ status: 'partial', reason: 'timeout', message: HISTORY_CATCH_UP });
    }, remaining);
  }
  const releaseVisibility = observePresentationVisibility(visible => {
    if (!visible && timer !== undefined) {
      remaining = Math.max(0, remaining - (Date.now() - timerStarted)); clearTimer();
    } else if (visible) { resumeTimer(); pump(); }
  });
  return {
    update(next) {
      if (disposed) return;
      const source = next.recap?.source;
      const nextKey = source ? JSON.stringify([source.streamId, source.kind,
        source.kind === 'run' ? source.runId : [source.visitKey, source.enteredSequence], next.recap!.watermark]) : null;
      input = next;
      if (nextKey === null) {
        revision++; clearTimer(); key = null; phase = 'settled'; receive(null);
      } else if (nextKey !== key) { key = nextKey; start(); }
      else pump();
    },
    retry() { if (!disposed && input?.recap) start(); },
    dispose() {
      if (disposed) return;
      disposed = true; visibility.dispose(); releaseVisibility(); revision++; clearTimer(); input = null; receive(null);
    },
  };
}

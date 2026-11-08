import type { Client } from '../../bridge/client';
import { createVisibilityPause } from '../../bridge/visibility';
import type { HomeBossBaseline } from '../../bindings/HomeBossBaseline';
import type { DungeonProfile } from '../../bindings/DungeonProfile';
import type { EventEnvelope } from '../../bindings/EventEnvelope';
import type { HistoryReply } from '../../bindings/HistoryReply';
import type { HomeHud } from '../../bindings/HomeHud';
import type { HomeScope } from '../../bindings/HomeScope';
import type { HomeDungeonStreak } from '../../bindings/HomeDungeonStreak';
import type { DropView } from '../../bindings/DropView';
import type { TimeRange } from '../../bindings/TimeRange';
import { invalidatesLootHistory } from '../../loot/invalidation';
import { historyControl } from '../../runs/policy';
import { invalidatesStatsHistory } from '../../stats/invalidation';

export const USUAL_WINDOW_MICROS = 7_776_000_000_000; // number: epoch micros stay below 2^53
export interface HomeV4ReadInput {
  scope: HomeScope | null;
  dungeon: string | null;
  runId: string | null;
  profileDungeon: string | null;
  generation: string | null;
  drops?: readonly DropView[];
  characterId: string | null;
  bossTypes: number[];
  nowMicros: number;
}
export interface HomeV4ReadState {
  hud: HomeHud | null;
  streak: { generation: string; value: HomeDungeonStreak } | null;
  profile: DungeonProfile | null;
  usual: ReadonlyMap<number, HomeBossBaseline>;
  failedUsualTypes: ReadonlySet<number>;
  errors: { hud: boolean; streak: boolean; profile: boolean; usual: boolean };
}
interface ProfileEntry { value: DungeonProfile | null; failed: boolean }

/** Event-driven history reads; changing live values never polls saved history. */
export function createHomeV4Reads(
  client: Pick<Client, 'getHomeHud' | 'getDungeonProfile' | 'getHomeBossBaselines' | 'getHomeDungeonStreak'>,
  receive: (state: HomeV4ReadState) => void,
): { update(input: HomeV4ReadInput): void; onEvent(envelope: EventEnvelope): void; consumeStreak(generation: string): void; dispose(): void } {
  let input: HomeV4ReadInput | null = null, disposed = false;
  const visibility = createVisibilityPause();
  let scopeKey = '', profileKey = '', hudRevision = 0, characterRevision = 0;
  let streakKey = '', streakRevision = 0, consumption = 0;
  let state: HomeV4ReadState = { hud: null, streak: null, profile: null, usual: new Map(), failedUsualTypes: new Set(),
    errors: { hud: false, streak: false, profile: false, usual: false } };
  const memberships = new Map<string, { white: boolean; run: string | null; at: string }>();
  const membership = (drop: DropView) => ({ white: drop.bag_rank === 255, run: drop.run_id, at: drop.at });
  function rememberWhite(drop: DropView) {
    // Evict by first insertion; Map.set on an existing key keeps its position.
    memberships.set(drop.id, membership(drop));
    if (memberships.size > 512) memberships.delete(memberships.keys().next().value!);
  }
  const profiles = new Map<string, ProfileEntry>();
  const requestedTypes = new Set<number>(), failedTypes = new Set<number>();
  const timers = new Set<ReturnType<typeof setTimeout>>();
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let maxRefreshTimer: ReturnType<typeof setTimeout> | undefined;
  let refreshProfile = false, refreshStreak = false;
  let refreshRunEnded = false, streakAfterRun = false;
  let whiteCheckpoint: { id: string; at: string; retries: number } | null = null;
  let streakRetryTimer: ReturnType<typeof setTimeout> | undefined;
  let hudRunning = false, hudPending = false;
  let streakRunning = false, streakPending = false;
  let queue: Promise<void> = Promise.resolve();
  const emit = () => { if (!disposed) receive(state); };
  function error(key: keyof HomeV4ReadState['errors'], failed: boolean) {
    state = { ...state, errors: { ...state.errors, [key]: failed } };
  }
  function later(action: () => void, delay: number) {
    const timer = setTimeout(() => { timers.delete(timer); if (!disposed) action(); }, delay);
    timers.add(timer); return timer;
  }
  const range = (): TimeRange => ({ start: String(input!.nowMicros - USUAL_WINDOW_MICROS), end: String(input!.nowMicros) });

  // Serialize attempts, but release
  // the queue during retry delays so the other cards can still load.
  function read<T>(request: () => Promise<HistoryReply<T>>, current: () => boolean,
    accept: (value: T) => void, fail: () => void, complete: () => void = () => {}) {
    let retryUsed = false, superseded = 0;
    const finish = (action: () => void) => { try { action(); } finally { complete(); } };
    function enqueue() {
      const next = queue.then(async () => {
        if (disposed || !current()) return;
        if (!await visibility.wait() || disposed || !current()) return;
        let reply: HistoryReply<T>;
        try { reply = await request(); }
        catch {
          if (!disposed && current()) finish(fail);
          return;
        }
        if (disposed || !current()) return;
        if (reply.status === 'ready') finish(() => accept(reply.value));
        else if (reply.status === 'superseded' && superseded++ < 2) later(enqueue, 2000);
        else if (reply.status === 'busy' && historyControl(reply.status, retryUsed).retry) {
          retryUsed = true; later(enqueue, 2000);
        } else finish(fail);
      });
      // Delivery failures must not poison the queue or become read failures.
      queue = next.catch(() => {});
    }
    enqueue();
  }

  function readHud() {
    if (!input?.scope) return;
    if (hudRunning) { hudPending = true; return; }
    hudRunning = true;
    const scope = input.scope, revision = hudRevision;
    const current = () => revision === hudRevision;
    const complete = () => {
      if (!current()) return;
      hudRunning = false;
      if (hudPending) { hudPending = false; readHud(); }
    };
    read(() => client.getHomeHud(scope), current, value => {
      state = { ...state, hud: value }; error('hud', false); emit();
    }, () => { error('hud', true); emit(); }, complete);
  }

  function readStreak() {
    if (!input?.dungeon || !input.generation) return;
    if (streakRunning) { streakPending = true; return; }
    cancelStreakRetry();
    streakRunning = true;
    const dungeon = input.dungeon, generation = input.generation, revision = streakRevision;
    const current = () => revision === streakRevision;
    // A banner consumes the pre-white value and invalidates any read already
    // in flight. A later event-driven read can replace the consumed value.
    const beforeWhite = consumption;
    const complete = () => {
      if (!current()) return;
      streakRunning = false;
      if (streakPending) {
        streakPending = false;
        if (streakRetryTimer === undefined) readStreak();
      }
    };
    read(() => client.getHomeDungeonStreak({ dungeon }), current, value => {
      if (beforeWhite !== consumption) return;
      // A successful read can still precede the writer's commit. Keep the
      // consumed value until the latest observed current-run white is in history.
      if (whiteCheckpoint && (value.last_white_at === null || BigInt(value.last_white_at) < BigInt(whiteCheckpoint.at))) {
        if (whiteCheckpoint.retries < 3) {
          whiteCheckpoint.retries++;
          streakRetryTimer = later(() => { streakRetryTimer = undefined; readStreak(); }, 500);
        }
        return;
      }
      whiteCheckpoint = null;
      state = { ...state, streak: { generation, value } }; error('streak', false); emit();
    }, () => { error('streak', true); emit(); }, complete);
  }

  function cancelStreakRetry() {
    if (streakRetryTimer !== undefined) { clearTimeout(streakRetryTimer); timers.delete(streakRetryTimer); }
    streakRetryTimer = undefined;
  }

  function changesWhite(envelope: EventEnvelope): boolean {
    const event = envelope.event;
    const drop = 'bag_dropped' in event ? event.bag_dropped : 'drop_updated' in event ? event.drop_updated : null;
    if (!drop) return false;
    const previous = memberships.get(drop.id), next = membership(drop);
    // Absence represents non-white membership too. Retain only whites across
    // run changes so a late reclassification still invalidates the streak.
    if (next.white) rememberWhite(drop);
    else memberships.delete(drop.id);
    if ('bag_dropped' in event) return next.white;
    return (next.white || previous?.white === true)
      && (!previous || previous.white !== next.white || previous.run !== next.run || previous.at !== next.at);
  }

  function refresh() {
    for (const timer of [refreshTimer, maxRefreshTimer]) {
      if (timer !== undefined) { clearTimeout(timer); timers.delete(timer); }
    }
    refreshTimer = undefined; maxRefreshTimer = undefined;
    readHud();
    if (refreshStreak || streakAfterRun) readStreak();
    refreshStreak = false;
    // Run end has no saved timestamp in this reply. Recheck once at the next
    // HUD invalidation, without introducing a periodic history poll.
    streakAfterRun = refreshRunEnded; refreshRunEnded = false;
    if (refreshProfile) { profiles.clear(); readProfile(); }
    refreshProfile = false;
    for (const type of failedTypes) requestedTypes.delete(type);
    readUsual();
  }

  function readProfile() {
    if (!input?.profileDungeon) return;
    const key = profileKey;
    if (profiles.has(key)) return;
    const entry: ProfileEntry = { value: state.profile, failed: state.errors.profile };
    profiles.set(key, entry);
    const request = { dungeon: input.profileDungeon, character_id: input.characterId, range: range() };
    const publish = () => {
      if (key !== profileKey) return;
      state = { ...state, profile: entry.value }; error('profile', entry.failed); emit();
    };
    read(() => client.getDungeonProfile(request), () => profiles.get(key) === entry, value => {
      entry.value = value; entry.failed = false; publish();
    }, () => { entry.failed = true; publish(); });
  }

  function readUsual() {
    // Without an identified character, all-character DPS is not their usual.
    if (!input?.characterId) return;
    const types = [...new Set(input.bossTypes)].filter(type => !requestedTypes.has(type));
    const revision = characterRevision, character_id = input.characterId, window = range();
    // Reserve absent and failed rows too: snapshots must not retry those reads.
    for (const type of types) requestedTypes.add(type);
    // The command accepts at most 64 target types per batch.
    for (let offset = 0; offset < types.length; offset += 64) {
      const target_types = types.slice(offset, offset + 64);
      const publish = () => {
        state = { ...state, failedUsualTypes: new Set(failedTypes) };
        error('usual', failedTypes.size > 0); emit();
      };
      read(() => client.getHomeBossBaselines({ range: window, character_id, target_types }),
        () => revision === characterRevision, value => {
          const usual = new Map(state.usual);
          for (const row of value.rows) usual.set(row.target_type, row);
          for (const type of target_types) failedTypes.delete(type);
          state = { ...state, usual }; publish();
        }, () => { for (const type of target_types) failedTypes.add(type); publish(); });
    }
  }

  return {
    update(next) {
      if (disposed) return;
      // Copy the small DTO by value; Svelte's reactive proxies are not cloneable.
      const scope: HomeScope | null = next.scope === null ? null : 'session' in next.scope
        ? { session: { id: next.scope.session.id } }
        : { today: { range: { start: next.scope.today.range.start, end: next.scope.today.range.end } } };
      const nextScope = JSON.stringify(scope);
      const nextProfile = JSON.stringify([next.profileDungeon, next.characterId]);
      const nextStreak = JSON.stringify([next.dungeon, next.generation]);
      const scopeChanged = nextScope !== scopeKey;
      const profileChanged = nextProfile !== profileKey;
      const streakChanged = nextStreak !== streakKey;
      const characterChanged = !input || next.characterId !== input.characterId;
      input = { ...next, scope, bossTypes: [...next.bossTypes] };
      if (scopeChanged) {
        scopeKey = nextScope; hudRevision++;
        hudRunning = false; hudPending = false;
        state = { ...state, hud: null }; error('hud', false);
      }
      if (streakChanged) {
        streakKey = nextStreak; streakRevision++;
        streakRunning = false; streakPending = false; refreshStreak = false;
        whiteCheckpoint = null; cancelStreakRetry();
        refreshRunEnded = false; streakAfterRun = false;
        for (const drop of next.drops ?? []) {
          if (drop.bag_rank === 255 && !memberships.has(drop.id)) rememberWhite(drop);
        }
        state = { ...state, streak: null }; error('streak', false);
      }
      if (profileChanged) {
        profileKey = nextProfile;
        const cached = profiles.get(profileKey);
        state = { ...state, profile: cached?.value ?? null }; error('profile', cached?.failed ?? false);
      }
      if (characterChanged) {
        characterRevision++; requestedTypes.clear(); failedTypes.clear();
        state = { ...state, usual: new Map(), failedUsualTypes: new Set() }; error('usual', false);
      }
      if (scopeChanged || streakChanged || profileChanged || characterChanged) emit();
      if (scopeChanged) readHud();
      if (streakChanged) readStreak();
      if (profileChanged) readProfile();
      readUsual();
    },
    onEvent(envelope) {
      if (disposed || !input) return;
      if (!invalidatesLootHistory({ kind: 'loot', view: 'feed' }, envelope)
        && !invalidatesStatsHistory({ kind: 'stats', view: 'maps' }, envelope)) return;
      refreshProfile ||= 'run_ended' in envelope.event;
      const whiteChanged = changesWhite(envelope);
      if (input.dungeon !== null && whiteChanged) {
        const event = envelope.event;
        const drop = 'bag_dropped' in event ? event.bag_dropped : 'drop_updated' in event ? event.drop_updated : null;
        if (drop?.bag_rank === 255 && input.runId !== null && drop.run_id === input.runId) {
          // Late classification of another older bag cannot lower the required
          // timestamp. A correction to this white's own timestamp can replace it.
          if (!whiteCheckpoint || (whiteCheckpoint.id === drop.id
            ? whiteCheckpoint.at !== drop.at : BigInt(drop.at) >= BigInt(whiteCheckpoint.at))) {
            whiteCheckpoint = { id: drop.id, at: drop.at,
              retries: whiteCheckpoint?.id === drop.id ? whiteCheckpoint.retries : 0 };
          }
        } else if (drop?.id === whiteCheckpoint?.id) {
          // changesWhite already removed or reassigned this drop's membership.
          // Keep guarding the writer against the run's newest remaining white.
          whiteCheckpoint = null;
          for (const [id, member] of memberships) {
            if (member.white && input.runId !== null && member.run === input.runId
              && (!whiteCheckpoint || BigInt(member.at) > BigInt(whiteCheckpoint.at))) {
              whiteCheckpoint = { id, at: member.at, retries: 0 };
            }
          }
          if (!whiteCheckpoint) cancelStreakRetry();
        }
      }
      refreshRunEnded ||= input.dungeon !== null && 'run_ended' in envelope.event;
      refreshStreak ||= input.dungeon !== null && ('run_ended' in envelope.event || whiteChanged);
      if (refreshTimer !== undefined) { clearTimeout(refreshTimer); timers.delete(refreshTimer); }
      refreshTimer = later(refresh, 500);
      maxRefreshTimer ??= later(refresh, 2000);
    },
    consumeStreak(generation) {
      if (disposed || generation !== input?.generation) return;
      consumption++;
      if (state.streak?.generation !== generation) return;
      state = { ...state, streak: { ...state.streak, value: { ...state.streak.value, runs_since_last_white: 0 } } };
      emit();
    },
    dispose() {
      disposed = true;
      visibility.dispose();
      for (const timer of timers) clearTimeout(timer);
      timers.clear(); profiles.clear(); memberships.clear(); requestedTypes.clear(); failedTypes.clear(); input = null;
    },
  };
}

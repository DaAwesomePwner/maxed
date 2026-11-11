import { dungeonMetadata } from './dungeon-catalog';
import type { AlertView } from '../bindings/AlertView';
import type { AlertPreferences } from '../bindings/AlertPreferences';
import type { AlertLoudness } from '../bindings/AlertLoudness';
import type { AlertSession } from './session';

export function includesKeyPopDungeon(preferences: AlertPreferences, dungeon: string): boolean {
  return preferences.key_pop_dungeons == null
    ? !(preferences.key_pop_excluded_dungeons ?? []).includes(dungeon)
    : preferences.key_pop_dungeons.includes(dungeon);
}

export type KeyPopMetadata = { difficulty: number | null; exalt: boolean; exalt_gateway: boolean };
/** Undefined catalog fails open: the store already filtered at record time. */
export function keyPopAllows(preferences: AlertPreferences, dungeon: string, meta: KeyPopMetadata | null | undefined): boolean {
  if (!includesKeyPopDungeon(preferences, dungeon)) return false;
  if (preferences.key_pop_dungeons !== null || meta === undefined) return true;
  return (!preferences.key_pop_exalt_only || !!meta && (meta.exalt || meta.exalt_gateway))
    && (preferences.key_pop_min_difficulty == null || meta?.difficulty == null
      || meta.difficulty >= preferences.key_pop_min_difficulty);
}
export function filteredByRules(preferences: AlertPreferences, dungeon: string, meta = dungeonMetadata(dungeon)): boolean {
  return !keyPopAllows({ ...preferences, key_pop_excluded_dungeons: [] }, dungeon, meta)
    && preferences.key_pop_dungeons === null;
}

export function allowsAlert(preferences: AlertPreferences, alert: AlertView): boolean {
  return alert.kind !== 'key_popped' || (alert.details?.type === 'key_popped'
    && keyPopAllows(preferences, alert.details.dungeon, dungeonMetadata(alert.details.dungeon)));
}

export const keyPopFilterLimitMessage = 'Dungeon filters allow at most 256 combined entries. Clear a selection before adding another.';
export function keyPopFilterWithinLimit(preferences: Pick<AlertPreferences, 'key_pop_dungeons' | 'key_pop_excluded_dungeons'>): boolean {
  return (preferences.key_pop_dungeons?.length ?? 0) + preferences.key_pop_excluded_dungeons.length <= 256;
}

export const loudnessLabels: Record<AlertLoudness, string> = {
  off: 'Off', quiet_log_only: 'Log only', toast: 'Toast', banner: 'Banner',
};
export const loudnessChoices: AlertLoudness[] = ['off', 'quiet_log_only', 'toast', 'banner'];
export interface AlertPreferencesState {
  saved: AlertPreferences | null; draft: AlertPreferences | null;
  dirty: boolean; busy: boolean; message: string;
}
export const emptyAlertPreferences = (): AlertPreferencesState => ({ saved: null, draft: null, dirty: false, busy: false, message: '' });

function rebaseDungeonNames(saved: string[], base: string[], draft: string[]): string[] {
  const before = new Set(base), after = new Set(draft);
  const removed = new Set(base.filter(name => !after.has(name)));
  return [...new Set([...saved, ...draft.filter(name => !before.has(name))])]
    .filter(name => !removed.has(name));
}

function rebaseDungeonMode(saved: AlertPreferences, base: AlertPreferences, draft: AlertPreferences): AlertPreferences {
  const only = draft.key_pop_dungeons !== null;
  if (only === (base.key_pop_dungeons !== null) && only === (saved.key_pop_dungeons !== null)) return draft;
  // Translate saved policy changes into the chosen mode, rather than copying its representation.
  const candidates = new Set([base, saved, draft].flatMap(value => [
    ...(value.key_pop_dungeons ?? []), ...value.key_pop_excluded_dungeons,
  ]));
  const names = new Set(only ? draft.key_pop_dungeons! : draft.key_pop_excluded_dungeons);
  for (const dungeon of candidates) {
    const allowed = includesKeyPopDungeon(saved, dungeon);
    if (allowed === includesKeyPopDungeon(base, dungeon)) continue;
    if (allowed === only) names.add(dungeon);
    else names.delete(dungeon);
  }
  return only ? { ...draft, key_pop_dungeons: [...names] } : { ...draft, key_pop_excluded_dungeons: [...names] };
}

/** Local editor only. The borrowed settings session owns accepted policy and persistence. */
export function createAlertPreferences(session: AlertSession, receive: (state: AlertPreferencesState) => void,
  canEdit: () => boolean = () => true) {
  let state = emptyAlertPreferences(), revision = 0, disposed = false;
  let base: AlertPreferences | null = null;
  const copy = (value: AlertPreferences | null) => value ? { ...value } : null;
  const publish = () => { if (!disposed) receive({ ...state, saved: copy(state.saved), draft: copy(state.draft) }); };
  const release = session.preferences.subscribe(saved => {
    state.saved = copy(saved);
    if (saved) {
      if (state.draft && base && state.dirty) {
        // Retain local fields and list deltas; another editor may have saved a mute.
        const draft = state.draft, previous = base;
        const edits = Object.fromEntries((Object.keys(previous) as (keyof AlertPreferences)[])
          .filter(key => {
            const before = previous[key], after = draft[key];
            return Array.isArray(before) && Array.isArray(after)
              ? before.length !== after.length || before.some((value, index) => value !== after[index])
              : before !== after;
          }).map(key => {
            const before = previous[key], after = draft[key], incoming = saved[key];
            if ((key === 'key_pop_excluded_dungeons' || key === 'key_pop_dungeons')
              && Array.isArray(before) && Array.isArray(after) && Array.isArray(incoming)) {
              return [key, rebaseDungeonNames(incoming, before, after)];
            }
            // Null/array transitions change the mode, so retain the local field choice.
            return [key, after];
          }));
        state.draft = rebaseDungeonMode(saved, previous, { ...saved, ...edits });
      } else if (!state.busy) state.draft = copy(saved);
      base = copy(saved);
    } else if (!state.dirty && !state.busy) { state.draft = null; base = null; }
    publish();
  });
  return {
    edit(patch: Partial<AlertPreferences>) {
      if (disposed || !canEdit() || !state.draft) return;
      revision++; state.draft = { ...state.draft, ...patch };
      state.dirty = true; state.message = ''; publish();
    },
    revert() {
      if (disposed) return;
      revision++; state.draft = copy(state.saved); base = copy(state.saved); state.dirty = false; state.message = ''; publish();
    },
    async apply() {
      if (disposed || !canEdit() || !state.draft || !state.saved || !state.dirty || state.busy) return;
      if (!keyPopFilterWithinLimit(state.draft)) { state.message = keyPopFilterLimitMessage; publish(); return; }
      let request = revision, exceedsLimit = false;
      state.busy = true; state.message = ''; publish();
      try {
        await session.update(() => {
          if (disposed || !state.dirty) return null;
          if (!canEdit() || !state.draft || !state.saved) throw new Error('Alert preferences are unavailable.');
          if (!keyPopFilterWithinLimit(state.draft)) {
            exceedsLimit = true;
            throw new Error(keyPopFilterLimitMessage);
          }
          request = revision;
          return { ...state.draft };
        });
        if (disposed) return;
        if (request === revision || !state.dirty) { state.draft = copy(state.saved); state.dirty = false; }
        state.message = state.dirty ? 'Unsaved Changes' : 'Saved';
      } catch {
        if (!disposed) state.message = exceedsLimit ? keyPopFilterLimitMessage : 'Couldn’t save. Retry.';
      } finally { if (!disposed) { state.busy = false; publish(); } }
    },
    async refresh() {
      if (disposed || state.busy) return;
      state.busy = true; state.message = ''; publish();
      try { await session.refresh(); }
      catch { if (!disposed) state.message = 'Couldn’t load alerts. Retry.'; }
      finally {
        if (!disposed) { state.busy = false; if (!state.dirty) state.draft = copy(state.saved); publish(); }
      }
    },
    dispose() { disposed = true; release(); },
  };
}

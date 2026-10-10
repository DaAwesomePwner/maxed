import { get, writable } from 'svelte/store';
import type { AlertSession } from './session';
import { createAlertPreferences, emptyAlertPreferences, keyPopFilterWithinLimit, keyPopFilterLimitMessage } from './preferences';

export const DUNGEON_MUTE = Symbol('dungeon-mute');
export type DungeonMute = ReturnType<typeof createDungeonMute>;

/** Shared with the banner and log; survives policy removing the current banner. */
export function createDungeonMute(session: AlertSession, canEdit: () => boolean) {
  const state = writable({ busy: false, message: '', undo: false, failed: false });
  let editorState = emptyAlertPreferences();
  let editor = createAlertPreferences(session, value => { editorState = value; }, canEdit);
  let revision = 0;
  let operation: { dungeon: string; only: boolean; index: number; undoing: boolean } | null = null;
  let busy = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const clearTimer = () => { if (timer !== undefined) clearTimeout(timer); timer = undefined; };
  const available = () => canEdit() && !busy && get(session.status) === 'ready' && !!get(session.preferences);
  async function save() {
    if (!operation || !canEdit() || busy) return;
    const token = revision, op = operation;
    clearTimer(); busy = true;
    state.set({ busy: true, message: 'Updating alert preferences', undo: false, failed: false });
    // A failed readback leaves the editor without accepted policy. Retry that read first.
    if (!get(session.preferences)) await editor.refresh();
    if (token !== revision) return;
    const saved = get(session.preferences);
    if (saved) {
      editor.revert();
      const only = saved.key_pop_dungeons !== null;
      const names = [...(only ? saved.key_pop_dungeons! : saved.key_pop_excluded_dungeons)];
      // A failed action may be retried after Settings switched modes.
      if (!op.undoing && only !== op.only) { op.only = only; op.index = names.indexOf(op.dungeon); }
      // Mute disallows and Undo allows the dungeon in the currently saved mode.
      const remove = op.undoing ? !only : only;
      let next = names.filter(name => name !== op.dungeon);
      if (!remove) {
        if (op.undoing && only === op.only && op.index >= 0) next.splice(Math.min(op.index, next.length), 0, op.dungeon);
        else next = [...next, op.dungeon];
      }
      const patch = only ? { key_pop_dungeons: next } : { key_pop_excluded_dungeons: next };
      if (!keyPopFilterWithinLimit({ ...saved, ...patch })) {
        busy = false;
        state.set({ busy: false, message: keyPopFilterLimitMessage, undo: op.undoing, failed: false });
        return;
      }
      editor.edit(patch);
      await editor.apply();
    }
    if (token !== revision) return;
    busy = false;
    const failed = !saved || editorState.dirty;
    state.set({ busy: false, message: failed ? 'Couldn’t save. Retry.' : op.undoing ? `Restored ${op.dungeon}.` : `Muted ${op.dungeon}.`, undo: !failed && !op.undoing, failed });
    if (!failed) timer = setTimeout(() => { operation = null; state.set({ busy: false, message: '', undo: false, failed: false }); }, 10000);
  }
  function reset() {
    revision++; busy = false; operation = null; clearTimer();
    editor.dispose(); editor = createAlertPreferences(session, value => { editorState = value; }, canEdit);
    state.set({ busy: false, message: '', undo: false, failed: false });
  }
  return {
    state, available, preferences: session.preferences, status: session.status,
    async mute(dungeon: string) {
      if (!available()) return;
      const saved = get(session.preferences)!;
      const only = saved.key_pop_dungeons !== null;
      const names = only ? saved.key_pop_dungeons! : saved.key_pop_excluded_dungeons;
      if (only ? !names.includes(dungeon) : names.includes(dungeon)) return;
      revision++; operation = { dungeon, only, index: names.indexOf(dungeon), undoing: false };
      await save();
    },
    async unmute(dungeon: string) {
      if (!available()) return;
      const saved = get(session.preferences)!;
      const only = saved.key_pop_dungeons !== null;
      const names = only ? saved.key_pop_dungeons! : saved.key_pop_excluded_dungeons;
      if (only ? names.includes(dungeon) : !names.includes(dungeon)) return;
      revision++; operation = { dungeon, only, index: names.indexOf(dungeon), undoing: true };
      await save();
    },
    async undo() { if (!operation || !get(state).undo || !available()) return; operation.undoing = true; await save(); },
    retry: save,
    reset,
    dispose() { revision++; clearTimer(); editor.dispose(); },
  };
}

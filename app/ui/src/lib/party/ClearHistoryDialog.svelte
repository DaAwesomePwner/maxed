<script lang="ts">
  import type { Client } from '../bridge/client';
  let { client, onCleared, onCancel, onError }: { client: Client; onCleared: () => void; onCancel: () => void; onError: (message: string) => void } = $props();
  let dialog: HTMLDialogElement;
  let marksAndNotes = $state(false), busy = $state(false), error = $state('');
  $effect(() => {
    const element = dialog;
    if (element.showModal) element.showModal(); else element.open = true;
    return () => { if (element.open) element.close?.(); };
  });
  function cancel() { dialog.close?.(); onCancel(); }
  async function confirm() {
    if (busy) return;
    busy = true; error = '';
    try { await client.clearPartyHistory({ marks_and_notes: marksAndNotes }); dialog?.close?.(); onCleared(); }
    catch { error = 'Couldn’t clear party history. Retry to clear it.'; onError(error); }
    finally { busy = false; }
  }
</script>
<dialog bind:this={dialog} aria-label="Clear party history" oncancel={event => { event.preventDefault(); if (!busy) cancel(); }} onclose={onCancel}>
  <h2>Clear party history</h2>
  <p>Clear saved Party player history and other players' saved profiles, gear and timelines. Your own profiles and saved runs are kept.</p>
  {#if !marksAndNotes}<p>Keeping marks and notes also keeps each marked or noted player's name and first and last seen times. Their last profile, scope and map are removed.</p>{/if}
  <label><input type="checkbox" bind:checked={marksAndNotes} disabled={busy} /> Also clear marks and notes</label>
  {#if error}<p role="alert">{error}</p>{/if}
  <div class="actions"><button class="ctl" disabled={busy} onclick={cancel}>Cancel</button><button class="ctl" disabled={busy} onclick={confirm}>{busy ? 'Clearing…' : 'Confirm'}</button></div>
</dialog>
<style>
  dialog { max-width: 440px; background: var(--surface-raised); color: var(--text-primary); border: 1px solid var(--hairline-strong); border-radius: var(--radius-card); padding: var(--space-5); box-shadow: var(--elevation-popover); }
  dialog::backdrop { background: rgb(0 0 0 / 60%); }
  h2 { margin: 0 0 var(--space-3); font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); }
  p, label { font-size: var(--text-label); line-height: var(--leading-label); color: var(--text-secondary); }
  input[type=checkbox] { width: 1rem; height: 1rem; margin: 0 var(--space-2) 0 0; vertical-align: -2px; accent-color: var(--text-secondary); }
  .actions { display: flex; justify-content: flex-end; gap: var(--space-2); margin-top: var(--space-4); } [role=alert] { color: var(--danger); }
</style>

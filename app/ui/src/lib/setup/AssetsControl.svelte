<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { SetupSession } from './session';
  import { assetsText } from './presentation';
  let { session, preview = false }: { session: SetupSession; preview?: boolean } = $props();
  const id = $props.id();
  let path = $state('');
  // Cancel discards path edits; submitting a replacement restarts extraction.
  let editing = $state(false);
  let revision = 0;
  $effect(() => { if (!editing) path = $session.settings?.assets_path ?? ''; });
  let working = $derived($session.extracting || ['finding', 'extracting'].includes($session.setup?.assets ?? ''));
  async function submit() {
    if (preview || $session.busy || !path.trim()) return;
    const submitted = ++revision;
    const accepted = await session.selectAssets(path);
    if (accepted && submitted === revision && !$session.errors.assets) editing = false;
  }
</script>
<div class="assets-control">
  <p role="status" aria-live="polite" aria-atomic="true">{assetsText($session.setup)}</p>
  {#if $session.previousAssets && $session.setup?.assets !== 'ready'}<DebugNote text="Previously loaded assets are not verified for the current selection. A failed replacement keeps the last valid cache." />{/if}
  <form onsubmit={(event) => { event.preventDefault(); void submit(); }}>
    <label for={`${id}-path`}>Assets path</label>
    <input id={`${id}-path`} type="text" bind:value={path} oninput={() => { editing = true; revision++; }} placeholder="resources.assets" spellcheck="false" autocomplete="off" disabled={preview || $session.busy} />
    <div class="actions">
      <button type="submit" aria-disabled={preview || $session.busy || !path.trim()}>{working ? 'Replace' : 'Extract'}</button>
      <button type="button" disabled={!editing} onclick={() => { editing = false; revision++; path = $session.settings?.assets_path ?? ''; }}>Cancel</button>
      <button type="button" onclick={() => session.refreshSetup({ retry: true })}>Retry detection</button>
    </div>
  </form>
</div>
<style>
  .assets-control { display: grid; gap: var(--space-3); }
  form { display: grid; gap: var(--space-1); }
  label { color: var(--text-secondary); font-size: var(--text-caption); line-height: var(--leading-caption); font-weight: var(--weight-medium); }
  p { margin: 0; color: var(--text-secondary); }
  input { min-width: 0; width: 100%; }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); margin-top: var(--space-2); }
</style>

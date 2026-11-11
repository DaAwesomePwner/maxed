<script lang="ts">
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { getContext } from 'svelte';
  import { SHELL_CONNECTION, type ShellConnection } from '../lib/shell/context';
  import type { Client } from '../lib/bridge/client';
  import SetupStep from '../lib/setup/SetupStep.svelte';
  import AssetsControl from '../lib/setup/AssetsControl.svelte';
  import AdapterControl from '../lib/settings/AdapterControl.svelte';
  import { createSetupSession } from '../lib/setup/session';
  import HistoryUpgrade from '../lib/setup/HistoryUpgrade.svelte';
  import { historyRunning, historyUpgradeRequired } from '../lib/settings/storage-state';
  import { errorLine, npcapText, setupProgress } from '../lib/setup/presentation';
  import { captureLabels } from '../lib/shell/rail';
  let { client, preview = false }: { client: Client; preview?: boolean } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  let session = $derived(createSetupSession(client, preview, connection));
  $effect(() => { const current = session; current.start(); return () => current.dispose(); });
  let adapter = $state('');
  let adapterEdited = $state(false);
  $effect(() => { if (!adapterEdited) adapter = $session.settings?.adapter_id ?? ''; });
  let progress = $derived(setupProgress($session.setup, $session.capture, preview, $session.busy || $session.extracting));
  let active = $derived(['Live', 'WaitingForGame', 'Recovering'].includes($session.capture?.state ?? ''));
  let restartRequired = $derived($session.capture?.state === 'Failed' && ['Capture worker failed; restart Maxed'].includes($session.capture.message));
  // Setup, adapter and history failures sit beside their own retry buttons when those are on screen.
  let historyInline = $derived(!preview && $session.history?.phase !== 'ready'
    && !(historyUpgradeRequired($session.history) && $session.historyDeferred));
  let inline = $derived(new Set(['setup', ...(preview ? [] : ['adapters']), ...(historyInline ? ['history'] : [])]));
  let errors = $derived(Object.entries($session.errors).filter(([key, message]) => message && !inline.has(key)));
  let map = $state<string | null>(null);
  $effect(() => {
    const connectedClient = client;
    map = null;
    if (connection) return connection.snapshot.subscribe(snapshot => { map = snapshot?.map ?? null; });
    let disposed = false, revision = 0;
    let release: (() => void) | undefined;
    void (async () => {
      try {
        const stop = await connectedClient.onSnapshot((snapshot) => { if (!disposed) { revision++; map = ('base_sequence' in snapshot ? snapshot.snapshot : snapshot).map; } });
        if (disposed) { stop(); return; }
        release = stop;
        const before = revision;
        const snapshot = await connectedClient.querySnapshot();
        if (!disposed && before === revision) map = snapshot.map;
      } catch { /* Location remains explicitly unobserved. */ }
    })();
    return () => { disposed = true; release?.(); };
  });
  let upgrading = $derived($session.upgrading || historyRunning($session.history));
  let historyUnavailable = $derived(upgrading || $session.historyChecking || $session.history?.phase !== 'ready');
  let checkUnavailable = $derived(historyUnavailable || preview || $session.busy || $session.extracting || active || restartRequired || $session.setup?.npcap !== 'compatible');
  let adapterUnavailable = $derived(preview || $session.busy || $session.extracting || !$session.settings || !!$session.errors.settings);
  let startUnavailable = $derived(historyUnavailable || preview || $session.busy || $session.extracting || active || !$session.capture || restartRequired);
  let stopUnavailable = $derived(upgrading || preview || $session.busy || !$session.capture || $session.capture.state === 'Stopped' || restartRequired);
  async function applyAdapter() {
    if (adapterUnavailable) return;
    const accepted = await session.action(() => client.setAdapter(adapter || null), 'Couldn’t change adapter. Retry.');
    if (accepted) adapterEdited = false;
  }
</script>

<div class="setup-page">
  {#if preview || $session.setup?.demo}<p class="notice">Demo</p>{/if}
  <div class="error-summary" role="alert" aria-atomic="true">{#if errors.length}<ul>
    {#each errors as [key, message] (key)}<li><span>{errorLine(message)}</span>
      {#if key === 'settings'}<button class="ctl" onclick={() => session.refreshSettings()}>Retry settings</button>{/if}
      {#if key === 'capture'}<button class="ctl" onclick={() => session.connectCapture()}>Reconnect capture</button>{/if}</li>{/each}
  </ul>{/if}</div>
  <HistoryUpgrade {session} {preview} />
  <div class="actions">
    <button onclick={() => session.refreshSetup({ retry: true })}>Retry checks</button>
    <span class="inline-error" aria-live="assertive" aria-atomic="true">{$session.errors.setup ? errorLine($session.errors.setup) : ''}</span>
    <a href="#/settings">Settings</a>
  </div>
  <ol aria-label="Setup">
    <SetupStep number={1} heading="Npcap" current={progress.current === 1} complete={progress.npcap}>
      <p role="status" aria-live="polite" aria-atomic="true">{npcapText($session.setup)}</p>
      {#if $session.setup?.npcap === 'missing' || $session.setup?.npcap === 'incompatible' || $session.setup?.npcap === 'permission_denied' || $session.setup?.access === 'denied'}

        <label class="official-url">Npcap installer<input readonly value="https://npcap.com/#download" aria-label="Npcap URL" /></label>
      {/if}
      {#if $session.setup?.npcap === 'compatible' && $session.setup.access === 'unknown'}
        <DebugNote text="Finding Npcap does not prove this user can open an adapter. Start capture to check access." />
      {/if}
      <button aria-disabled={checkUnavailable} onclick={() => !checkUnavailable && session.action(() => client.startCapture(), 'Couldn’t check access. Retry.', { captureAction: true })}>Check access</button>
      <div class="adapter" oninput={() => { adapterEdited = true; }}>
        <AdapterControl adapters={$session.adapters} {preview} bind:value={adapter} disabled={preview || $session.busy || $session.extracting} />
      </div>
      <div class="actions">
        <button aria-disabled={adapterUnavailable} onclick={applyAdapter}>Apply adapter</button>
        {#if !preview}<button onclick={() => session.refreshAdapters()}>Retry adapters</button>
          <span class="inline-error" aria-live="assertive" aria-atomic="true">{$session.errors.adapters ? errorLine($session.errors.adapters) : ''}</span>{/if}
      </div>
      <DebugNote text="Adapter changes may stop and restart capture. An empty list or Auto selection does not verify access." />
    </SetupStep>
    <SetupStep number={2} heading="Game assets" current={progress.current === 2} complete={progress.assets && !preview && !$session.setup?.demo}>
      <AssetsControl {session} {preview} />
    </SetupStep>
    <SetupStep number={3} heading="Live capture" current={progress.current === 3} complete={progress.live}>
      <p role="status" aria-live="polite" aria-atomic="true">{progress.live ? 'Connected' : $session.capture?.state === 'Recovering' ? 'Reconnecting' : 'Waiting for game'}</p>
      <p>Capture: {$session.capture ? captureLabels[$session.capture.state] : 'Unavailable'}</p>
      {#if restartRequired}<p>Restart required</p>{/if}
      {#if map && !preview && !$session.setup?.demo}<p>Location: {map}</p>{:else}<DebugNote text="Location not yet observed." />{/if}

      <div class="actions">
        <button aria-disabled={startUnavailable} onclick={() => !startUnavailable && session.action(() => client.startCapture(), 'Couldn’t start capture. Retry.', { captureAction: true })}>Start capture</button>
        <button aria-disabled={stopUnavailable} onclick={() => !stopUnavailable && session.action(() => client.stopCapture(), 'Couldn’t stop capture. Retry.', { duringExtraction: true, captureAction: true })}>Stop capture</button>
      </div>
      {#if $session.capture}
        <dl aria-label="Capture diagnostics">
          <div><dt>Dropped frames</dt><dd>{$session.capture.dropped_frames}</dd></div>
          <div><dt>Unknown packets</dt><dd>{$session.capture.unknown_packets}</dd></div>
          <div><dt>Decode failures</dt><dd>{$session.capture.decode_failures}</dd></div>
        </dl>
      {/if}
    </SetupStep>
  </ol>
</div>
<style>
  .setup-page { display: grid; gap: var(--space-4); max-width: 65rem; min-width: 0; overflow-wrap: anywhere; font-size: var(--text-label); line-height: var(--leading-label); }
  ol { display: grid; gap: var(--space-4); margin: 0; padding: 0; }
  p { margin: var(--space-2) 0; color: var(--text-secondary); }
  .notice { color: var(--warning); }
  .error-summary:empty { display: none; }
  .inline-error { color: var(--danger); }
  .inline-error:empty { display: none; }
  .error-summary ul { display: grid; gap: var(--space-2); margin: 0; padding: 0; list-style: none; }
  .error-summary li { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2) var(--space-3); min-height: var(--control-dense); color: var(--danger); }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); margin: var(--space-3) 0; }
  a { color: var(--text-primary); text-decoration: none; text-underline-offset: 3px; }
  a:hover, a:focus-visible { text-decoration: underline; }
  .actions > a { display: inline-flex; align-items: center; min-height: var(--target-min); padding: 0 var(--space-2); color: var(--text-secondary); }
  .actions > a:hover { color: var(--text-primary); }
  .adapter { margin-top: var(--space-4); }
  .official-url { display: grid; gap: var(--space-1); color: var(--text-secondary); font-size: var(--text-caption); }
  input { min-width: 0; width: 100%; }
  dl { display: flex; flex-wrap: wrap; gap: var(--space-2) var(--space-6); margin: var(--space-3) 0 0; }
  dt { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); } dd { margin: 0; font-variant-numeric: tabular-nums; }
</style>

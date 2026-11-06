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
  import { historyRunning } from '../lib/settings/storage-state';
  import { npcapText, setupProgress } from '../lib/setup/presentation';
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
  let restartRequired = $derived($session.capture?.state === 'Failed' && ['Capture worker failed; restart Maxed', 'Live updates interrupted; reconnect or restart Maxed'].includes($session.capture.message));
  let errorText = $derived(Object.values($session.errors).filter(Boolean).join(' '));
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
  <p class="error-summary" role="alert" aria-atomic="true">{errorText}</p>
  <HistoryUpgrade {session} {preview} />
  <div class="actions">
    <button onclick={() => session.refreshSetup({ retry: true })}>Retry Checks</button>
    {#if $session.errors.settings}<button onclick={() => session.refreshSettings()}>Retry Settings</button>{/if}
    {#if $session.errors.capture}<button onclick={() => session.connectCapture()}>Reconnect Capture</button>{/if}
    <a href="#/settings">Settings</a>
  </div>
  <ol aria-label="Setup">
    <SetupStep number={1} heading="Npcap" current={progress.current === 1} complete={progress.npcap}>
      <p role="status" aria-live="polite" aria-atomic="true">{npcapText($session.setup)}</p>
      {#if $session.setup?.npcap === 'missing' || $session.setup?.npcap === 'incompatible' || $session.setup?.npcap === 'permission_denied' || $session.setup?.access === 'denied'}

        <label class="official-url">Npcap Installer<input readonly value="https://npcap.com/#download" aria-label="Npcap URL" /></label>
      {/if}
      {#if $session.setup?.npcap === 'compatible' && $session.setup.access === 'unknown'}
        <DebugNote text="Finding Npcap does not prove this user can open an adapter. Start Capture to check access." />
      {/if}
      <button aria-disabled={checkUnavailable} onclick={() => !checkUnavailable && session.action(() => client.startCapture(), 'Couldn’t check access. Retry.', { captureAction: true })}>Check Access</button>
      <div class="adapter" oninput={() => { adapterEdited = true; }}>
        <AdapterControl adapters={$session.adapters} {preview} bind:value={adapter} disabled={preview || $session.busy || $session.extracting} />
      </div>
      <div class="actions">
        <button aria-disabled={adapterUnavailable} onclick={applyAdapter}>Apply Adapter</button>
        {#if !preview}<button onclick={() => session.refreshAdapters()}>Retry Adapters</button>{/if}
      </div>
      <DebugNote text="Adapter changes may stop and restart capture. An empty list or Auto selection does not verify access." />
    </SetupStep>
    <SetupStep number={2} heading="Game Assets" current={progress.current === 2} complete={progress.assets && !preview && !$session.setup?.demo}>
      <AssetsControl {session} {preview} />
    </SetupStep>
    <SetupStep number={3} heading="Live Capture" current={progress.current === 3} complete={progress.live}>
      <p role="status" aria-live="polite" aria-atomic="true">{progress.live ? 'Connected' : $session.capture?.state === 'Recovering' ? 'Reconnecting' : 'Waiting for Game'}</p>
      <p>Capture: {$session.capture ? captureLabels[$session.capture.state] : 'Unavailable'}</p>
      {#if restartRequired}<p>Restart required</p>{/if}
      {#if map && !preview && !$session.setup?.demo}<p>Location: {map}</p>{:else}<DebugNote text="Location not yet observed." />{/if}

      <div class="actions">
        <button aria-disabled={startUnavailable} onclick={() => !startUnavailable && session.action(() => client.startCapture(), 'Couldn’t start capture. Retry.', { captureAction: true })}>Start Capture</button>
        <button aria-disabled={stopUnavailable} onclick={() => !stopUnavailable && session.action(() => client.stopCapture(), 'Couldn’t stop capture. Retry.', { duringExtraction: true, captureAction: true })}>Stop Capture</button>
      </div>
      {#if $session.capture}
        <dl aria-label="Capture Diagnostics">
          <div><dt>Dropped Frames</dt><dd>{$session.capture.dropped_frames}</dd></div>
          <div><dt>Unknown Packets</dt><dd>{$session.capture.unknown_packets}</dd></div>
          <div><dt>Decode Failures</dt><dd>{$session.capture.decode_failures}</dd></div>
        </dl>
      {/if}
    </SetupStep>
  </ol>
</div>
<style>
  .setup-page { display: grid; gap: var(--space-4); max-width: 65rem; min-width: 0; overflow-wrap: anywhere; font-size: var(--text-body); line-height: var(--leading-body); }
  ol { display: grid; gap: var(--space-6); margin: 0; padding: 0; }
  p { margin: var(--space-2) 0; color: var(--text-secondary); }
  .notice { color: var(--warning); }
  .error-summary { color: var(--danger); margin: 0; }
  .error-summary:empty { display: none; }
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); margin: var(--space-3) 0; }
  button { min-height: var(--target-min); }
  a { color: var(--violet-text); }
  .adapter { margin-top: var(--space-4); }
  .official-url { display: grid; gap: var(--space-2); }
  input { min-width: 0; width: 100%; font: inherit; color: var(--text-primary); background: var(--surface-canvas); border: 1px solid var(--border-strong); border-radius: var(--radius-control); padding: var(--space-3); }
  dl { display: flex; flex-wrap: wrap; gap: var(--space-4); margin-bottom: 0; }
  dt { color: var(--text-secondary); } dd { margin: 0; }
</style>

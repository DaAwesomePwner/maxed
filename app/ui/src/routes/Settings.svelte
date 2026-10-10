<script lang="ts">
  import { debugMode, setDebugMode } from '../lib/debug/debug';
  import { readable } from 'svelte/store';
  import type { CaptureStatus } from '../lib/bindings/CaptureStatus';
  import type { LiveSnapshot } from '../lib/bindings/LiveSnapshot';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import { productionFeatures, type Features } from '../lib/navigation/routes';
  import AlertPreferences from '../lib/alerts/AlertPreferences.svelte';
  import ChatSettingsSection from '../lib/logs/ChatSettingsSection.svelte';
  import { getContext } from 'svelte';
  import { SHELL_CONNECTION, type ShellConnection } from '../lib/shell/context';
  import type { Client } from '../lib/bridge/client';
  import type { AppSettings } from '../lib/bindings/AppSettings';
  import SettingsSection from '../lib/settings/SettingsSection.svelte';
  import RecordingControl from '../lib/settings/RecordingControl.svelte';
  import AdapterControl from '../lib/settings/AdapterControl.svelte';
  import UpdateControl from '../lib/settings/UpdateControl.svelte';
  import { createRecordingSession } from '../lib/settings/recording';
  import AssetsControl from '../lib/setup/AssetsControl.svelte';
  import { createSetupSession } from '../lib/setup/session';
  import { titleCase } from '../lib/styles/format';
  let { client, preview = false, features = productionFeatures, enabled = true, streamId = null, store = null }: {
    client: Client; preview?: boolean; features?: Features; enabled?: boolean; streamId?: string | null; store?: StoreStatus | null;
  } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  const capture = connection?.capture ?? readable<CaptureStatus | null>(null);
  const snapshot = connection?.snapshot ?? readable<LiveSnapshot | null>(null);
  let live = $derived(enabled && $capture?.state === 'Live' && $snapshot !== null
    && $snapshot.stream_id === streamId && streamId.startsWith('live-') && /^\d+$/.test($snapshot.at) && store?.state !== 'failed');
  let session = $derived(createSetupSession(client, preview, connection));
  $effect(() => { const current = session; current.start(); return () => current.dispose(); });
  const id = $props.id();
  let draft = $state<AppSettings | null>(null);
  let edited = $state(false);
  let revision = 0;
  let saved = $state('');
  let policyError = $state('');
  let adapter = $state('');
  let savedAdapter = $derived.by(() => {
    if (!$session.settings || $session.errors.settings) return 'Unavailable';
    if ($session.settings.adapter_id === null) return 'Auto';
    const selected = $session.adapters?.find((item) => item.id === $session.settings?.adapter_id);
    return selected ? selected.description || 'Unnamed Adapter' : 'Unknown Adapter';
  });
  let recordingSession = $derived(connection?.recording ?? createRecordingSession(client, preview));
  function refreshRecording() { return recordingSession.refresh(); }
  $effect(() => { const current = recordingSession; void current.refresh(); return () => { if (!connection?.recording) current.dispose(); }; });
  $effect(() => {
    if (!edited && $session.settings) {
      draft = { ...$session.settings };
      adapter = $session.settings.adapter_id ?? '';
    }
  });
  let readUnavailable = $derived(preview || $session.extracting || !!$session.errors.settings || $session.settings?.version !== 1);
  let unavailable = $derived(readUnavailable || $session.busy);
  let errorText = $derived([...Object.values($session.errors), policyError].filter(Boolean).join(' '));
  function changed() { revision++; edited = true; saved = ''; }
  function revert() {
    revision++; edited = false; saved = '';
    draft = $session.settings ? { ...$session.settings } : null;
    adapter = $session.settings?.adapter_id ?? '';
  }
  async function apply() {
    if (!draft || unavailable || !edited) return;
    const submitted = revision;
    const currentSession = session;
    const selected = { adapter_id: adapter || null, save_recordings: draft.save_recordings, update_channel: draft.update_channel, hardware_acceleration: draft.hardware_acceleration };
    saved = '';
    const accepted = await currentSession.action(async () => {
      // Preserve independently applied assets and alert policy. Never save guessed defaults.
      const latest = await client.querySettings();
      if (latest.version !== 1) throw new Error('Unavailable settings');
      if (selected.adapter_id !== latest.adapter_id) await client.setAdapter(selected.adapter_id);
      // Re-read after set_adapter so update_settings preserves its accepted selection.
      const current = await client.querySettings();
      if (current.version !== 1) throw new Error('Unavailable settings');
      await client.updateSettings({ ...current, save_recordings: selected.save_recordings, update_channel: selected.update_channel, hardware_acceleration: selected.hardware_acceleration });
    }, 'Couldn’t save. Retry.');
    if (!accepted || currentSession !== session) return;
    if (!$session.errors.settings && submitted === revision) {
      edited = false; saved = 'Saved';
    } else if (!$session.errors.settings) {
      saved = 'Unsaved Changes';
    }
    await Promise.all([refreshPolicy(), refreshRecording()]);
  }
  async function refreshPolicy() {
    try { await connection?.alerts?.refresh(); policyError = ''; }
    catch { policyError = 'Couldn’t refresh alerts. Retry.'; }
  }
  async function retrySettings() {
    await session.refreshSettings();
    if (!$session.errors.settings) await refreshPolicy();
  }
  let demoUnavailable = $derived(preview || $session.busy || $session.extracting || !$session.setup);
  async function toggleDemo() {
    if (demoUnavailable) return;
    saved = '';
    const enabled = !$session.setup?.demo;
    await session.action(() => client.setDemo(enabled), 'Couldn’t change demo. Retry.');
  }
</script>
<div class="settings-page">
  <a href="#/setup">Setup</a>
  {#if preview}<p class="notice">Preview</p>{/if}
  <p class="error-summary" role="alert" aria-atomic="true">{errorText}</p>
  {#if !draft}<p>Disconnected</p>{/if}
  {#if draft && draft.version !== 1}<p class="notice">Update Required</p>{/if}
  {#if $session.errors.settings || !draft}<button type="button" onclick={retrySettings}>Retry Settings</button>{/if}
  <div class="settings-grid">
    <div class="column">
      <SettingsSection heading="Capture">
        {#if draft}
          <div oninput={changed} onchange={changed}>
            <div class="adapter-row">
              <AdapterControl adapters={$session.adapters} {preview} bind:value={adapter} disabled={readUnavailable} />
              {#if !preview}<button type="button" disabled={$session.busy} onclick={() => session.refreshAdapters()}>Retry Adapters</button>{/if}
              <p>Saved: {savedAdapter}</p>
            </div>
            <RecordingControl session={recordingSession} bind:checked={draft.save_recordings} disabled={readUnavailable} demo={$session.setup?.demo ?? false} saved={$session.settings?.save_recordings ?? false} limit={draft.recording_limit_bytes} />
          </div>
        {/if}
      </SettingsSection>
      <SettingsSection heading="Chat">
        {#if draft}
          <ChatSettingsSection settings={draft.chat} update={(next) => client.updateChatSettings(next)} clear={() => client.clearChatHistory()} editable={!preview} />
        {/if}
      </SettingsSection>
      {#if features.logs}
        <SettingsSection heading="Alerts">
          <AlertPreferences {client} session={connection?.alerts} {preview} {enabled} {live} />
        </SettingsSection>
      {/if}
      <SettingsSection heading="Appearance">
        {#if draft}
          <label class="check"><input type="checkbox" bind:checked={draft.hardware_acceleration} onchange={changed} disabled={readUnavailable} aria-describedby={`${id}-acceleration-note`} /> Hardware Acceleration</label>
          <p>Saved: {$session.settings?.hardware_acceleration ? 'On' : 'Off'}</p>
        {/if}
        <p id={`${id}-acceleration-note`}><span>Restart required</span>. In Maxed's tests (2026-10-04, one Intel Core Ultra 7 laptop, WebView2 154),
          turning acceleration on added roughly 80–385 MB of memory. Results vary with hardware and how much history you view.</p>
      </SettingsSection>
    </div>
    <div class="column">
      <SettingsSection heading="Game Assets">
        <p>Assets: <strong>{$session.setup?.assets ? titleCase($session.setup.assets) : 'Unavailable'}</strong></p>
        <details>
          <summary>Assets Path</summary>
          <AssetsControl {session} {preview} />
        </details>
      </SettingsSection>
      <SettingsSection heading="Updates">
        {#if draft}
          <div class="channel-row">
            <label for={`${id}-channel`}>Update Channel</label>
            <select id={`${id}-channel`} bind:value={draft.update_channel} onchange={changed} disabled={readUnavailable}><option value="stable">Stable</option><option value="beta">Beta</option></select>
          </div>
        {/if}
        <UpdateControl {client} {preview} channel={$session.settings?.update_channel} capture={$session.capture} />
      </SettingsSection>
      <SettingsSection heading="Demo">
        <div class="demo-row"><button type="button" role="switch" aria-checked={$session.setup?.demo ?? false} aria-disabled={demoUnavailable} onclick={toggleDemo}>Demo Mode</button><span>Mode: {$session.setup ? $session.setup.demo ? 'Demo' : 'Live' : 'Unavailable'}</span></div>
      </SettingsSection>
      <SettingsSection heading="Debug">
        <label class="check"><input type="checkbox" checked={$debugMode} onchange={(event) => setDebugMode(event.currentTarget.checked)} /> Debug mode</label>
      </SettingsSection>
    </div>
  </div>
  <form class="save-bar" onsubmit={(event) => { event.preventDefault(); void apply(); }}>
    <div class="actions">
      <span>{edited ? 'Unsaved Changes' : draft && !$session.errors.settings ? 'Saved' : 'Disconnected'}</span>
      <button type="submit" aria-disabled={unavailable || !edited}>Apply</button>
      <button type="button" disabled={$session.busy || !edited} onclick={revert}>Revert</button>
      <a href={features.logs ? "#/logs/alerts" : "#/alerts"} aria-label={features.logs ? "Alert history" : "Alerts"}>{features.logs ? "Alert history" : "Alerts"}</a>
    </div>
    <p role="status" aria-live="polite">{saved}</p>
    <details>
      <summary>Recovery</summary>
      <div class="actions">
        {#if draft && !$session.errors.settings}<button type="button" onclick={retrySettings}>Retry Settings</button>{/if}
        <button type="button" onclick={() => session.refreshSetup({ retry: true })}>Retry Checks</button>
      </div>
    </details>
  </form>
</div>
<style>
  .settings-page { display: grid; gap: var(--space-2); min-width: 0; overflow-wrap: anywhere; font-size: var(--text-label); line-height: var(--leading-label); container-type: inline-size; }
  .settings-grid { display: grid; grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr); gap: var(--space-2); align-items: start; }
  .column { display: grid; gap: var(--space-2); min-width: 0; }
  p { color: var(--text-secondary); margin: var(--space-1) 0; line-height: var(--leading-label); }
  .notice { color: var(--warning); }
  .error-summary { color: var(--danger); margin: 0; }
  .error-summary:empty, .save-bar p:empty { display: none; }
  .actions, .demo-row, .channel-row { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); }
  .actions > span { margin-right: auto; }
  .adapter-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--space-1) var(--space-2); align-items: end; }
  .adapter-row :global(label), .adapter-row :global(p) { grid-column: 1 / -1; }
  button, select, .actions a { min-height: var(--target-min); font-size: var(--text-label); }
  button { padding: var(--space-1) var(--space-2); }
  a { color: var(--violet-text); }
  .actions a { display: inline-flex; align-items: center; }
  .check { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); min-height: var(--target-min); font-size: var(--text-body); }
  input[type="checkbox"] { width: 1.25rem; height: 1.25rem; accent-color: var(--violet); }
  select { min-width: 0; padding: var(--space-1) var(--space-2); color: var(--text-primary); background: var(--surface-canvas); border: 1px solid var(--border-strong); border-radius: var(--radius-control); font: inherit; }
  summary { min-height: var(--target-min); align-content: center; color: var(--violet-text); cursor: pointer; }
  summary:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  .save-bar { position: sticky; bottom: 0; min-width: 0; padding: var(--space-2) var(--space-3); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-card); }
  @container (max-width: 38rem) { .settings-grid { grid-template-columns: minmax(0, 1fr); } .save-bar { position: static; } }
</style>

<script lang="ts">
  import '../lib/styles/controls.css';
  import SettingsTabs from '../lib/settings/SettingsTabs.svelte';
  import StorageUsage from '../lib/settings/StorageUsage.svelte';
  import { debugMode, setDebugMode } from '../lib/debug/debug';
  import { readable } from 'svelte/store';
  import type { CaptureStatus } from '../lib/bindings/CaptureStatus';
  import type { LiveSnapshot } from '../lib/bindings/LiveSnapshot';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import { productionFeatures, type Features, type SettingsTab } from '../lib/navigation/routes';
  import AlertPreferences from '../lib/alerts/AlertPreferences.svelte';
  import ChatSettingsSection from '../lib/logs/ChatSettingsSection.svelte';
  import ClearHistoryDialog from '../lib/party/ClearHistoryDialog.svelte';
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
  import { historyRunning } from '../lib/settings/storage-state';
  import { titleCase } from '../lib/styles/format';
  import { errorLine } from '../lib/setup/presentation';
  let { client, tab, preview = false, features = productionFeatures, enabled = true, streamId = null, store = null }: {
    client: Client; tab?: SettingsTab; preview?: boolean; features?: Features; enabled?: boolean; streamId?: string | null; store?: StoreStatus | null;
  } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  const capture = connection?.capture ?? readable<CaptureStatus | null>(null);
  const snapshot = connection?.snapshot ?? readable<LiveSnapshot | null>(null);
  let live = $derived(enabled && $capture?.state === 'Live' && $snapshot !== null
    && $snapshot.stream_id === streamId && streamId.startsWith('live-') && /^\d+$/.test($snapshot.at) && store?.state !== 'failed');
  let session = $derived(createSetupSession(client, preview, connection));
  let partyClearHint = $derived(preview ? 'Clearing is not available in preview.'
    : $session.errors.setup ? 'Couldn’t check demo mode. Reload to try again.'
    : $session.setup?.demo ? 'Turn off demo mode to clear your party history.' : '');
  $effect(() => { const current = session; current.start(); return () => current.dispose(); });
  const id = $props.id();
  let draft = $state<AppSettings | null>(null);
  let edited = $state(false);
  let revision = 0;
  let saved = $state('');
  let partyClearOpen = $state(false), partyClearMessage = $state('');
  let partyClearButton: HTMLButtonElement;
  function closePartyClear() {
    if (partyClearOpen) partyClearMessage = '';
    partyClearOpen = false; partyClearButton?.focus();
  }
  let policyError = $state('');
  let adapter = $state('');
  let savedAdapter = $derived.by(() => {
    if (!$session.settings || $session.errors.settings) return 'Unavailable';
    if ($session.settings.adapter_id === null) return 'Auto';
    const selected = $session.adapters?.find((item) => item.id === $session.settings?.adapter_id);
    return selected ? selected.description || 'Unnamed adapter' : 'Unknown adapter';
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
  // Settings panels stay mounted while hidden, so every failure stays in the always-visible top list.
  let errors = $derived([...Object.entries($session.errors), ['policy', policyError] as [string, string]].filter(([, message]) => message));
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
      saved = 'Unsaved changes';
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
  let upgrading = $derived($session.upgrading || historyRunning($session.history));
  let demoUnavailable = $derived(upgrading || $session.historyChecking || preview || $session.busy || $session.extracting || !$session.setup);
  async function toggleDemo() {
    if (demoUnavailable) return;
    saved = '';
    const enabled = !$session.setup?.demo;
    await session.action(() => client.setDemo(enabled), 'Couldn’t change demo. Retry.');
  }
</script>
<div class="settings-page">
  <a class="setup-link ctl" href="#/setup">Setup</a>
  {#if preview}<p class="notice">Preview</p>{/if}
  <div class="error-summary" role="alert" aria-atomic="true">{#if errors.length}<ul>
    {#each errors as [key, message] (key)}<li><span>{errorLine(message)}</span>
      {#if key === 'settings'}<button class="ctl" type="button" onclick={retrySettings}>Retry settings</button>{/if}</li>{/each}
  </ul>{/if}</div>
  {#if upgrading}<p class="notice">Demo mode is unavailable while the history upgrade is running.</p>{/if}
  {#if !draft}<p>Disconnected</p>{/if}
  {#if draft && draft.version !== 1}<p class="notice">Update required</p>{/if}
  {#if !draft && !$session.errors.settings}<button class="ctl retry" type="button" onclick={retrySettings}>Retry settings</button>{/if}
  <SettingsTabs requestedTab={tab} notificationsEnabled={features.logs}>
    {#snippet general()}
      <div class="settings-grid">
        <SettingsSection heading="Capture">
          {#if draft}
            <div oninput={changed} onchange={changed}>
              <div class="adapter-row">
                <AdapterControl adapters={$session.adapters} {preview} bind:value={adapter} disabled={readUnavailable} />
                {#if !preview}<button class="ctl" type="button" disabled={$session.busy} onclick={() => session.refreshAdapters()}>Retry adapters</button>{/if}
                <p>Current adapter: {savedAdapter}</p>
              </div>
            </div>
          {/if}
        </SettingsSection>
        <SettingsSection heading="Appearance">
          {#if draft}
            <label class="check"><input type="checkbox" bind:checked={draft.hardware_acceleration} onchange={changed} disabled={readUnavailable} aria-describedby={`${id}-acceleration-note`} /> Hardware acceleration</label>
            <p>Current setting: {$session.settings?.hardware_acceleration ? 'On' : 'Off'}</p>
          {/if}
          <p id={`${id}-acceleration-note`}><span>Restart required</span>. In Maxed's tests (2026-10-04, one Intel Core Ultra 7 laptop, WebView2 154),
            turning acceleration on added roughly 80–385 MB of memory. Results vary with hardware and how much history you view.</p>
        </SettingsSection>
        <SettingsSection heading="Updates">
          {#if draft}
            <div class="channel-row">
              <label for={`${id}-channel`}>Update channel</label>
              <select class="ctl" id={`${id}-channel`} bind:value={draft.update_channel} onchange={changed} disabled={readUnavailable}><option value="stable">Stable</option><option value="beta">Beta</option></select>
            </div>
          {/if}
          <UpdateControl {client} {preview} channel={$session.settings?.update_channel} capture={$session.capture} />
        </SettingsSection>
        <SettingsSection heading="Demo">
          <div class="demo-row"><button class="ctl" type="button" role="switch" aria-checked={$session.setup?.demo ?? false} aria-disabled={demoUnavailable} onclick={toggleDemo}>Demo mode</button><span>Mode: {$session.setup ? $session.setup.demo ? 'Demo' : 'Live' : 'Unavailable'}</span></div>
        </SettingsSection>
        <SettingsSection heading="Debug">
          <label class="check"><input type="checkbox" checked={$debugMode} onchange={(event) => setDebugMode(event.currentTarget.checked)} /> Debug mode</label>
        </SettingsSection>
      </div>
    {/snippet}
    {#snippet notifications()}
      <SettingsSection heading="Alerts">
        <AlertPreferences {client} session={connection?.alerts} {preview} {enabled} {live} />
      </SettingsSection>
    {/snippet}
    {#snippet data()}
      <div class="settings-grid">
        <SettingsSection heading="Storage">
          <StorageUsage {client} {preview} />
          {#if draft}
            <div oninput={changed} onchange={changed}>
              <RecordingControl session={recordingSession} bind:checked={draft.save_recordings} disabled={readUnavailable} demo={$session.setup?.demo ?? false} saved={$session.settings?.save_recordings ?? false} limit={draft.recording_limit_bytes} />
            </div>
          {/if}
        </SettingsSection>
        <SettingsSection heading="Chat">
          {#if draft}
            <ChatSettingsSection settings={draft.chat} update={(next) => client.updateChatSettings(next)} clear={() => client.clearChatHistory()} editable={!preview} />
          {/if}
        </SettingsSection>
        <SettingsSection heading="Party history">
          <p>Maxed links players you've seen across runs by name. This removes that history and other players' stored gear and stats from your runs.</p>
          <button class="ctl" type="button" bind:this={partyClearButton} disabled={preview || !$session.setup || $session.setup.demo}
            aria-describedby={partyClearHint ? `${id}-party-clear-hint` : undefined}
            onclick={() => { partyClearMessage = ''; partyClearOpen = true; }}>Clear party history</button>
          {#if partyClearHint}<p class="hint" id={`${id}-party-clear-hint`}>{partyClearHint}</p>{/if}
          <p role="status" aria-live="polite">{partyClearMessage}</p>
        </SettingsSection>
      </div>
    {/snippet}
    {#snippet assets()}
      <SettingsSection heading="Game assets">
        <p>Assets: <strong>{$session.setup?.assets ? titleCase($session.setup.assets) : 'Unavailable'}</strong></p>
        <details>
          <summary>Assets path</summary>
          <AssetsControl {session} {preview} />
        </details>
      </SettingsSection>
    {/snippet}
  </SettingsTabs>
  <form class="save-bar" onsubmit={(event) => { event.preventDefault(); void apply(); }}>
    <div class="actions">
      <span role="status" aria-live="polite">{saved || (edited ? 'Unsaved changes' : draft && !$session.errors.settings ? 'Saved' : 'Disconnected')}</span>
      <button class="ctl" type="submit" aria-disabled={unavailable || !edited}>Apply</button>
      <button class="ctl" type="button" disabled={$session.busy || !edited} onclick={revert}>Revert</button>
      <a href={features.logs ? "#/logs/alerts" : "#/alerts"} aria-label={features.logs ? "Alert history" : "Alerts"}>{features.logs ? "Alert history" : "Alerts"}</a>
    </div>
    <details>
      <summary>Recovery</summary>
      <div class="actions">
        {#if draft && !$session.errors.settings}<button class="ctl" type="button" onclick={retrySettings}>Retry settings</button>{/if}
        <button class="ctl" type="button" onclick={() => session.refreshSetup({ retry: true })}>Retry checks</button>
      </div>
    </details>
  </form>
  {#if partyClearOpen}<ClearHistoryDialog {client} onCancel={closePartyClear}
    onError={message => { partyClearMessage = message; }}
    onCleared={() => { closePartyClear(); partyClearMessage = 'Party history cleared'; }} />{/if}
</div>
<style>
  .settings-page :global(.preferences) { padding: 0; border: 0; background: transparent; }
  .hint { color: var(--text-muted); font-size: var(--text-caption); line-height: var(--leading-caption); }
  .settings-page { display: grid; gap: var(--space-3); min-width: 0; overflow-wrap: anywhere; font-size: var(--text-label); line-height: var(--leading-label); container-type: inline-size; }
  .settings-grid { display: grid; grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
  p { color: var(--text-secondary); margin: var(--space-1) 0; line-height: var(--leading-label); }
  .notice { color: var(--warning); }
  .error-summary:empty { display: none; }
  .error-summary ul { display: grid; gap: var(--space-2); margin: 0; padding: 0; list-style: none; }
  .error-summary li { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2) var(--space-3); min-height: var(--control-dense); color: var(--danger); }
  .actions, .demo-row, .channel-row { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); }
  .channel-row label { color: var(--text-secondary); }
  .actions > span { margin-right: auto; color: var(--text-secondary); }
  .adapter-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--space-1) var(--space-2); align-items: end; }
  .adapter-row :global(label), .adapter-row :global(p) { grid-column: 1 / -1; }
  .adapter-row button { justify-self: start; }
  .setup-link, .retry { justify-self: start; }
  a { color: var(--text-primary); text-decoration: none; text-underline-offset: 3px; }
  a:hover, a:focus-visible { text-decoration: underline; }
  .actions a { display: inline-flex; align-items: center; min-height: var(--control-dense); padding: 0 var(--space-2); color: var(--text-secondary); }
  .actions a:hover { color: var(--text-primary); }
  .check { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); min-height: var(--target-min); font-size: var(--text-label); }
  input[type="checkbox"] { width: 1rem; height: 1rem; margin: 0; accent-color: var(--text-secondary); }
  select { min-width: 0; }
  strong { font-weight: var(--weight-medium); color: var(--text-primary); }
  summary { min-height: var(--target-min); align-content: center; color: var(--text-secondary); cursor: pointer; }
  summary:hover { color: var(--text-primary); }
  summary:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  .save-bar { position: static; min-width: 0; padding: var(--space-3) var(--space-4); background: var(--surface-panel); border: 1px solid var(--hairline); border-radius: var(--radius-card); }
  @container (max-width: 38rem) { .settings-grid { grid-template-columns: minmax(0, 1fr); } }
</style>

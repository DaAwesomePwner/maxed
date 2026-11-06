<script lang="ts">
  import DungeonPicker from './DungeonPicker.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import DebugNote from '../debug/DebugNote.svelte';
  import { untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { AlertKind } from '../bindings/AlertKind';
  import type { AlertLoudness } from '../bindings/AlertLoudness';
  import type { AlertDeliveryStatus } from '../bindings/AlertDeliveryStatus';
  import type { NotificationPermission } from '../bindings/NotificationPermission';
  import Badge from '../components/Badge.svelte';
  import type { AlertSession, AlertSessionStatus } from './session';
  import { readable } from 'svelte/store';
  import { alertKindDisplayOrder, alertKindLabels } from './filters';
  import { createAlertPreferences, emptyAlertPreferences, loudnessChoices, loudnessLabels } from './preferences';
  const permissionLabels: Record<NotificationPermission, string> = {
    unknown: 'Unknown', granted: 'Granted', denied: 'Denied', unavailable: 'Unsupported',
  };
  let { client, session, preview = false, enabled = true, live = false }: {
    client?: Client; session?: AlertSession; preview?: boolean; enabled?: boolean; live?: boolean;
  } = $props();
  let editor = $state.raw<ReturnType<typeof createAlertPreferences>>();
  let preferencesState = $state.raw(emptyAlertPreferences());
  const disconnected = readable<AlertSessionStatus>('retired');
  let sessionStatus = $derived(session?.status ?? disconnected);
  let delivery = $state.raw<AlertDeliveryStatus | null>(null);
  let demo = $state<boolean | null>(null), checking = $state(false), testing = $state(false);
  let deliveryMessage = $state('');
  let generation = 0;
  let testRevision = 0;
  let editable = $derived(enabled && !preview && demo === false && !!session);
  let testAvailable = $derived(editable && live
    && (delivery?.sound_available === true || delivery?.permission === 'granted') && !testing && !checking);
  const supportedKinds = alertKindDisplayOrder.filter(kind => kind !== 'death');
  let notificationTestKind = $derived(supportedKinds.find(kind => kind !== 'flawless_run'
    && ['toast', 'banner'].includes(preferencesState.saved?.[kind] ?? 'off')));
  $effect(() => {
    // Retire an in-flight sample result when eligibility changes without polling.
    const eligible = live && enabled && !preview;
    void eligible;
    testRevision++; testing = false; deliveryMessage = '';
  });
  $effect(() => {
    const owner = session;
    if (!owner) { preferencesState = emptyAlertPreferences(); editor = undefined; return; }
    const instance = createAlertPreferences(owner, next => { preferencesState = next; }, () => editable);
    editor = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const owner = client, active = enabled;
    generation++; demo = null; delivery = null; deliveryMessage = ''; checking = false; testing = false;
    if (owner && active) untrack(() => { void refreshDelivery(); });
    return () => { generation++; };
  });
  async function refreshDelivery() {
    if (!client || !enabled || checking) return;
    const token = generation, owner = client;
    checking = true; deliveryMessage = '';
    const [status, setup] = await Promise.allSettled([owner.queryAlertDelivery(), owner.querySetup()]);
    if (token !== generation) return;
    delivery = status.status === 'fulfilled' ? status.value : null;
    demo = setup.status === 'fulfilled' ? setup.value.demo : null;
    if (status.status === 'rejected' || setup.status === 'rejected') deliveryMessage = 'Couldn’t load delivery. Retry.';
    checking = false;
  }
  async function test(kind: AlertKind) {
    if (!client || !testAvailable || kind === 'flawless_run') return;
    const owner = client, token = generation, sample = testRevision;
    testing = true; deliveryMessage = '';
    try {
      const result = await owner.testAlertDelivery(kind);
      if (token === generation && sample === testRevision) {
        delivery = result;
        deliveryMessage = 'Test Sent';
      }
    } catch {
      if (token === generation && sample === testRevision) deliveryMessage = 'Couldn’t test. Retry live.';
    } finally { if (token === generation && sample === testRevision) testing = false; }
  }
  function choose(kind: AlertKind, value: string) {
    if (loudnessChoices.includes(value as AlertLoudness)) editor?.edit({ [kind]: value as AlertLoudness });
  }
</script>

<BusyStatus message={enabled && $sessionStatus === 'loading' ? 'Loading alert preferences'
  : enabled && $sessionStatus === 'saving' ? 'Updating alert preferences' : null} />
<section class="preferences" aria-label="Alerts">
  <h2>Presentation</h2>
  {#if $sessionStatus === 'retired'}<p>Disconnected</p>{/if}
  {#if $sessionStatus === 'failed'}
    <p>Couldn't load alert preferences. Retry.</p>
    <button class="ctl" type="button" disabled={!enabled || preferencesState.busy} onclick={() => { void editor?.refresh(); }}>Retry Preferences</button>
  {/if}
  {#if preview || demo === true}<p>Preview</p>
  {:else if demo === null}<p class="note">Disconnected</p>{/if}
  <form onsubmit={(event) => { event.preventDefault(); void editor?.apply(); }}>
    {#each supportedKinds as kind}
      <section class:key-pop={kind === 'key_popped'} aria-label={kind === 'key_popped' ? 'Key Popped' : undefined}>
        {#if kind === 'key_popped'}<h3>Key Popped</h3>{/if}
        <div class="kind-row">
          <label for={`alert-${kind}`}>{alertKindLabels[kind]}</label>
          <select class="ctl" id={`alert-${kind}`} value={preferencesState.draft?.[kind] ?? ''} disabled={!editable || !preferencesState.draft || kind === 'flawless_run'} onchange={(event) => choose(kind, event.currentTarget.value)}>
            {#if !preferencesState.draft}<option value="">Unavailable</option>{/if}
            {#each loudnessChoices as choice}<option value={choice}>{loudnessLabels[choice]}</option>{/each}
          </select>
          <button class="ctl" type="button" aria-label={`Test ${alertKindLabels[kind]}`} disabled={!testAvailable || kind === 'flawless_run'} onclick={() => { void test(kind); }}>Test</button>
        </div>
        {#if kind === 'key_popped' && preferencesState.draft}
          <p class="note">All observed pops remain in Logs. These choices control alerts only.</p>
          {#if preferencesState.draft.key_popped === 'off'}<p class="note">Key-pop alerts are off. Choose dungeons, then select Toast or Banner and save to enable them.</p>{/if}
          <DungeonPicker {client} preferences={preferencesState.draft} disabled={!editable} onEdit={patch => editor?.edit(patch)} />
        {/if}
      </section>
    {/each}
    <DebugNote text={delivery?.flawless_unavailable ?? 'Damage-taken evidence unavailable.'} />
    <p class="note">Sound: {delivery === null ? '—' : delivery.sound_available ? 'Available' : 'Unavailable'}{#if !live}<DebugNote inline>Tests require live capture.</DebugNote>{/if}</p>
    <label class="mute"><input type="checkbox" checked={preferencesState.draft?.muted ?? false} disabled={!editable || !preferencesState.draft} onchange={(event) => editor?.edit({ muted: event.currentTarget.checked })} /> Mute Sounds</label>
    <label class="volume">Volume<input type="range" min="0" max="100" step="1" value={preferencesState.draft?.volume_percent ?? 0} disabled={!editable || !preferencesState.draft} oninput={(event) => editor?.edit({ volume_percent: Number(event.currentTarget.value) })} /><span>{preferencesState.draft ? `${preferencesState.draft.volume_percent}%` : 'Unavailable'}</span></label>
    <div class="actions"><button class="ctl" type="submit" disabled={!editable || !preferencesState.saved || !preferencesState.dirty || preferencesState.busy}>Save</button><button class="ctl" type="button" disabled={!preferencesState.dirty} onclick={() => editor?.revert()}>Revert</button></div>
    <p class="note" role="status">{(preferencesState.message === 'Saved' ? 'Alert preferences saved' : preferencesState.message) || (preferencesState.dirty ? 'Unsaved Changes' : '')}</p>
  </form>
  <div class="windows-row"><label><input type="checkbox" disabled={!editable || !preferencesState.draft || (delivery?.permission !== 'granted' && !preferencesState.draft.windows_notifications)} checked={preferencesState.draft?.windows_notifications ?? false} onchange={(event) => editor?.edit({ windows_notifications: event.currentTarget.checked })} /> Windows Notifications</label><Badge label={delivery?.permission === 'granted' ? 'Available' : 'Unavailable'} /></div>
  <button class="ctl" type="button" disabled={!testAvailable || !notificationTestKind || delivery?.permission !== 'granted' || !preferencesState.saved?.windows_notifications} onclick={() => { if (notificationTestKind) void test(notificationTestKind); }}>Test Notification</button>
  <details>
    <summary>Delivery</summary>
    <p>Sound: {delivery === null ? '—' : delivery.sound_available ? 'Available' : 'Unavailable'}.</p>
    {#if delivery?.message}<DebugNote text={delivery.message} />{/if}
    <DebugNote text={live ? 'Tests use saved preferences and the native live-capture guard.' : 'Sound tests require live capture and an observed capture clock.'} />
    {#if delivery}<p>Permission: {permissionLabels[delivery.permission]}</p>{/if}
    <div class="actions">
      {#if client}<button class="ctl" type="button" disabled={checking || !enabled} onclick={() => { void refreshDelivery(); }}>{delivery === null || demo === null ? 'Retry Delivery' : 'Refresh Delivery'}</button>{/if}
      {#if session && $sessionStatus !== 'failed'}<button class="ctl" type="button" disabled={preferencesState.busy || !enabled || $sessionStatus === 'retired' || $sessionStatus === 'loading' || $sessionStatus === 'saving'} onclick={() => { void editor?.refresh(); }}>{preferencesState.saved === null ? 'Retry Preferences' : 'Refresh Preferences'}</button>{/if}
    </div>
  </details>
  <p class="note" role="status">{deliveryMessage}</p>
</section>

<style>
  .key-pop { border: 1px solid var(--border); border-radius: var(--radius-card); padding: var(--space-2); margin: var(--space-2) 0; }
  h3 { font-size: var(--text-body); margin: 0; }
  .preferences { padding: var(--space-3); background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-card); min-width: 0; overflow-wrap: anywhere; }
  h2 { font-size: var(--text-body); margin: 0 0 var(--space-2); }
  p { margin: var(--space-1) 0; }
  .note, details { font-size: var(--text-label); color: var(--text-secondary); }
  .kind-row { display: grid; grid-template-columns: minmax(5rem, 1fr) minmax(0, 8rem) auto; gap: var(--space-2); align-items: center; padding: var(--space-1) 0; border-bottom: 1px solid var(--border); }
  .kind-row label { font-size: var(--text-label); }
  button, select { color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-control); font: inherit; min-height: var(--target-min); padding: var(--space-1) var(--space-2); min-width: 0; max-width: 100%; }
  select { font-size: var(--text-label); }
  button:disabled, select:disabled { color: var(--text-disabled); }
  button:not(:disabled), select:not(:disabled), summary { cursor: pointer; }
  .mute, .volume { display: flex; gap: var(--space-2); align-items: center; min-height: var(--target-min); }
  .windows-row { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); min-height: var(--target-min); margin-top: var(--space-2); }
  .volume { font-size: var(--text-label); }
  .volume input { flex: 1; min-width: 0; min-height: var(--target-min); }
  input { accent-color: var(--violet); }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); margin-top: var(--space-2); }
  summary { min-height: var(--target-min); align-content: center; color: var(--violet-text); }
  @media (max-width: 440px) { .kind-row { grid-template-columns: 1fr minmax(0, 8rem); } .kind-row button { grid-column: 2; } }
</style>

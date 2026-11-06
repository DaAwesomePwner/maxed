<script lang="ts">
  import type { SetupSession } from './session';
  import type { StorageFailure } from '../bindings/StorageFailure';
  import { backupSpaceReason, historyRunning, historyUpgradeRequired, storageBytes, storageReasons } from '../settings/storage-state';
  let { session, preview = false }: { session: SetupSession; preview?: boolean } = $props();
  let running = $derived($session.upgrading || historyRunning($session.history));
  let required = $derived(historyUpgradeRequired($session.history));
  let spaceReason = $derived(backupSpaceReason($session.history));
  let unavailable = $derived(preview || running || $session.historyChecking || $session.busy || $session.extracting
    || !$session.history || !!$session.history.failure || !!spaceReason);
  function failureText(failure: StorageFailure) {
    if (failure === 'cancelled') return 'Upgrade cancelled. Back up and upgrade when ready.';
    if (failure === 'owner_poisoned' || failure === 'blocked_stream') return 'The backup needs attention. Retry status before requesting another backup.';
    return storageReasons[failure];
  }
</script>

{#if !preview && $session.history?.phase !== 'ready'}
  <section aria-label="History upgrade" class="history-upgrade">
    <h2>{required ? 'History upgrade required' : $session.history || $session.errors.history ? "Couldn't check saved history." : 'Checking history status…'}</h2>
    {#if !required}
      <button class="ctl" disabled={$session.historyChecking} onclick={() => session.retryHistory()}>Retry history status and space</button>
    {:else if $session.historyDeferred}
      <p>History is unchanged. Live capture stays blocked until the upgrade succeeds.</p>
      <button class="ctl" onclick={() => session.reviewHistory()}>Review history upgrade</button>
    {:else}
      <p>Your saved history needs an upgrade before live capture can start. Maxed first makes and verifies a backup in the initial backup slot. This backup is kept until you deliberately delete it in Settings.</p>
      <dl>
        <div><dt>Required backup space</dt><dd>{storageBytes($session.history?.required_bytes)}</dd></div>
        <div><dt>Free space</dt><dd>{storageBytes($session.history?.free_bytes)}</dd></div>
      </dl>
      <p class="location">Backup location: {$session.backupLocation ?? 'Unavailable'}</p>
      {#if spaceReason}<p>{spaceReason}</p>{/if}
      {#if $session.history?.failure && failureText($session.history.failure) !== spaceReason}<p role="status">{failureText($session.history.failure)}</p>{/if}
      {#if $session.history?.last_failure}<p role="status">Previous attempt: {failureText($session.history.last_failure)}</p>{/if}
      {#if running}
        <p role="status">{$session.history?.phase === 'upgrading' ? 'Upgrading history after the verified backup.' : 'Backing up history before upgrading.'}</p>
        <p>Capture, Stop, and Demo Mode are unavailable while the history upgrade is running.</p>
      {/if}
      <div class="actions">
        <button class="ctl" disabled={unavailable} onclick={() => session.upgradeHistory()}>Back up and upgrade</button>
        <button class="ctl" disabled={running} onclick={() => session.deferHistory()}>Not now</button>
        <button class="ctl" disabled={$session.historyChecking} onclick={() => session.retryHistory()}>Retry history status and space</button>
      </div>
      <p>Insufficient or unavailable space keeps history unchanged and live capture blocked. Free space, then retry.</p>
    {/if}
  </section>
{/if}
<style>
  .history-upgrade { border: 1px solid var(--border-strong); border-radius: var(--radius-card); padding: var(--space-4); min-width: 0; overflow-wrap: anywhere; }
  h2 { font-size: 16px; margin: 0 0 var(--space-3); }
  p, dt { color: var(--text-secondary); } p { margin: var(--space-2) 0; }
  dl { display: flex; flex-wrap: wrap; gap: var(--space-4); } dd { margin: 0; font-variant-numeric: tabular-nums; }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); margin: var(--space-3) 0; }
</style>

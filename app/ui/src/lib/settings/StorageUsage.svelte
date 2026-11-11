<script lang="ts">
  import { errorLine } from '../setup/presentation';
  import type { Client } from '../bridge/client';
  import { createStorageState, storageBytes, storageCount, storageTime, storageReasons, backupSpaceReason, historyUpgradeRequired } from './storage-state';
  import type { StorageFailure } from '../bindings/StorageFailure';
  import type { ManagedStorageBackup } from '../bindings/ManagedStorageBackup';
  import type { StorageMaintenancePhase } from '../bindings/StorageMaintenancePhase';
  let { client, preview = false }: { client: Client; preview?: boolean } = $props();
  let session = $derived(createStorageState(client, preview));
  $effect(() => { const current = session; current.start(); return () => current.dispose(); });
  let review = $state(false), deleting = $state<ManagedStorageBackup | null>(null);
  $effect(() => { void session; review = false; deleting = null; });
  let spaceReason = $derived(backupSpaceReason($session.space));
  let absent = $derived($session.status?.database.state === 'absent');
  const terminalFailures: StorageFailure[] = ['space_unknown', 'insufficient_space', 'io', 'full', 'verification_failed', 'superseded', 'invalid_unit', 'unavailable'];
  let jobs = $derived($session.maintenance?.jobs ?? []);
  let resumable = $derived(jobs.some(job => job.needs_resume));
  let active = $derived(jobs.some(job => job.phase !== 'reclaimable' && !job.needs_resume));
  let compactUnavailable = $derived(preview || absent || $session.busy || $session.loading || !!spaceReason || $session.space?.phase !== 'ready');
  const phases: Record<StorageMaintenancePhase, string> = {
    awaiting_backup: 'Backing up before optimization', copying_receipts: 'Optimizing receipts: copy, verify, and remove legacy rows',
    verifying: 'Verifying receipts', deleting: 'Removing verified legacy rows', reclaimable: 'Receipt optimization complete',
  };
  let measures = $derived([
    ['Database', $session.status?.database.main_bytes], ['WAL', $session.status?.database.wal_bytes],
    ['SHM', $session.status?.database.shm_bytes], ['Reusable pages', $session.status?.database.reusable_bytes],
    ['Retained backups', $session.status?.backups.retained_bytes], ['Temporary backups', $session.status?.backups.temporary_bytes],
    ['Recordings on disk', $session.status?.recordings.physical_bytes], ['Recordings accounted', $session.status?.recordings.accounted_bytes],
    ['Recording quota', $session.status?.recordings.quota_bytes],
  ]);
  async function compact() {
    if (compactUnavailable) return;
    const owner = session;
    if (await owner.command('compact') && owner === session) review = false;
  }
  async function remove() {
    if (!deleting) return;
    const owner = session;
    if (await owner.deleteBackup(deleting.id) && owner === session) deleting = null;
  }
</script>
<div class="storage-card">
  {#if preview}<p class="note">Unavailable in preview</p>{/if}
  <dl class="measures">
    {#each measures as [label, bytes]}<div><dt>{label}</dt><dd title={bytes == null ? undefined : `${bytes} bytes`}>{storageBytes(bytes)}</dd></div>{/each}
  </dl>
  <p class="note">Live history sizes may change while capturing. The database file size may stay unchanged while pages become reusable for new history. Recording storage is separate.</p>
  <div class="actions">
    <button class="ctl" disabled={preview || $session.loading || $session.busy} onclick={() => session.retry()}>Refresh storage</button>
    <button class="ctl" disabled={preview || $session.loading || $session.busy} onclick={() => session.refresh(true)}>Estimate receipt storage</button>
  </div>
  <p class="note">Legacy receipt estimate: {storageBytes($session.status?.receipt_estimate.bytes)}. Measured: {storageTime($session.status?.receipt_estimate.measured_at)}.</p>
  <p class="note">Last successful maintenance: {storageTime($session.maintenance?.last_success_at)}</p>
  {#if $session.maintenance === null}<p class="note">Maintenance: Unavailable</p>{/if}
  {#each jobs as job}
    <div class="progress" role="status">
      <strong>{phases[job.phase]}</strong>
      {#if job.paused}<span>Paused</span>{/if}
      {#if job.reason}<p>{storageReasons[job.reason]}</p>{/if}
      <p>Receipts processed: {storageCount(job.counters.processed_records)?.toLocaleString('en-US') ?? 'Unavailable'};
        eligible: {storageCount(job.counters.eligible_records)?.toLocaleString('en-US') ?? 'Unavailable'};
        blocked: {storageCount(job.counters.blocked_records)?.toLocaleString('en-US') ?? 'Unavailable'}.</p>
      <p>Processed receipt bytes: {storageBytes(job.counters.processed_bytes)}. These are not measured disk savings.</p>
    </div>
  {/each}
  {#if $session.failure}<p class="note" role="status">{storageReasons[$session.failure]}</p>{/if}
  {#if $session.failure && terminalFailures.includes($session.failure) && !resumable}<p class="note">After resolving the problem, use Compact now to request another backup and retry.</p>{/if}
  <p>Lossless receipt optimization keeps your history and receipt retry checks. A verified backup is required before legacy receipt rows are replaced.</p>
  <p>A ~600 MB backup may initially cost more disk than the estimated 55–70 MB reusable-page gain. These are planning estimates; your database may differ. There is no promise of net disk savings while the backup remains.</p>
  <dl class="measures">
    <div><dt>Required backup space</dt><dd>{storageBytes($session.space?.required_bytes)}</dd></div>
    <div><dt>Free space</dt><dd>{storageBytes($session.space?.free_bytes)}</dd></div>
  </dl>
  <p class="location">Backup location: {$session.location ?? 'Unavailable'}</p>
  {#if absent}<p class="note">There is no saved history to optimize yet.</p>
  {:else if !preview && spaceReason}<p class="note">{spaceReason}</p>{/if}
  {#if historyUpgradeRequired($session.space)}<p class="note">Complete the history upgrade in Setup before optimizing receipts.</p>{/if}
  <div class="actions">
    <button class="ctl" disabled={compactUnavailable} onclick={() => { review = true; }}>Compact now</button>
    {#if resumable}<button class="ctl" disabled={preview || $session.busy} onclick={() => session.command('resume')}>Resume maintenance</button>{/if}
    {#if active}<button class="ctl" disabled={preview || $session.busy} onclick={() => session.command('cancel')}>Cancel maintenance</button>{/if}
  </div>
  {#if review}
    <div class="review" role="group" aria-label="Review receipt optimization">
      <p>Back up, then optimize receipts. All fight detail is kept. Requested work continues after you leave Settings; cancellation takes effect between batches.</p>
      <p>The first destructive operation uses the initial backup slot. Later requests use the rolling slot; the initial backup is never automatically rotated.</p>
      <div class="actions"><button class="ctl" disabled={$session.busy} onclick={() => { review = false; }}>Not now</button><button class="ctl" disabled={compactUnavailable} onclick={compact}>Back up and optimize receipts</button></div>
    </div>
  {/if}
  <h3>Managed backups</h3>
  <p class="note">The initial backup and at most one rolling backup provide restore coverage. A temporary candidate can use extra space while a replacement is verified.</p>
  {#if $session.backups === null}<p>Backups: Unavailable</p>
  {:else if !$session.backups.length}<p>No managed backups.</p>
  {:else}
    {#each $session.backups as backup (backup.id)}
      <div class="backup">
        <p><strong>{backup.role === 'initial' ? 'Initial backup' : 'Rolling backup'}</strong> — {storageBytes(backup.bytes)} — {storageTime(backup.completed_at)}{backup.ready ? '' : ' — Not ready'}</p>
        {#if backup.in_use}<p class="note">This backup is in use. Wait for maintenance to finish before deleting it.</p>{/if}
        <button class="ctl" disabled={preview || $session.busy || backup.in_use} onclick={() => { deleting = backup; }}>Delete {backup.role} backup</button>
      </div>
    {/each}
  {/if}
  {#if deleting}
    <div class="review" role="group" aria-label="Review backup deletion">
      <p>Delete {deleting.role} backup — {storageBytes(deleting.bytes)} — {storageTime(deleting.completed_at)}?</p>
      <p>This removes its restore coverage. Future destructive work may require a new backup.</p>
      {#if $session.backups?.filter(backup => backup.ready).length === 1 && deleting.ready}<p>This is your last ready backup. Deleting it leaves no managed restore backup.</p>{/if}
      <div class="actions"><button class="ctl" disabled={$session.busy} onclick={() => { deleting = null; }}>Keep backup</button><button class="ctl" disabled={preview || $session.busy || $session.backups?.find(backup => backup.id === deleting?.id)?.in_use} onclick={remove}>Confirm backup deletion</button></div>
    </div>
  {/if}
  {#if $session.notice}<p role="status">{$session.notice}</p>{/if}
  {#if $session.error}<p role="alert">{errorLine($session.error)}</p>{/if}
  {#if $session.subscriptionError}<p role="status">{$session.subscriptionError}</p>{/if}
</div>
<style>
  .storage-card { min-width: 0; overflow-wrap: anywhere; }
  .measures { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 9rem), 1fr)); gap: var(--space-3); margin: 0; }
  dt, .note { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  dd { margin: var(--space-1) 0 0; font-size: var(--text-body); line-height: var(--leading-body); font-weight: var(--weight-medium); font-variant-numeric: tabular-nums; }
  p { font-size: var(--text-caption); line-height: var(--leading-caption); margin: var(--space-2) 0; }
  h3 { font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); margin: var(--space-4) 0 var(--space-2); }
  .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); margin: var(--space-3) 0; }
  .review, .backup, .progress { border-top: 1px solid var(--hairline); margin-top: var(--space-3); padding-top: var(--space-3); }
  .progress span { margin-left: var(--space-2); color: var(--warning); }
  [role=alert] { color: var(--danger); }
  strong { font-weight: var(--weight-medium); color: var(--text-primary); }
</style>

import { writable } from 'svelte/store';
import type { Client } from '../bridge/client';
import type { StorageStatus } from '../bindings/StorageStatus';
import type { StorageMaintenance } from '../bindings/StorageMaintenance';
import type { StorageFailure } from '../bindings/StorageFailure';
import type { StorageMaintenanceReason } from '../bindings/StorageMaintenanceReason';
import type { HistoryUpgradeStatus } from '../bindings/HistoryUpgradeStatus';
import type { ManagedStorageBackup } from '../bindings/ManagedStorageBackup';

export function storageCount(raw: string | null | undefined): bigint | null {
  return raw != null && /^\d+$/.test(raw) ? BigInt(raw) : null;
}
export function storageBytes(raw: string | null | undefined): string {
  const value = storageCount(raw);
  if (value === null) return 'Unavailable';
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) return `${value.toLocaleString('en-US')} B`;
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let unit = 0, divisor = 1n;
  while (unit < units.length - 1 && value >= divisor * 1000n) { unit++; divisor *= 1000n; }
  let scale = unit < 3 ? 1n : 10n;
  let rounded = (value * scale + divisor / 2n) / divisor;
  if (rounded >= 1000n * scale && unit < units.length - 1) {
    unit++; divisor *= 1000n;
    scale = unit < 3 ? 1n : 10n;
    rounded = (value * scale + divisor / 2n) / divisor;
  }
  const fraction = scale === 10n && rounded % scale ? `.${rounded % scale}` : '';
  return `${(rounded / scale).toLocaleString('en-US')}${fraction} ${units[unit]}`;
}
export function storageTime(raw: string | null | undefined): string {
  const micros = storageCount(raw);
  if (micros === null || micros / 1000n > 8640000000000000n) return 'Unavailable';
  return new Date(Number(micros / 1000n)).toLocaleString();
}
export function backupSpaceReason(status: HistoryUpgradeStatus | null): string {
  const required = storageCount(status?.required_bytes), free = storageCount(status?.free_bytes);
  if (free === null) return 'Free space is unavailable. Retry before making a backup.';
  if (required === null) return 'Required backup space is unavailable. Retry before making a backup.';
  if (free < required) return 'Not enough free space for a backup. Free space and retry.';
  return '';
}
export function historyRunning(status: HistoryUpgradeStatus | null): boolean {
  return status?.phase === 'backing_up' || status?.phase === 'upgrading';
}
export function historyUpgradeRequired(status: HistoryUpgradeStatus | null): boolean {
  return status !== null && ((status.schema_version !== null && status.schema_version >= 1 && status.schema_version <= 10)
    || status.phase === 'required' || historyRunning(status));
}
export const storageReasons: Record<StorageFailure | StorageMaintenanceReason, string> = {
  space_unknown: 'Free space is unavailable. Retry before making a backup.',
  insufficient_space: 'Not enough free space for a backup. Free space and retry.',
  cancelled: 'Cancelled. Resume when ready.',
  busy: 'Waiting for storage to become available.',
  io: 'Couldn’t read or write storage. Check disk access and retry.',
  full: 'Storage is full. Free space and retry.',
  invalid_unit: 'Some receipts could not be optimized. History is retained.',
  blocked_stream: 'A receipt stream needs attention. Resume to retry.',
  owner_poisoned: 'The backup needs verification. Resume to retry.',
  backup_poisoned: 'The backup needs verification. Resume to retry.',
  verification_failed: 'Backup verification failed. Request a new backup.',
  superseded: 'Backup request superseded; request a new backup.',
  wal_pressure: 'Paused for pending database writes.',
  in_use: 'This backup is in use. Wait for maintenance to finish before deleting it.',
  stopped_required: 'Stop capture before continuing.',
  unavailable: 'Storage is unavailable. Retry.',
  awaiting_backup: 'Waiting for a verified backup.',
  capture: 'Paused while capture has pending work.',
  budget: 'Paused to let capture catch up.',
  wal: 'Paused for pending database writes.',
};

interface StorageView {
  status: StorageStatus | null;
  maintenance: StorageMaintenance | null;
  failure: StorageFailure | null;
  backups: ManagedStorageBackup[] | null;
  location: string | null;
  space: HistoryUpgradeStatus | null;
  loading: boolean;
  busy: boolean;
  error: string;
  notice: string;
  subscriptionError: string;
}

/** Route lifetime owns reads/listeners only; persisted jobs belong to native storage. */
export function createStorageState(client: Client, preview = false) {
  let value: StorageView = { status: null, maintenance: null, failure: null, backups: null, location: null, space: null,
    loading: false, busy: false, error: '', notice: '', subscriptionError: '' };
  const store = writable(value);
  let disposed = false, started = false, request = 0, notices = 0, subscription = 0;
  let noticeRefresh = false, refreshAgain = false;
  let release: (() => void) | undefined;
  const publish = (patch: Partial<StorageView>) => {
    if (!disposed) { value = { ...value, ...patch }; store.set(value); }
  };
  async function refresh(estimate = false) {
    if (preview || disposed) return;
    const token = ++request, before = notices;
    publish({ loading: true, error: '' });
    const [status, backups, location, space] = await Promise.allSettled([
      client.queryStorageStatus(estimate ? { estimate: true } : undefined), client.listStorageBackups(),
      client.queryStorageBackupLocation(), client.queryHistoryUpgradeStatus(),
    ]);
    if (disposed || token !== request) return;
    publish({ loading: false,
      status: status.status === 'fulfilled' ? status.value : null,
      ...(before === notices ? { maintenance: status.status === 'fulfilled' ? status.value.maintenance : null,
        failure: status.status === 'fulfilled' ? status.value.failure : null } : {}),
      backups: backups.status === 'fulfilled' ? backups.value : null,
      location: location.status === 'fulfilled' ? location.value : null,
      space: space.status === 'fulfilled' ? space.value : null,
      error: status.status === 'rejected' ? 'Couldn’t read storage. Retry.'
        : backups.status === 'rejected' ? 'Couldn’t list backups. Retry.' : '',
    });
  }
  async function command(kind: 'compact' | 'resume' | 'cancel') {
    if (preview || disposed || value.busy) return false;
    if (kind === 'compact' && (backupSpaceReason(value.space) || value.space?.phase !== 'ready')) return false;
    if (kind === 'resume' && !value.maintenance?.jobs.some(job => job.needs_resume)) return false;
    publish({ busy: true, error: '', notice: '' });
    try {
      const outcome = await (kind === 'compact' ? client.compactStorage()
        : kind === 'resume' ? client.resumeStorageMaintenance() : client.cancelStorageMaintenance());
      if (disposed) return false;
      publish({ notice: outcome === 'queued' ? 'Storage maintenance is queued; waiting for storage to become available.'
        : kind === 'cancel' ? 'Cancellation requested. Work stops between batches.' : 'Storage maintenance requested.' });
      return true;
    } catch { publish({ notice: 'Couldn’t update storage maintenance. Retry.' }); return false; }
    finally { if (!disposed) { publish({ busy: false }); await refresh(); } }
  }
  async function deleteBackup(id: string) {
    if (preview || disposed || value.busy) return false;
    const backup = value.backups?.find(item => item.id === id);
    if (!backup || backup.in_use) return false;
    publish({ busy: true, error: '', notice: '' });
    let accepted = false;
    try {
      await client.deleteStorageBackup(id);
      accepted = !disposed;
      publish({ notice: 'Backup deleted. Its restore coverage is no longer available.' });
    } catch { publish({ notice: 'Couldn’t delete the backup. It may be in use; refresh and retry.' }); }
    finally { if (!disposed) { publish({ busy: false }); await refresh(); } }
    return accepted;
  }
  async function connect() {
    const token = ++subscription;
    release?.(); release = undefined;
    if (preview || disposed) return;
    try {
      const stop = await client.onStorageMaintenanceChanged(notice => {
        if (disposed || token !== subscription) return;
        notices++;
        publish({ maintenance: notice.maintenance, failure: notice.failure });
        void refreshFromNotice();
      });
      if (disposed || token !== subscription) stop(); else { release = stop; publish({ subscriptionError: '' }); }
    } catch { if (token === subscription) publish({ subscriptionError: 'Storage updates are unavailable. Refresh to retry.' }); }
  }
  async function refreshFromNotice() {
    if (noticeRefresh) { refreshAgain = true; return; }
    noticeRefresh = true;
    do { refreshAgain = false; await refresh(); } while (refreshAgain && !disposed);
    noticeRefresh = false;
  }
  return { subscribe: store.subscribe, refresh, command, deleteBackup,
    async retry() { await connect(); if (!disposed) await refresh(); },
    start() { if (!started) { started = true; void connect(); void refresh(); } },
    dispose() { disposed = true; request++; release?.(); },
  };
}

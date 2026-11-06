import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { StorageStatus } from '../../src/lib/bindings/StorageStatus';
import fixture from '../fixtures/t0-live';

export const storagePreviewMarker = 'STORAGE_SYNTHETIC_PREVIEW_ONLY';
export const storageVariants = ['default', 'pending', 'cancelled', 'pressure', 'complete', 'backups', 'unknown', 'superseded'] as const;
export const upgradeVariants = ['required', 'low-space', 'unknown', 'backing-up', 'upgrading', 'failed', 'ready'] as const;

export function createStoragePreview(surface: 'storage' | 'upgrade', variant: string) {
  const initial = structuredClone(fixture);
  initial.capture.state = 'Stopped';
  const transport = createFixtureTransport(initial);
  const client = createClient(transport);
  client.listAdapters = async () => [];
  const status: StorageStatus = {
    measured_at: '1900000000000000',
    database: { state: 'present', main_bytes: '600000000', wal_bytes: '1250000', shm_bytes: '32768',
      page_size: 4096, page_count: '146484', freelist_count: '0', allocated_bytes: '599998464', reusable_bytes: '0', auto_vacuum: 'none', journal_mode: 'wal' },
    backups: { retained_bytes: '0', temporary_bytes: '0' },
    recordings: { physical_bytes: '853735820', accounted_bytes: '853000000', quota_bytes: '1000000000' },
    receipt_estimate: { bytes: '94720000', basis: 'legacy_receipt_logical_bytes', measured_at: '1900000000000000' },
    maintenance: { jobs: [], totals: { eligible_records: '3000', eligible_bytes: '94720000', processed_records: '0', processed_bytes: '0', blocked_records: '0', blocked_bytes: '0' }, last_success_at: null },
    failure: null,
  };
  if (surface === 'storage') {
    transport.setHistoryUpgradeFixture({ schemaVersion: 16, freeBytes: '2000000000' });
    transport.setHistoryUpgradeStateFixture({ required_bytes: '750000000' });
    if (variant === 'unknown') {
      status.database.main_bytes = '9007199254740993'; status.database.reusable_bytes = null;
      status.recordings.physical_bytes = null; status.maintenance = null;
      transport.setHistoryUpgradeFixture({ schemaVersion: 16, freeBytes: null });
    } else if (variant !== 'default') {
      status.maintenance!.jobs = [{ phase: variant === 'pending' ? 'awaiting_backup' : variant === 'complete' ? 'reclaimable' : 'copying_receipts',
        paused: variant === 'cancelled' || variant === 'pressure', needs_resume: variant === 'cancelled',
        reason: variant === 'cancelled' ? 'cancelled' : variant === 'pressure' ? 'wal' : variant === 'pending' ? 'awaiting_backup' : null,
        requested_at: status.measured_at, updated_at: status.measured_at, counters: status.maintenance!.totals }];
      if (variant === 'complete') { status.maintenance!.last_success_at = status.measured_at; status.database.reusable_bytes = '62000000'; }
      if (variant === 'pending') status.backups.temporary_bytes = '300000000';
      if (variant === 'superseded') status.failure = 'superseded';
    }
    const backups = variant === 'backups' || variant === 'complete' ? [
      { id: '1', role: 'initial' as const, completed_at: '1899000000000000', bytes: '600000000', ready: true, in_use: variant === 'backups' },
      { id: '2', role: 'rolling' as const, completed_at: '1900000000000000', bytes: '610000000', ready: true, in_use: false },
    ] : [];
    status.backups.retained_bytes = backups.length ? '1210000000' : '0';
    transport.setStorageFixture(status, backups);
  } else {
    transport.setHistoryUpgradeFixture({ schemaVersion: variant === 'ready' ? 16 : 10,
      freeBytes: variant === 'unknown' ? null : variant === 'low-space' ? '1000' : '2000000000' });
    // Seed requested/running states directly; mounting a preview never invokes a mutation command.
    if (['backing-up', 'upgrading', 'failed'].includes(variant)) {
      transport.setHistoryUpgradeStateFixture({ phase: variant === 'upgrading' ? 'upgrading' : variant === 'failed' ? 'blocked' : 'backing_up',
        last_failure: variant === 'failed' ? 'verification_failed' : null });
    }
  }
  return { client, transport };
}

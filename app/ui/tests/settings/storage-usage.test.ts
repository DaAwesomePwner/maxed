import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import StorageUsage from '../../src/lib/settings/StorageUsage.svelte';
import { setupFixture, deferred, storageFixture } from '../setup/fixture';
import type { StorageStatus } from '../../src/lib/bindings/StorageStatus';
import { storageBytes, storageReasons } from '../../src/lib/settings/storage-state';
import type { StorageFailure } from '../../src/lib/bindings/StorageFailure';
import { nativeTransport } from '../../src/lib/bridge/native';

afterEach(cleanup);
it.each([
  ['space_unknown', true], ['insufficient_space', true], ['io', true], ['full', true],
  ['verification_failed', true], ['superseded', true], ['invalid_unit', true], ['unavailable', true],
  ['busy', false], ['wal_pressure', false], ['in_use', false], ['stopped_required', false],
  ['cancelled', false], ['owner_poisoned', false], ['blocked_stream', false],
] satisfies [StorageFailure, boolean][])('shows the Compact retry hint only for terminal failure %s', async (failure, hint) => {
  const f = await storageFixture();
  f.transport.setStorageFixture({ failure });
  render(StorageUsage, { client: f.client });
  await screen.findByText(storageReasons[failure]);
  expect(screen.queryByText(/use Compact now to request another backup/) !== null).toBe(hint);
});
it('does not request an upgrade in Settings after a failed current-schema probe', async () => {
  const f = await storageFixture(), ready = await f.client.queryHistoryUpgradeStatus();
  f.client.queryHistoryUpgradeStatus = vi.fn().mockResolvedValue({ ...ready, phase: 'blocked', schema_version: null, failure: 'busy' });
  render(StorageUsage, { client: f.client });
  await screen.findByText('600 MB');
  expect(screen.queryByText(/Complete the history upgrade in Setup/)).toBeNull();
});
it('explains that absent history has nothing to optimize', async () => {
  const f = await storageFixture(), ready = await f.client.queryHistoryUpgradeStatus();
  f.transport.setStorageFixture({ database: { ...f.status.database, state: 'absent', main_bytes: null } });
  f.client.queryHistoryUpgradeStatus = vi.fn().mockResolvedValue({ ...ready, schema_version: null, required_bytes: null, free_bytes: null });
  render(StorageUsage, { client: f.client });
  await screen.findByText('There is no saved history to optimize yet.');
  expect(screen.queryByText('Free space is unavailable. Retry before making a backup.')).toBeNull();
  expect((screen.getByRole('button', { name: 'Compact now' }) as HTMLButtonElement).disabled).toBe(true);
});
it.each([['999499', '999 KB'], ['999500', '1 MB'], ['999999', '1 MB'], ['999500000', '1 GB']])('promotes rounded bytes %s to %s', (bytes, expected) => {
  expect(storageBytes(bytes)).toBe(expected);
});
it('shows separate counters, unavailable values, zero, and exact large bytes', async () => {
  const { client } = setupFixture();
  const status = await client.queryStorageStatus();
  status.database.main_bytes = '9007199254740993';
  status.database.wal_bytes = '0';
  client.queryStorageStatus = vi.fn().mockResolvedValueOnce(status)
    .mockRejectedValueOnce(new Error('PRIVATE path')).mockResolvedValue(status);
  const compact = vi.spyOn(client, 'compactStorage');
  render(StorageUsage, { client });
  await screen.findByText('9,007,199,254,740,993 B');
  expect(screen.getAllByText('0 B').length).toBeGreaterThan(0);
  expect(screen.getAllByText('Unavailable').length).toBeGreaterThan(0);
  for (const label of ['Database', 'WAL', 'SHM', 'Reusable pages', 'Retained backups', 'Temporary backups', 'Recordings on disk', 'Recording quota']) expect(screen.getByText(label)).toBeTruthy();
  expect(compact).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Refresh storage' }));
  await screen.findByText('Couldn’t read storage. Retry.');
  expect(document.body.textContent).not.toContain('PRIVATE');
  await fireEvent.click(screen.getByRole('button', { name: 'Refresh storage' }));
  await screen.findByText('9,007,199,254,740,993 B');
});
it('never reads or mutates native storage in preview, even on retry', async () => {
  const { client } = setupFixture();
  const calls = ['queryStorageStatus', 'queryStorageBackupLocation', 'queryHistoryUpgradeStatus', 'listStorageBackups', 'compactStorage', 'deleteStorageBackup'] as const;
  const spies = calls.map(name => vi.spyOn(client, name));
  render(StorageUsage, { client, preview: true });
  await fireEvent.click(screen.getByRole('button', { name: 'Refresh storage' }));
  for (const spy of spies) expect(spy).not.toHaveBeenCalled();
  expect(screen.getByText('Unavailable in preview')).toBeTruthy();
});
it('ignores a retired client response and releases a late notice subscription', async () => {
  const a = setupFixture().client, b = setupFixture().client, old = deferred<StorageStatus>();
  const status = await b.queryStorageStatus(); status.database.main_bytes = '1000';
  a.queryStorageStatus = vi.fn(() => old.promise);
  b.queryStorageStatus = vi.fn(async () => status);
  const listen = deferred<() => void>(), release = vi.fn();
  a.onStorageMaintenanceChanged = vi.fn(() => listen.promise);
  const releaseB = vi.fn(); b.onStorageMaintenanceChanged = vi.fn(async () => releaseB);
  const view = render(StorageUsage, { client: a });
  await view.rerender({ client: b });
  await screen.findByText('1 KB');
  listen.resolve(release); old.resolve({ ...status, database: { ...status.database, main_bytes: '999999999' } });
  await waitFor(() => expect(release).toHaveBeenCalledOnce());
  expect(screen.getByText('1 KB')).toBeTruthy();
  view.unmount();
  expect(releaseB).toHaveBeenCalledOnce();
});

it.each([null, undefined, '', '-1', '1.5', '1e3', ' 1', '9007199254740993x'])('does not invent bytes for %s', value => {
  expect(storageBytes(value)).toBe('Unavailable');
});
it('keeps integer precision and uses the existing decimal storage units', () => {
  expect(storageBytes('0')).toBe('0 B');
  expect(storageBytes('853735820')).toBe('854 MB');
  expect(storageBytes('18446744073709551615')).toBe('18,446,744,073,709,551,615 B');
});
it('requires explicit review, treats queued as information, and keeps work after unmount', async () => {
  const f = await storageFixture();
  const native = vi.spyOn(nativeTransport, 'invoke');
  const pending = deferred<'queued'>();
  const compact = vi.spyOn(f.client, 'compactStorage').mockReturnValue(pending.promise);
  const cancel = vi.spyOn(f.client, 'cancelStorageMaintenance');
  const view = render(StorageUsage, { client: f.client });
  await screen.findByText('600 MB');
  expect(compact).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Compact now' }));
  expect(screen.getByText(/~600 MB.*estimated 55–70 MB/)).toBeTruthy();
  expect(screen.getByText(/file size may stay unchanged/)).toBeTruthy();
  expect(compact).not.toHaveBeenCalled();
  const confirm = screen.getByRole('button', { name: 'Back up and optimize receipts' });
  await fireEvent.click(confirm); await fireEvent.click(confirm);
  expect(compact).toHaveBeenCalledOnce();
  pending.resolve('queued');
  await screen.findByText('Storage maintenance is queued; waiting for storage to become available.');
  expect(screen.queryByRole('alert')).toBeNull();
  view.unmount(); expect(cancel).not.toHaveBeenCalled(); expect(native).not.toHaveBeenCalled();
});
it('shows backup-first progress, cancellation and restart, pressure, and last success from notices', async () => {
  const f = await storageFixture();
  const compact = vi.spyOn(f.client, 'compactStorage'), resume = vi.spyOn(f.client, 'resumeStorageMaintenance');
  let view = render(StorageUsage, { client: f.client });
  await screen.findByText('600 MB');
  await fireEvent.click(screen.getByRole('button', { name: 'Compact now' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Back up and optimize receipts' }));
  await screen.findByText('Backing up before optimization');
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel maintenance' }));
  await screen.findByText('Cancelled. Resume when ready.');
  view.unmount(); view = render(StorageUsage, { client: f.client });
  await screen.findByText('Cancelled. Resume when ready.');
  expect(compact).toHaveBeenCalledOnce();
  await fireEvent.click(screen.getByRole('button', { name: 'Resume maintenance' }));
  expect(resume).toHaveBeenCalledOnce();
  f.transport.advanceStorageMaintenance();
  await screen.findByText('Optimizing receipts: copy, verify, and remove legacy rows');
  await screen.findByText('Initial backup');
  const status = await f.client.queryStorageStatus();
  status.maintenance!.jobs[0].paused = true; status.maintenance!.jobs[0].reason = 'wal';
  f.transport.setStorageFixture(status);
  await screen.findByText('Paused for pending database writes.');
  expect(screen.queryByRole('button', { name: 'Resume maintenance' })).toBeNull();
  f.transport.advanceStorageMaintenance();
  await screen.findByText('Receipt optimization complete');
  await waitFor(() => expect(screen.getByText(/Last successful maintenance:/).textContent).not.toContain('Unavailable'));
  view.unmount();
});
it('uses only needs_resume and retains failure feedback after resume', async () => {
  const f = await storageFixture(); await f.client.compactStorage();
  f.transport.advanceStorageMaintenance('owner_poisoned');
  const status = await f.client.queryStorageStatus();
  status.maintenance!.jobs[0].paused = false;
  f.transport.setStorageFixture(status);
  render(StorageUsage, { client: f.client });
  await fireEvent.click(await screen.findByRole('button', { name: 'Resume maintenance' }));
  await screen.findByText('The backup needs verification. Resume to retry.');
  f.transport.advanceStorageMaintenance('insufficient_space');
  await screen.findByText(/use Compact now to request another backup/);
  expect(screen.queryByRole('button', { name: 'Resume maintenance' })).toBeNull();
  f.transport.setStorageFixture({ failure: 'superseded' });
  await screen.findByText('Backup request superseded; request a new backup.');
});
it.each(['0', null])('refuses a new backup with insufficient or unknown space (%s)', async freeBytes => {
  const f = await storageFixture();
  f.transport.setHistoryUpgradeFixture({ schemaVersion: 16, freeBytes });
  const compact = vi.spyOn(f.client, 'compactStorage');
  render(StorageUsage, { client: f.client });
  await screen.findByText('600 MB');
  expect((screen.getByRole('button', { name: 'Compact now' }) as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.click(screen.getByRole('button', { name: 'Compact now' }));
  expect(compact).not.toHaveBeenCalled();
});
it('shows both backup slots, reviews deletion, and reports in-use and last-backup coverage', async () => {
  const f = await storageFixture();
  const initial = { id: '1', role: 'initial' as const, completed_at: '1900000000000000', bytes: '600000000', ready: true, in_use: true };
  const rolling = { ...initial, id: '2', role: 'rolling' as const, bytes: '610000000', in_use: false };
  f.transport.setStorageFixture({ backups: { retained_bytes: '1210000000', temporary_bytes: '0' } }, [initial, rolling]);
  const remove = vi.spyOn(f.client, 'deleteStorageBackup');
  render(StorageUsage, { client: f.client });
  await screen.findByText('Rolling backup');
  expect(screen.getByText(/Backup location: C:/)).toBeTruthy();
  expect(screen.getByText(/610 MB/)).toBeTruthy();
  expect((screen.getByRole('button', { name: 'Delete initial backup' }) as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.click(screen.getByRole('button', { name: 'Delete rolling backup' }));
  expect(remove).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Keep backup' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Delete rolling backup' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm backup deletion' }));
  await screen.findByText('Backup deleted. Its restore coverage is no longer available.');
  expect(remove).toHaveBeenCalledExactlyOnceWith('2');
  f.transport.setStorageFixture({}, [{ ...initial, in_use: false }]);
  await waitFor(() => expect((screen.getByRole('button', { name: 'Delete initial backup' }) as HTMLButtonElement).disabled).toBe(false));
  await fireEvent.click(screen.getByRole('button', { name: 'Delete initial backup' }));
  expect(screen.getByText(/last ready backup/)).toBeTruthy();
  remove.mockRejectedValueOnce(new Error('PRIVATE backup path'));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm backup deletion' }));
  await screen.findByText('Couldn’t delete the backup. It may be in use; refresh and retry.');
  for (const notice of screen.getAllByRole('status')) expect(notice.textContent).not.toContain('C:');
  expect(document.body.textContent).not.toContain('PRIVATE');
});

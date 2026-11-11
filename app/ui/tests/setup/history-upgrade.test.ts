import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { expect, it, vi } from 'vitest';
import Setup from '../../src/routes/Setup.svelte';
import { deferred, setupFixture } from './fixture';
import type { HistoryUpgradeStatus } from '../../src/lib/bindings/HistoryUpgradeStatus';
import { nativeTransport } from '../../src/lib/bridge/native';

it.each(['pending', 'failed'] as const)('does not claim an upgrade is required when the history probe is %s', async state => {
  const f = setupFixture(), ready = await f.client.queryHistoryUpgradeStatus();
  f.client.queryHistoryUpgradeStatus = state === 'pending'
    ? vi.fn(() => deferred<HistoryUpgradeStatus>().promise)
    : vi.fn().mockResolvedValue({ ...ready, phase: 'blocked', schema_version: null, failure: 'busy', required_bytes: null, free_bytes: null });
  render(Setup, { client: f.client });
  await screen.findByText(state === 'pending' ? 'Checking history status' : 'Couldn’t check saved history');
  expect(screen.getByRole('button', { name: 'Retry history status and space' })).toBeTruthy();
  expect(screen.queryByText(/needs an upgrade|initial backup slot|Free space is unavailable/)).toBeNull();
  expect(screen.queryByRole('heading', { name: 'History upgrade required' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Start capture' }).getAttribute('aria-disabled')).toBe('true');
});

it.each([1, 2, 3, 4, 5, 6, 7, 8, 9, 10])('offers a backup-required upgrade for v%s without mutation on mount or retry', async schemaVersion => {
  const f = setupFixture();
  f.transport.setHistoryUpgradeFixture({ schemaVersion, freeBytes: '1000000000' });
  const upgrade = vi.spyOn(f.client, 'backupAndUpgradeHistory');
  render(Setup, { client: f.client });
  await screen.findByRole('heading', { name: 'History upgrade required' });
  expect(screen.getByText(/initial backup slot/)).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Upgrade without backup' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Start capture' }).getAttribute('aria-disabled')).toBe('true');
  await fireEvent.click(screen.getByRole('button', { name: 'Retry history status and space' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Not now' }));
  expect(upgrade).not.toHaveBeenCalled();
  expect(f.client.startCapture).not.toHaveBeenCalled();
  await screen.findByText('History is unchanged. Live capture stays blocked until the upgrade succeeds.');
});
it.each(['0', null])('refuses insufficient or unknown space (%s)', async freeBytes => {
  const f = setupFixture(); f.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes });
  const upgrade = vi.spyOn(f.client, 'backupAndUpgradeHistory');
  render(Setup, { client: f.client });
  const button = await screen.findByRole('button', { name: 'Back up and upgrade' });
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(true));
  await fireEvent.click(button); expect(upgrade).not.toHaveBeenCalled();
  await screen.findByText(freeBytes === null ? 'Free space is unavailable. Retry before making a backup.' : 'Not enough free space for a backup. Free space and retry.');
});
it.each([0, 11, 12, 14, 15, 16])('bypasses the upgrade prompt for v%s', async schemaVersion => {
  const f = setupFixture(); f.transport.setHistoryUpgradeFixture({ schemaVersion, freeBytes: '1000000000' });
  render(Setup, { client: f.client });
  await waitFor(() => expect(screen.getByRole('button', { name: 'Start capture' }).getAttribute('aria-disabled')).toBe('false'));
  expect(screen.queryByRole('heading', { name: 'History upgrade required' })).toBeNull();
});

it('upgrades only after confirmation, prevents repeated clicks, and enables capture only after success', async () => {
  const f = setupFixture(); f.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes: '1000000000' });
  const native = vi.spyOn(nativeTransport, 'invoke');
  const view = render(Setup, { client: f.client });
  const button = await screen.findByRole('button', { name: 'Back up and upgrade' });
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
  await fireEvent.click(button); await fireEvent.click(button);
  expect(f.client.backupAndUpgradeHistory).toHaveBeenCalledOnce();
  await screen.findByText('Backing up history before upgrading.');
  expect(screen.getByRole('button', { name: 'Stop capture' }).getAttribute('aria-disabled')).toBe('true');
  view.unmount(); render(Setup, { client: f.client });
  await screen.findByText('Backing up history before upgrading.');
  expect(f.client.backupAndUpgradeHistory).toHaveBeenCalledOnce();
  f.transport.advanceHistoryUpgrade();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry history status and space' }));
  await screen.findByText('Upgrading history after the verified backup.');
  expect(screen.getByRole('button', { name: 'Start capture' }).getAttribute('aria-disabled')).toBe('true');
  f.transport.advanceHistoryUpgrade();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry history status and space' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Start capture' }).getAttribute('aria-disabled')).toBe('false'));
  expect(f.client.startCapture).not.toHaveBeenCalled();
  expect(native).not.toHaveBeenCalled();
  expect((await f.client.listStorageBackups()).map(backup => backup.role)).toEqual(['initial']);
  await f.client.compactStorage(); f.transport.advanceStorageMaintenance();
  expect((await f.client.listStorageBackups()).map(backup => backup.role)).toEqual(['initial', 'rolling']);
});
it('keeps history blocked after backup failure and retries safely with retained failure feedback', async () => {
  const f = setupFixture(); f.transport.setHistoryUpgradeFixture({ schemaVersion: 7, freeBytes: '1000000000' });
  render(Setup, { client: f.client });
  const button = await screen.findByRole('button', { name: 'Back up and upgrade' });
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
  await fireEvent.click(button);
  f.transport.advanceHistoryUpgrade('verification_failed');
  await fireEvent.click(screen.getByRole('button', { name: 'Retry history status and space' }));
  await screen.findByText('Previous attempt: Backup verification failed. Request a new backup.');
  expect(screen.getByRole('button', { name: 'Check access' }).getAttribute('aria-disabled')).toBe('true');
  await fireEvent.click(button);
  expect(f.client.backupAndUpgradeHistory).toHaveBeenCalledTimes(2);
  await screen.findByText('Backing up history before upgrading.');
});
it('offers the prompt again after Not now and remount, with no mutation', async () => {
  const f = setupFixture(); f.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes: '1000000000' });
  const view = render(Setup, { client: f.client });
  await fireEvent.click(await screen.findByRole('button', { name: 'Not now' }));
  view.unmount(); render(Setup, { client: f.client });
  await screen.findByRole('button', { name: 'Back up and upgrade' });
  expect(f.client.backupAndUpgradeHistory).not.toHaveBeenCalled();
});
it('discards command replies from a retired client', async () => {
  const a = setupFixture(), b = setupFixture();
  a.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes: '1000000000' });
  b.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes: null });
  const pending = deferred<HistoryUpgradeStatus>(); a.client.backupAndUpgradeHistory = vi.fn(() => pending.promise);
  const ready = await setupFixture().client.queryHistoryUpgradeStatus();
  const view = render(Setup, { client: a.client });
  const button = await screen.findByRole('button', { name: 'Back up and upgrade' });
  await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
  await fireEvent.click(button);
  await view.rerender({ client: b.client });
  await screen.findByText('Free space is unavailable. Retry before making a backup.');
  const before = screen.getByRole('region', { name: 'History upgrade' }).innerHTML;
  pending.resolve(ready);
  await tick();
  expect(screen.getByRole('region', { name: 'History upgrade' }).innerHTML).toBe(before);
  expect(screen.getByRole('heading', { name: 'History upgrade required' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Start capture' }).getAttribute('aria-disabled')).toBe('true');
  expect(b.client.startCapture).not.toHaveBeenCalled();
});
it('makes no native upgrade reads or mutations in preview', async () => {
  const f = setupFixture();
  const native = vi.spyOn(nativeTransport, 'invoke');
  render(Setup, { client: f.client, preview: true });
  await screen.findByText('Assets ready');
  expect(f.client.queryHistoryUpgradeStatus).not.toHaveBeenCalled();
  expect(f.client.queryStorageBackupLocation).not.toHaveBeenCalled();
  expect(f.client.backupAndUpgradeHistory).not.toHaveBeenCalled();
  expect(native).not.toHaveBeenCalled();
});

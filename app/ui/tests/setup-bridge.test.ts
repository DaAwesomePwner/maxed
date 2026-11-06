import { expect, test, vi } from 'vitest';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import snapshot from './fixtures/t0-live';

test('setup queries preserve passive calls and forward only explicit retry options', async () => {
  const transport = createFixtureTransport(snapshot);
  const invoke = vi.spyOn(transport, 'invoke');
  const client = createClient(transport);

  await client.querySetup();
  await client.querySetup({ retry: true });
  await client.querySetup({ retry: false });
  await client.querySetup();

  expect(invoke.mock.calls).toEqual([
    ['get_setup_status', undefined],
    ['get_setup_status', { retry: true }],
    ['get_setup_status', { retry: false }],
    ['get_setup_status', undefined],
  ]);
});

test('history upgrade commands forward exact arguments and probes never advance work', async () => {
  const transport = createFixtureTransport(snapshot);
  const invoke = vi.spyOn(transport, 'invoke');
  const client = createClient(transport);
  await client.queryHistoryUpgradeStatus();
  await client.backupAndUpgradeHistory();
  expect(invoke.mock.calls).toEqual([
    ['get_history_upgrade_status', undefined],
    ['backup_and_upgrade_history', undefined],
  ]);
});

test('synthetic legacy upgrade requires space and confirmation and exposes every transition', async () => {
  const transport = createFixtureTransport(snapshot);
  const client = createClient(transport);
  for (const freeBytes of [null, '0'] as const) {
    transport.setHistoryUpgradeFixture({ schemaVersion: 6, freeBytes });
    const before = await client.queryHistoryUpgradeStatus();
    expect(before.failure).toBe(freeBytes === null ? 'space_unknown' : 'insufficient_space');
    expect(await client.backupAndUpgradeHistory()).toEqual({ ...before, last_failure: before.failure });
    expect((await client.queryHistoryUpgradeStatus()).schema_version).toBe(6);
  }
  transport.setHistoryUpgradeFixture({ schemaVersion: 6, freeBytes: '9999999999' });
  expect((await client.queryHistoryUpgradeStatus()).phase).toBe('required');
  expect((await client.backupAndUpgradeHistory()).phase).toBe('backing_up');
  expect((await client.backupAndUpgradeHistory()).phase).toBe('backing_up');
  expect((await client.queryHistoryUpgradeStatus()).schema_version).toBe(6);
  await client.cancelStorageMaintenance();
  expect(await client.queryHistoryUpgradeStatus()).toMatchObject({ phase: 'required', failure: null, last_failure: 'cancelled' });
  await client.backupAndUpgradeHistory();
  transport.advanceHistoryUpgrade('io');
  expect(await client.queryHistoryUpgradeStatus()).toMatchObject({ phase: 'required', failure: null, last_failure: 'io' });
  expect((await client.queryHistoryUpgradeStatus()).schema_version).toBe(6);
  await client.backupAndUpgradeHistory();
  transport.advanceHistoryUpgrade();
  expect((await client.queryHistoryUpgradeStatus()).phase).toBe('upgrading');
  transport.advanceHistoryUpgrade();
  expect(await client.queryHistoryUpgradeStatus()).toMatchObject({ phase: 'ready', schema_version: 16, failure: null });
});

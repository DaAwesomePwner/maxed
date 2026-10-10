import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { get } from 'svelte/store';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { createUpdateSession } from '../src/lib/settings/updates';
import type { UpdateStatus } from '../src/lib/bindings/UpdateStatus';
import Settings from '../src/routes/Settings.svelte';
import UpdateControl from '../src/lib/settings/UpdateControl.svelte';
import snapshot from './fixtures/t0-live';
import { deferred, setupFixture } from './setup/fixture';

const status = (phase: UpdateStatus['phase'] = 'idle'): UpdateStatus => ({ phase, channel: 'stable', offer: null, downloaded_bytes: 0, total_bytes: null, failure: null });
afterEach(() => vi.useRealTimers());

describe('P7 command contracts', () => {
  it('forwards decimal IDs using camelCase and reports the rail with undefined args', async () => {
    const transport = createFixtureTransport(snapshot);
    const invoke = vi.spyOn(transport, 'invoke');
    const client = createClient(transport);
    await client.queryUpdate(); await client.queryRecording(); await client.reportInteractiveRail();
    await expect(client.checkForUpdates()).rejects.toThrow();
    await expect(client.downloadUpdate('18446744073709551615')).rejects.toThrow();
    await expect(client.installUpdate('7')).rejects.toThrow();
    await expect(client.cancelUpdate()).rejects.toThrow();
    expect(invoke.mock.calls).toEqual([
      ['get_update_status', undefined], ['get_recording_status', undefined], ['report_interactive_rail', undefined],
      ['check_for_updates', undefined], ['download_update', { requestId: '18446744073709551615' }],
      ['install_update', { requestId: '7' }], ['cancel_update', undefined],
    ]);
  });

  it('keeps synthetic queries clone-isolated and rail reporting a no-op', async () => {
    const client = createClient(createFixtureTransport(snapshot));
    const first = await client.queryUpdate(); first.channel = 'beta';
    const recording = await client.queryRecording(); recording.retained_bytes = 123;
    expect((await client.queryUpdate()).channel).toBe('stable');
    expect((await client.queryRecording()).retained_bytes).toBe(0);
    await expect(client.reportInteractiveRail()).resolves.toBeUndefined();
    await expect(client.reportInteractiveRail()).resolves.toBeUndefined();
  });
});

describe('P7 Settings lifecycle', () => {
  it('uses Settings capture status to hide restart while running', async () => {
    const f = setupFixture();
    f.client.queryUpdate = vi.fn(async (): Promise<UpdateStatus> => ({ ...status('failed'), failure: 'install_failed' }));
    render(Settings, { client: f.client });
    await screen.findByText('Install failed. Check again.');
    for (const state of ['Live', 'WaitingForGame', 'Recovering'] as const) {
      f.capture('Stopped');
      await screen.findByRole('button', { name: 'Restart Capture' });
      f.capture(state);
      await vi.waitFor(() => expect(screen.queryByRole('button', { name: 'Restart Capture' })).toBeNull());
    }
    f.capture('Stopped');
    await screen.findByRole('button', { name: 'Restart Capture' });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Restart Capture' }));
    await vi.waitFor(() => expect(screen.queryByRole('button', { name: 'Restart Capture' })).toBeNull());
    expect(f.client.startCapture).toHaveBeenCalledTimes(1);
  });

  it('does not offer capture restart before capture status is known', async () => {
    const client = createClient(createFixtureTransport(snapshot));
    client.queryUpdate = vi.fn(async (): Promise<UpdateStatus> => ({ ...status('failed'), failure: 'install_failed' }));
    render(UpdateControl, { client });
    await screen.findByText('Install failed. Check again.');
    expect(screen.queryByRole('button', { name: 'Restart Capture' })).toBeNull();
  });

  it('offers restart after post-shutdown verification failure without a stale Download action', async () => {
    const client = createClient(createFixtureTransport(snapshot));
    client.queryUpdate = vi.fn(async (): Promise<UpdateStatus> => ({ ...status('failed'), failure: 'install_failed' }));
    client.startCapture = vi.fn(async () => {});
    render(UpdateControl, { client, capture: { ...snapshot.capture, state: 'Stopped' } });
    await screen.findByText('Install failed. Check again.');
    expect(screen.queryByRole('button', { name: 'Download' })).toBeNull();
    expect(client.startCapture).not.toHaveBeenCalled();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Restart Capture' }));
    expect(client.startCapture).toHaveBeenCalledTimes(1);
    vi.mocked(client.startCapture).mockRejectedValueOnce(new Error('synthetic private failure'));
    await userEvent.setup().click(screen.getByRole('button', { name: 'Restart Capture' }));
    await screen.findByText('Couldn’t restart capture. Retry.');
    expect(screen.queryByText('synthetic private failure')).toBeNull();
    expect((screen.getByRole('button', { name: 'Check Updates' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('queries status without checking at mount and preserves smaller recording quotas', async () => {
    const f = setupFixture(); f.setSettings({ recording_limit_bytes: 65_575 });
    f.client.checkForUpdates = vi.fn(async () => status());
    render(Settings, { client: f.client });
    const channel = await screen.findByRole('combobox', { name: 'Update Channel' });
    const user = userEvent.setup();
    await user.selectOptions(channel, 'beta');
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText('Saved', { selector: '[role="status"]' });
    expect((await f.client.querySettings()).recording_limit_bytes).toBe(65_575);
    expect(f.client.checkForUpdates).not.toHaveBeenCalled();
    expect((screen.getByRole('checkbox', { name: 'Save Recordings' }) as HTMLInputElement).disabled).toBe(false);
    expect((screen.getByRole('checkbox', { name: 'Save Recordings' }) as HTMLInputElement).checked).toBe(false);
  });

  it('renders notes as plain text, leaves unknown totals honest, and disables preview actions', async () => {
    const client = createClient(createFixtureTransport(snapshot));
    client.queryUpdate = vi.fn(async (): Promise<UpdateStatus> => ({ ...status('available'), offer: { request_id: '7', version: '2.0.0', channel: 'stable', notes: '<img src="https://example.invalid/private">' } }));
    render(UpdateControl, { client, preview: true });
    await screen.findByText('<img src="https://example.invalid/private">');
    expect(document.querySelector('img')).toBeNull();
    expect((screen.getByRole('button', { name: 'Download' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Check Updates' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('polls with one outstanding request and a non-resetting 250 ms delay; disposal leaves native work alive', async () => {
    vi.useFakeTimers();
    const client = createClient(createFixtureTransport(snapshot));
    const pending = deferred<UpdateStatus>();
    client.queryUpdate = vi.fn().mockResolvedValueOnce(status('downloading')).mockReturnValueOnce(pending.promise).mockResolvedValue(status('ready'));
    client.cancelUpdate = vi.fn(async () => status());
    const session = createUpdateSession(client);
    await session.refresh();
    await vi.advanceTimersByTimeAsync(200);
    await session.refresh(); // Must not restart or bypass the scheduled poll.
    expect(client.queryUpdate).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(50);
    expect(client.queryUpdate).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1000);
    expect(client.queryUpdate).toHaveBeenCalledTimes(2);
    pending.resolve(status('downloading'));
    await Promise.resolve(); await Promise.resolve();
    session.dispose();
    await vi.advanceTimersByTimeAsync(1000);
    expect(client.queryUpdate).toHaveBeenCalledTimes(2);
    expect(client.cancelUpdate).not.toHaveBeenCalled();
  });

  it('stops on IPC failure, sanitizes the message and requires manual Retry', async () => {
    vi.useFakeTimers();
    const client = createClient(createFixtureTransport(snapshot));
    client.queryUpdate = vi.fn().mockRejectedValueOnce(new Error('PRIVATE path')).mockResolvedValue(status());
    const session = createUpdateSession(client);
    await session.refresh();
    expect(get(session).error).toBe('Couldn’t load updates. Retry.');
    expect(get(session).error).not.toContain('PRIVATE');
    await vi.advanceTimersByTimeAsync(5000);
    expect(client.queryUpdate).toHaveBeenCalledTimes(1);
    await session.refresh();
    expect(get(session).error).toBe('');
    session.dispose();
  });
});

it('retains the shutdown-blocked offer for explicit install Retry and cancel', async () => {
  const client = createClient(createFixtureTransport(snapshot));
  client.queryUpdate = vi.fn(async (): Promise<UpdateStatus> => ({ ...status('failed'), failure: 'shutdown_blocked', offer: { request_id: '7', version: '2.0.0', channel: 'stable', notes: '' } }));
  client.installUpdate = vi.fn(async () => status('installing'));
  render(UpdateControl, { client });
  await screen.findByText('Check storage. Retry Install.');
  expect(screen.queryByRole('button', { name: 'Download' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Cancel' })).toBeTruthy();
  await userEvent.setup().click(screen.getByRole('button', { name: 'Retry Install' }));
  expect(client.installUpdate).toHaveBeenCalledWith('7');
});

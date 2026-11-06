import { get } from 'svelte/store';
import { describe, expect, it, vi } from 'vitest';
import { createSetupSession } from '../../src/lib/setup/session';
import { setupProgress } from '../../src/lib/setup/presentation';
import type { SetupStatus } from '../../src/lib/bindings/SetupStatus';
import type { HistoryUpgradeStatus } from '../../src/lib/bindings/HistoryUpgradeStatus';
import { deferred, setupFixture } from './fixture';

describe('setup lifecycle', () => {
  it.each(['rejected', 'blocked'] as const)('retries a %s running-upgrade probe with backoff and refreshes setup and settings on completion', async failure => {
    vi.useFakeTimers();
    const f = setupFixture(), ready = await f.client.queryHistoryUpgradeStatus();
    f.client.queryHistoryUpgradeStatus = vi.fn()
      .mockResolvedValueOnce({ ...ready, phase: 'backing_up', schema_version: 10 })
      .mockImplementationOnce(async () => {
        if (failure === 'rejected') throw new Error('synthetic probe failure');
        return { ...ready, phase: 'blocked', schema_version: null, failure: 'busy' };
      })
      .mockResolvedValue(ready);
    const session = createSetupSession(f.client);
    try {
      session.start();
      await vi.advanceTimersByTimeAsync(0);
      const setupReads = vi.mocked(f.client.querySetup).mock.calls.length;
      const settingsReads = vi.mocked(f.client.querySettings).mock.calls.length;
      await vi.advanceTimersByTimeAsync(500);
      expect(get(session).history?.phase).toBe('backing_up');
      expect(get(session).errors.history).not.toBe('');
      await vi.advanceTimersByTimeAsync(999);
      expect(f.client.queryHistoryUpgradeStatus).toHaveBeenCalledTimes(2);
      await vi.advanceTimersByTimeAsync(1);
      expect(get(session).history?.phase).toBe('ready');
      expect(get(session).errors.history).toBe('');
      expect(f.client.querySetup).toHaveBeenCalledTimes(setupReads + 1);
      expect(f.client.querySettings).toHaveBeenCalledTimes(settingsReads + 1);
      await vi.advanceTimersByTimeAsync(5000);
      expect(f.client.queryHistoryUpgradeStatus).toHaveBeenCalledTimes(3);
    } finally { session.dispose(); vi.useRealTimers(); }
  });
  it('keeps running-upgrade controls blocked when a progress probe fails', async () => {
    const f = setupFixture();
    f.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes: '1000000000' });
    await f.client.backupAndUpgradeHistory();
    const session = createSetupSession(f.client);
    await session.refreshHistory();
    f.client.queryHistoryUpgradeStatus = vi.fn().mockRejectedValue(new Error('PRIVATE'));
    await session.refreshHistory();
    expect(get(session).history?.phase).toBe('backing_up');
    expect(get(session).errors.history).toBe('Couldn’t check history upgrade. Retry.');
    expect(await session.action(f.client.stopCapture, 'Stop failed.')).toBe(false);
    session.dispose();
  });
  it('discards superseded and disposed history probes without mutation', async () => {
    const f = setupFixture(), ready = await f.client.queryHistoryUpgradeStatus();
    const old = deferred<HistoryUpgradeStatus>();
    f.client.queryHistoryUpgradeStatus = vi.fn().mockReturnValueOnce(old.promise).mockResolvedValue(ready);
    const session = createSetupSession(f.client);
    const first = session.refreshHistory(); await session.refreshHistory();
    old.resolve({ ...ready, phase: 'required', schema_version: 10 }); await first;
    expect(get(session).history?.phase).toBe('ready');
    const late = deferred<HistoryUpgradeStatus>(); f.client.queryHistoryUpgradeStatus = () => late.promise;
    const last = session.refreshHistory(); session.dispose();
    late.resolve({ ...ready, phase: 'blocked' }); await last;
    expect(get(session).history?.phase).toBe('ready');
    expect(f.client.backupAndUpgradeHistory).not.toHaveBeenCalled();
  });
  it('polls only during upgrade and releases its timer at disposal', async () => {
    vi.useFakeTimers();
    const f = setupFixture(); f.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes: '1000000000' });
    const session = createSetupSession(f.client); session.start();
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.client.queryHistoryUpgradeStatus).toHaveBeenCalledOnce();
    await session.upgradeHistory();
    const calls = vi.mocked(f.client.queryHistoryUpgradeStatus).mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.client.queryHistoryUpgradeStatus).toHaveBeenCalledTimes(calls + 2);
    expect(await session.action(f.client.stopCapture, 'Stop failed.')).toBe(false);
    session.dispose();
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.client.queryHistoryUpgradeStatus).toHaveBeenCalledTimes(calls + 2);
    expect(f.client.stopCapture).not.toHaveBeenCalled();
  });
  it('has no stopped-idle polling, polls only pending extraction, and stops after completion and teardown', async () => {
    vi.useFakeTimers();
    const f = setupFixture();
    const pending = deferred<void>();
    f.client.setAssetsPath = vi.fn(() => pending.promise);
    const session = createSetupSession(f.client);
    session.start();
    await vi.advanceTimersByTimeAsync(0);
    const idleCount = vi.mocked(f.client.querySetup).mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.client.querySetup).toHaveBeenCalledTimes(idleCount);
    const extraction = session.selectAssets('C:\\Synthetic\\resources.assets');
    await vi.advanceTimersByTimeAsync(750);
    expect(f.client.querySetup).toHaveBeenCalledTimes(idleCount + 3);
    pending.resolve(); await extraction;
    const completeCount = vi.mocked(f.client.querySetup).mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.client.querySetup).toHaveBeenCalledTimes(completeCount);
    session.dispose();
    f.capture('Live');
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.client.querySetup).toHaveBeenCalledTimes(completeCount);
  });

  it('does not overlap timer polls and releases delayed listeners after disposal', async () => {
    vi.useFakeTimers();
    const f = setupFixture({ assets: 'extracting' });
    const listen = deferred<() => void>();
    const release = vi.fn();
    f.client.onCapture = vi.fn(() => listen.promise);
    const session = createSetupSession(f.client);
    session.start();
    await vi.advanceTimersByTimeAsync(0);
    const probe = deferred<SetupStatus>();
    f.client.querySetup = vi.fn(() => probe.promise);
    await vi.advanceTimersByTimeAsync(1250);
    expect(f.client.querySetup).toHaveBeenCalledOnce();
    session.dispose(); listen.resolve(release);
    probe.resolve({ ...get(session).setup!, assets: 'ready' });
    await vi.advanceTimersByTimeAsync(1000);
    expect(release).toHaveBeenCalledOnce();
    expect(f.client.querySetup).toHaveBeenCalledOnce();
    expect(get(session).setup?.assets).toBe('extracting');
  });

  it('discards superseded setup replies and stops polling on a failed probe until retry', async () => {
    vi.useFakeTimers();
    const f = setupFixture({ assets: 'extracting' });
    const session = createSetupSession(f.client);
    await session.refreshSetup();
    const stale = deferred<SetupStatus>();
    const staleValue = { ...get(session).setup!, assets: 'ready', live_confirmed: true } satisfies SetupStatus;
    f.client.querySetup = vi.fn().mockReturnValueOnce(stale.promise).mockRejectedValueOnce(new Error('PRIVATE'));
    const first = session.refreshSetup();
    await session.refreshSetup();
    stale.resolve(staleValue); await first;
    expect(get(session).setup).toBeNull();
    expect(get(session).errors.setup).toContain('Retry.');
    await vi.advanceTimersByTimeAsync(1000);
    expect(f.client.querySetup).toHaveBeenCalledTimes(2);
    session.dispose();
  });

  it('re-queries actual status on remount instead of remembering completion', async () => {
    vi.useFakeTimers();
    const f = setupFixture({ live_confirmed: true });
    f.capture('Live');
    const first = createSetupSession(f.client); first.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(setupProgress(get(first).setup, get(first).capture, false).live).toBe(true);
    first.dispose(); f.setSetup({ assets: 'missing', live_confirmed: false });
    const second = createSetupSession(f.client); second.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(second).setup?.assets).toBe('missing');
    expect(setupProgress(get(second).setup, get(second).capture, false).live).toBe(false);
    second.dispose();
  });

  it('allows Stop during extraction and never reports pending operations as complete', async () => {
    const f = setupFixture({ live_confirmed: true });
    const pending = deferred<void>(); f.client.setAssetsPath = () => pending.promise;
    const session = createSetupSession(f.client);
    const extraction = session.selectAssets('C:\\Synthetic\\resources.assets');
    expect(await session.action(f.client.stopCapture, 'Could not stop.', { duringExtraction: true, captureAction: true })).toBe(true);
    expect(f.client.stopCapture).toHaveBeenCalledOnce();
    expect(setupProgress(get(session).setup, { state: 'Live', message: '', dropped_frames: 0, unknown_packets: 0, decode_failures: 0 }, false, true).live).toBe(false);
    pending.resolve(); await extraction; session.dispose();
  });
});

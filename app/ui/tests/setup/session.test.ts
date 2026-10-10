import { get } from 'svelte/store';
import { describe, expect, it, vi } from 'vitest';
import { createSetupSession } from '../../src/lib/setup/session';
import { setupProgress } from '../../src/lib/setup/presentation';
import type { SetupStatus } from '../../src/lib/bindings/SetupStatus';
import { deferred, setupFixture } from './fixture';

describe('setup lifecycle', () => {
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

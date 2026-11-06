import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mountV2, homeSnapshot, deferred } from '../home-v2/fixtures';
import { mountPhase2, flush, follow, primary, ready, runPage, run, advance } from '../phase2/fixtures';
import { productionFeatures } from '../../src/lib/navigation/routes';
import { RECOVERY_GRACE_MS } from '../../src/lib/home/mode';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunPage } from '../../src/lib/bindings/RunPage';
import type { AlertPage } from '../../src/lib/bindings/AlertPage';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });
it.each(['nexus', 'dungeon', 'realm'] as const)('shows live resync outside the %s Home panel', async area_kind => {
  const snapshot = homeSnapshot({ area_kind });
  const page = mountV2(snapshot, undefined, { features: productionFeatures }); await flush();
  const idle = page.container.querySelector('.home-idle');
  const children = idle ? [...idle.children] : [];
  const pending = deferred<typeof snapshot>();
  page.client.querySnapshot = vi.fn(() => pending.promise);
  const focus = screen.getByRole('link', { name: 'Runs' }); focus.focus();
  page.transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: String(Number(snapshot.sequence) + 2),
    at: snapshot.at, event: { session_updated: snapshot.session! } });
  await flush();
  const indicator = screen.getByRole('status', { name: 'Background activity' });
  expect(indicator.textContent).toContain('Refreshing');
  expect(indicator.closest('main')).toBeNull();
  expect(screen.getByRole('main').querySelector('.storage-note[role="status"]')).toBeNull();
  if (area_kind === 'nexus') {
    expect(idle?.querySelector('.nexus-home')).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Reconnecting' })).toBeNull();
    expect([...idle!.children]).toEqual(children);
  }
  expect(document.activeElement).toBe(focus);
  pending.resolve({ ...snapshot, sequence: String(Number(snapshot.sequence) + 2) }); await flush();
  expect(indicator.textContent).not.toBe('Refreshing');
});

it('keeps Reconnecting when capture is recovering during a Nexus resync', async () => {
  const base = homeSnapshot({ area_kind: 'nexus' });
  const snapshot = { ...base, capture: { ...base.capture, state: 'Recovering' as const } };
  const page = mountV2(snapshot, undefined, { features: productionFeatures }); await flush();
  expect(screen.getByRole('heading', { name: 'Reconnecting' })).toBeTruthy();
  const pending = deferred<typeof snapshot>();
  page.client.querySnapshot = vi.fn(() => pending.promise);
  page.transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: String(Number(snapshot.sequence) + 2),
    at: snapshot.at, event: { session_updated: snapshot.session! } });
  await flush();
  expect(screen.getByRole('heading', { name: 'Reconnecting' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Ready' })).toBeNull();
  pending.resolve({ ...snapshot, sequence: String(Number(snapshot.sequence) + 2) }); await flush();
});

it.each(['dungeon', 'realm'] as const)('keeps Reconnecting when a long %s resync falls back to Nexus mode', async area_kind => {
  const snapshot = homeSnapshot({ area_kind });
  const page = mountV2(snapshot, undefined, { features: productionFeatures }); await flush();
  const pending = deferred<typeof snapshot>();
  page.client.querySnapshot = vi.fn(() => pending.promise);
  page.transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: String(Number(snapshot.sequence) + 2),
    at: snapshot.at, event: { session_updated: snapshot.session! } });
  await flush();
  await advance(RECOVERY_GRACE_MS + 100);
  expect(screen.getByRole('heading', { name: 'Reconnecting' })).toBeTruthy();
  pending.resolve({ ...snapshot, sequence: String(Number(snapshot.sequence) + 2) }); await flush();
});

it.each(['#/runs', '#/runs/run-1', '#/fights/fight-1', '#/fights/fight-1/players/7', '#/alerts'])(
  'terminal superseded stops activity without polling on %s', async hash => {
    const superseded = vi.fn(async () => ({ status: 'superseded' as const }));
    mountPhase2({ hash, queries: { listRuns: superseded, getRun: superseded, getFight: superseded, getPlayerBreakdown: superseded }, alerts: superseded });
    await flush();
    const calls = superseded.mock.calls.length;
    expect(calls).toBeGreaterThan(0);
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toBe('');
    await advance(60000);
    expect(superseded).toHaveBeenCalledTimes(calls);
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toBe('');
  },
);

it('exposes a failed initial preferences read beside the panel and retries visibly', async () => {
  const page = mountPhase2({ hash: '#/logs/alerts', configure: client => {
    const read = client.querySettings;
    client.querySettings = vi.fn().mockRejectedValueOnce(new Error('Synthetic initial failure')).mockImplementation(read);
  } });
  await flush();
  // Settings also reads app settings; fail the Shell's initial alert-policy read first.
  await follow('#/settings', primary());
  expect(page.client.querySettings).toHaveBeenCalledTimes(3);
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toBe('');
  expect(screen.getByText("Couldn't load alert preferences. Retry.").closest('.preferences')).toBeTruthy();
  const retry = screen.getByRole('button', { name: 'Retry Preferences' });
  expect(retry.closest('details')).toBeNull();
  await fireEvent.click(retry); await flush();
  expect(page.client.querySettings).toHaveBeenCalledTimes(4);
  expect((screen.getByRole('combobox', { name: 'Personal Best' }) as HTMLSelectElement).disabled).toBe(false);
  expect(screen.queryByText("Couldn't load alert preferences. Retry.")).toBeNull();
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toBe('');
});
it('shows a long initial Runs load in the shell and releases it when data arrives', async () => {
  const pending = deferred<HistoryReply<RunPage>>();
  mountPhase2({ hash: '#/runs', queries: { listRuns: () => pending.promise } }); await flush();
  const indicator = screen.getByRole('status', { name: 'Background activity' });
  expect(indicator.textContent).toContain('Loading runs');
  await vi.advanceTimersByTimeAsync(6000); await flush();
  expect(indicator.textContent).toContain('Loading runs');
  pending.resolve(ready(runPage())); await flush();
  expect(indicator.textContent).toBe('');
});
it('does not insert a Retry control while a saved run automatically catches up', async () => {
  const page = mountPhase2({ hash: '#/runs/run-1' }); await flush();
  const main = screen.getByRole('main');
  const controls = [...main.querySelectorAll('button')];
  page.event({ run_updated: run() }, '13'); await advance();
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toContain('Refreshing run');
  expect([...main.querySelectorAll('button')]).toEqual(controls);
});
it('keeps alert refresh outside the saved list and leaves load failures inline', async () => {
  const pending = deferred<HistoryReply<AlertPage>>();
  mountPhase2({ hash: '#/alerts', alerts: () => pending.promise }); await flush();
  const indicator = screen.getByRole('status', { name: 'Background activity' });
  expect(indicator.textContent).toContain('Loading alerts');
  pending.reject(new Error('Synthetic failure')); await flush();
  expect(screen.getByText("Couldn't load alerts. Your filters are kept.").closest('main')).toBeTruthy();
  expect(indicator.textContent).toBe('');
});

import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SetupStatus } from '../../src/lib/bindings/SetupStatus';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { localDayRange } from '../../src/lib/home/scope';
import { deferred, homeSnapshot, homeSummary, saved } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';
import { injectChicagoDate } from '../runs/fixtures';

describe('P4 setup retry protection (NON-RED)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  });
  afterEach(() => {
    cleanup();
    vi.clearAllTimers();
    vi.unstubAllGlobals();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('non_red_setup_retry_cap_and_manual_action_preserve_demo_calendar', async () => {
    injectChicagoDate();
    const captured = homeSnapshot({ stream_id: 'opaque-source', area_kind: 'nexus', run: null, at: '1772992800000000' });
    const known = { ...await createClient(createFixtureTransport(captured)).querySetup(), demo: true };
    const read = vi.fn().mockRejectedValue(new Error('setup unavailable'));
    const page = mountHome(captured, homeSummary(), '#/', { setupRead: read }); await flushHomeRender();
    await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await flushHomeRender();
    for (const delay of [1000, 2000, 4000, 8000, 16000, 30000]) {
      await vi.advanceTimersByTimeAsync(delay); await flushHomeRender();
    }
    expect(read).toHaveBeenCalledTimes(7); expect(todayCalls(page)).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(60000); await flushHomeRender();
    expect(read).toHaveBeenCalledTimes(7);
    read.mockResolvedValue(known);
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flushHomeRender();
    expect(read).toHaveBeenCalledTimes(8);
    expect(page.query).toHaveBeenLastCalledWith({ today: { range: { start: '1772949600000000', end: '1773032400000000' } } });
    expect(screen.getByRole('link', { name: /Synthetic Sanctum/ })).toBeTruthy();
    await vi.advanceTimersByTimeAsync(60000); await flushHomeRender();
    expect(read).toHaveBeenCalledTimes(8); expect(todayCalls(page)).toHaveLength(1);
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const snapshot = () => homeSnapshot({ area_kind: 'nexus', run: null, at: '1772971200000000' });
async function setup(demo: boolean) {
  return { ...await createClient(createFixtureTransport(snapshot())).querySetup(), demo };
}
const todayCalls = (page: ReturnType<typeof mountHome>) => page.query.mock.calls.filter(([scope]) => 'today' in scope);

it('backs off in the same stream and never reads wall-clock Today while the flag is unknown', async () => {
  const known = await setup(true);
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  const read = vi.fn().mockRejectedValueOnce(new Error('transient')).mockRejectedValueOnce(new Error('transient')).mockResolvedValue(known);
  const page = mountHome(snapshot(), homeSummary(), '#/', { setupRead: read });
  await flushHomeRender();
  expect(page.query).toHaveBeenCalledWith({ session: { id: snapshot().session!.id } });
  await fireEvent.click(screen.getByRole('button', { name: 'Today' }));
  await flushHomeRender();
  expect(todayCalls(page)).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(999); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1999); await flushHomeRender();
  expect(todayCalls(page)).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(1); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(3);
  expect(page.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(new Date(Number(BigInt(snapshot().at) / 1000n))) } });
});

it('bounds automatic attempts and lets Home Retry recover an exhausted setup read', async () => {
  const known = await setup(false);
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  const read = vi.fn().mockRejectedValue(new Error('transient'));
  const page = mountHome(snapshot(), homeSummary(), '#/', { setupRead: read });
  await flushHomeRender();
  expect(page.query).toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Today' }));
  await flushHomeRender();
  for (const delay of [1000, 2000, 4000, 8000, 16000, 30000]) {
    await vi.advanceTimersByTimeAsync(delay); await flushHomeRender();
  }
  expect(read).toHaveBeenCalledTimes(7);
  await vi.advanceTimersByTimeAsync(60000); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(7);
  expect(todayCalls(page)).toHaveLength(0);
  read.mockResolvedValue(known);
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(8);
  expect(page.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(new Date()) } });
});

it('cancels the pending retry on stream change and on unmount', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  const read = vi.fn().mockRejectedValue(new Error('transient'));
  const page = mountHome(snapshot(), homeSummary(), '#/', { setupRead: read });
  await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(500);
  const next = { ...snapshot(), stream_id: 'replacement', sequence: '1' };
  page.client.queryStore = vi.fn(async () => saved('1', next.stream_id));
  page.transport.publishSnapshot(next); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(2); // Positive control: replacement stream accepted.
  await vi.advanceTimersByTimeAsync(500); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(2); // Retired stream's deadline.
  await vi.advanceTimersByTimeAsync(500); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(3); // Replacement's own retry.
  page.unmount(); await flushHomeRender();
  await vi.advanceTimersByTimeAsync(60000); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(3);
});

it('Home Retry recovers immediately and cancels the scheduled backoff', async () => {
  const known = await setup(true);
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  const read = vi.fn().mockRejectedValueOnce(new Error('transient')).mockResolvedValue(known);
  const page = mountHome(snapshot(), homeSummary(), '#/', { setupRead: read });
  await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(1);
  await fireEvent.click(screen.getByRole('button', { name: 'Today' }));
  await flushHomeRender();
  expect(todayCalls(page)).toHaveLength(0);
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(2);
  expect(page.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(new Date(Number(BigInt(snapshot().at) / 1000n))) } });
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(2);
});

it('ignores a retired in-flight setup reply after the replacement stream has failed', async () => {
  const known = await setup(false);
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  const pending = deferred<SetupStatus>();
  const read = vi.fn().mockReturnValueOnce(pending.promise).mockRejectedValue(new Error('transient'));
  const page = mountHome(snapshot(), homeSummary(), '#/', { setupRead: read });
  await flushHomeRender();
  const next = { ...snapshot(), stream_id: 'replacement', sequence: '1' };
  page.client.queryStore = vi.fn(async () => saved('1', next.stream_id));
  page.transport.publishSnapshot(next); await flushHomeRender();
  expect(read).toHaveBeenCalledTimes(2);
  await fireEvent.click(screen.getByRole('button', { name: 'Today' }));
  await flushHomeRender();
  pending.resolve(known); await flushHomeRender();
  expect(todayCalls(page)).toHaveLength(0);
  read.mockResolvedValue(known);
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(page.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(new Date()) } });
});

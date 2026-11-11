import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import type { RecordingStatus } from '../../src/lib/bindings/RecordingStatus';
import { setupFixture } from '../setup/fixture';
import { fakeHistory } from '../navigation/fake-history';
import { advance, flush } from '../phase2/fixtures';
import { createRecordingSession } from '../../src/lib/settings/recording';
import { setNativePresentationVisible } from '../../src/lib/bridge/visibility';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); setNativePresentationVisible(true); vi.clearAllTimers(); vi.useRealTimers(); });

it.each(['disabled', 'failed'] as const)('refreshes a skipped capture-state read on restore from %s', async initial => {
  const f = setupFixture();
  let status: RecordingStatus = { state: initial, failure: initial === 'failed' ? 'disk' : null,
    retained_bytes: 0, limit_bytes: 1_000_000_000, dropped_frames: 0 };
  const query = vi.spyOn(f.client, 'queryRecording').mockImplementation(async () => ({ ...status }));
  const session = createRecordingSession(f.client);
  const seen = vi.fn(), unsubscribe = session.subscribe(seen);
  try {
    await session.refresh();
    expect(query).toHaveBeenCalledOnce();
    setNativePresentationVisible(false);
    status = { ...status, state: 'armed', failure: null };
    await session.refresh(); await session.refresh();
    await advance(60_000);
    expect(query).toHaveBeenCalledOnce();
    setNativePresentationVisible(true); await flush();
    expect(query).toHaveBeenCalledTimes(2);
    expect(seen).toHaveBeenLastCalledWith(expect.objectContaining({ status: expect.objectContaining({ state: 'armed' }), busy: false }));
    await advance(1000);
    expect(query).toHaveBeenCalledTimes(3);
    setNativePresentationVisible(false); await session.refresh();
    session.dispose(); setNativePresentationVisible(true); await advance(1000);
    expect(query).toHaveBeenCalledTimes(3);
  } finally { unsubscribe(); session.dispose(); }
});

it('shares one recording poller across Settings navigation and shows failures in the shell', async () => {
  const f = setupFixture(); f.setSettings({ save_recordings: true });
  let status: RecordingStatus = { state: 'writing', failure: null, retained_bytes: 123,
    limit_bytes: 1_000_000_000, dropped_frames: 0 };
  f.client.queryRecording = vi.fn(async () => ({ ...status }));
  const view = render(Shell, { client: f.client, historyAdapter: fakeHistory('#/').adapter });
  await flush();
  expect(screen.queryByText('Recording', { exact: true })).toBeNull();
  f.capture('Live'); await flush();
  expect(screen.getByText('Recording', { exact: true })).toBeTruthy();
  await fireEvent.click(screen.getByText('Recording', { exact: true })); await flush();
  let calls = vi.mocked(f.client.queryRecording).mock.calls.length;
  await advance(1000);
  expect(f.client.queryRecording).toHaveBeenCalledTimes(calls + 1);
  await fireEvent.click(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Home' })); await flush();
  calls = vi.mocked(f.client.queryRecording).mock.calls.length;
  status = { ...status, state: 'failed', failure: 'disk' };
  await advance(1000);
  expect(f.client.queryRecording).toHaveBeenCalledTimes(calls + 1);
  expect(screen.getByText('Recording Stopped', { exact: true })).toBeTruthy();
  f.capture('Stopped'); await flush();
  expect(screen.queryByText('Recording Stopped', { exact: true })).toBeNull();
  view.unmount();
  calls = vi.mocked(f.client.queryRecording).mock.calls.length;
  await advance(5000);
  expect(f.client.queryRecording).toHaveBeenCalledTimes(calls);
});

it.each([true, false])('hides the recording indicator in demo=%s when disabled', async demo => {
  const f = setupFixture({ demo });
  f.capture('Live');
  f.client.queryRecording = vi.fn(async () => ({ state: demo ? 'writing' as const : 'disabled' as const, failure: null,
    retained_bytes: 0, limit_bytes: 1_000_000_000, dropped_frames: 0 }));
  render(Shell, { client: f.client, historyAdapter: fakeHistory('#/').adapter });
  await flush();
  expect(screen.queryByText('Recording', { exact: true })).toBeNull();
});

it('counter-only capture publications do not refresh recording status', async () => {
  const f = setupFixture(); f.capture('Live');
  f.client.queryRecording = vi.fn(async () => ({ state: 'writing' as const, failure: null,
    retained_bytes: 123, limit_bytes: 1_000_000_000, dropped_frames: 0 }));
  render(Shell, { client: f.client, historyAdapter: fakeHistory('#/').adapter });
  await flush();
  expect(screen.getByText('Recording', { exact: true })).toBeTruthy();
  const calls = vi.mocked(f.client.queryRecording).mock.calls.length;
  const capture = await f.client.queryCapture();
  for (let count = 1; count <= 20; count++) {
    f.transport.publishCapture({ ...capture, dropped_frames: count, unknown_packets: count, decode_failures: count });
    await flush();
  }
  expect(f.client.queryRecording).toHaveBeenCalledTimes(calls);
  await advance(1000);
  expect(f.client.queryRecording).toHaveBeenCalledTimes(calls + 1);
  f.capture('Stopped'); await flush();
  expect(f.client.queryRecording).toHaveBeenCalledTimes(calls + 2);
});

const reply = (state: RecordingStatus['state']): RecordingStatus =>
  ({ state, failure: null, retained_bytes: 0, limit_bytes: 1_000_000_000, dropped_frames: 0 });

it('coalesces refreshes requested during an in-flight read into one trailing read', async () => {
  const f = setupFixture();
  const resolvers: Array<(status: RecordingStatus) => void> = [];
  const query = vi.spyOn(f.client, 'queryRecording').mockImplementation(() => new Promise<RecordingStatus>(resolve => { resolvers.push(resolve); }));
  const session = createRecordingSession(f.client);
  let latest: RecordingStatus | null = null;
  const unsubscribe = session.subscribe(value => { latest = value.status; });
  try {
    const first = session.refresh();
    expect(query).toHaveBeenCalledTimes(1);
    // Three requests arrive while the first read (which will answer 'disabled') is in flight.
    void session.refresh(); void session.refresh(); void session.refresh();
    expect(query).toHaveBeenCalledTimes(1);
    resolvers[0](reply('disabled')); await first; await flush();
    expect(query).toHaveBeenCalledTimes(2); // exactly one trailing read, not three
    resolvers[1](reply('writing')); await flush();
    expect(latest).toMatchObject({ state: 'writing' }); // the newer answer is not lost
    expect(query).toHaveBeenCalledTimes(2); // and nothing loops on
  } finally { unsubscribe(); session.dispose(); }
});

// One controllable queryRecording: each call parks until the test settles it.
function parked(f: ReturnType<typeof setupFixture>) {
  const calls: Array<{ resolve: (status: RecordingStatus) => void; reject: (error: Error) => void }> = [];
  const query = vi.spyOn(f.client, 'queryRecording').mockImplementation(() => new Promise<RecordingStatus>((resolve, reject) => { calls.push({ resolve, reject }); }));
  return { calls, query };
}

it('runs the trailing recording read unless the session was disposed', async () => {
  const f = setupFixture(), { calls, query } = parked(f);
  const kept = createRecordingSession(f.client), disposed = createRecordingSession(f.client);
  try {
    const keptFirst = kept.refresh(), disposedFirst = disposed.refresh();
    expect(query).toHaveBeenCalledTimes(2);
    void kept.refresh(); void disposed.refresh(); // both requested mid-read
    disposed.dispose();
    calls[0].resolve(reply('disabled')); calls[1].resolve(reply('disabled'));
    await keptFirst; await disposedFirst; await flush();
    expect(query).toHaveBeenCalledTimes(3); // only the live session read again
  } finally { kept.dispose(); disposed.dispose(); }
});

it('defers the trailing recording read while hidden and resumes once when visible', async () => {
  const f = setupFixture(), { calls, query } = parked(f);
  const session = createRecordingSession(f.client);
  try {
    const first = session.refresh();
    void session.refresh(); // visible and busy: pending
    setNativePresentationVisible(false);
    calls[0].resolve(reply('disabled')); await first; await flush();
    expect(query).toHaveBeenCalledTimes(1); // finally must not read while hidden
    await advance(60_000);
    expect(query).toHaveBeenCalledTimes(1);
    setNativePresentationVisible(true); await flush();
    expect(query).toHaveBeenCalledTimes(2); // resumed exactly once
    calls[1].resolve(reply('disabled')); await flush(); await advance(60_000);
    expect(query).toHaveBeenCalledTimes(2);
  } finally { session.dispose(); }
});

it('retries once after a failed read with a pending refresh, without looping', async () => {
  const f = setupFixture(), { calls, query } = parked(f);
  const session = createRecordingSession(f.client);
  let latest: { error: string } | null = null;
  const unsubscribe = session.subscribe(value => { latest = value; });
  try {
    const first = session.refresh();
    void session.refresh(); void session.refresh();
    calls[0].reject(new Error('query failed')); await first; await flush();
    expect(query).toHaveBeenCalledTimes(2); // exactly one retry
    calls[1].reject(new Error('query failed')); await flush(); await advance(60_000);
    expect(query).toHaveBeenCalledTimes(2); // the retry's failure does not loop
    expect(latest).toMatchObject({ error: expect.stringContaining('recording') });
  } finally { unsubscribe(); session.dispose(); }
});

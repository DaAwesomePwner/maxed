import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import type { RecordingStatus } from '../../src/lib/bindings/RecordingStatus';
import { setupFixture } from '../setup/fixture';
import { fakeHistory } from '../navigation/fake-history';
import { advance, flush } from '../phase2/fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

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

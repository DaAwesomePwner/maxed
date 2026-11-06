import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { get, readable } from 'svelte/store';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Settings from '../../src/routes/Settings.svelte';
import { SHELL_CONNECTION, type ShellConnection } from '../../src/lib/shell/context';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { createRecordingSession } from '../../src/lib/settings/recording';
import type { RecordingStatus } from '../../src/lib/bindings/RecordingStatus';
import { setupFixture } from '../setup/fixture';
import { advance, deferred, disabled, flush, microtasks, preferences } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

async function open(name: RegExp) {
  const summary = [...document.querySelectorAll('summary')].find(node => name.test(node.textContent ?? ''));
  if (!summary) throw new Error(`Missing Settings disclosure ${name}`);
  if (!summary.parentElement?.hasAttribute('open')) { await fireEvent.click(summary); await microtasks(); }
}

it('phase2_settings_save_race_and_opt_in', async () => {
  const fixture = setupFixture(); const pending = deferred<void>();
  fixture.client.updateSettings = vi.fn(() => pending.promise);
  render(Settings, { client: fixture.client }); await flush();
  const recording = screen.getByRole('checkbox', { name: 'Save Recordings' }) as HTMLInputElement;
  expect(recording.checked).toBe(false);
  const warning = screen.getByText(/passwords and other players' data/i);
  expect(recording.getAttribute('aria-describedby')).toContain(warning.id);
  const acceleration = screen.getByRole('checkbox', { name: 'Hardware Acceleration' }) as HTMLInputElement;
  await fireEvent.click(acceleration); await microtasks();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Capture Adapter' }), { target: { value: 'synthetic-ethernet' } });
  fixture.setSettings({ assets_path: 'C:\\Invented\\resources.assets', alerts: preferences({ muted: true }) });
  await fireEvent.click(screen.getByRole('button', { name: 'Apply' })); await microtasks();
  expect(fixture.client.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ hardware_acceleration: true,
    save_recordings: false, assets_path: 'C:\\Invented\\resources.assets', alerts: preferences({ muted: true }) }));
  expect(vi.mocked(fixture.client.setAdapter).mock.invocationCallOrder[0])
    .toBeLessThan(vi.mocked(fixture.client.updateSettings).mock.invocationCallOrder[0]);
  await fireEvent.click(acceleration); await microtasks();
  pending.resolve(); await flush();
  expect(acceleration.checked).toBe(false);
  expect(disabled(screen.getByRole('button', { name: 'Apply' }))).toBe(false);
  expect(disabled(within(screen.getByRole('button', { name: 'Apply' }).closest('form')!).getByRole('button', { name: 'Revert' }))).toBe(false);
  expect(fixture.client.startCapture).not.toHaveBeenCalled();
  expect(screen.getByRole('link', { name: 'Alert history' }).getAttribute('href')).toBe('#/logs/alerts');
});

it('failed Apply preserves a newer draft and does not report success', async () => {
  const fixture = setupFixture(); const pending = deferred<void>();
  fixture.client.updateSettings = vi.fn(() => pending.promise);
  render(Settings, { client: fixture.client }); await flush();
  const acceleration = screen.getByRole('checkbox', { name: 'Hardware Acceleration' }) as HTMLInputElement;
  await fireEvent.click(acceleration); await fireEvent.click(screen.getByRole('button', { name: 'Apply' })); await microtasks();
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Save Recordings' }));
  pending.reject(new Error('PRIVATE failure')); await flush();
  expect((screen.getByRole('checkbox', { name: 'Save Recordings' }) as HTMLInputElement).checked).toBe(true);
  expect(screen.getByText(/Couldn.t save/)).toBeTruthy();
  expect(screen.queryByText('Saved', { selector: 'p[role="status"]' })).toBeNull(); expect(document.body.textContent).not.toContain('PRIVATE');
  expect(disabled(screen.getByRole('button', { name: 'Apply' }))).toBe(false);
});

it('general Settings Apply refreshes shared alert policy only after a successful save', async () => {
  const fixture = setupFixture();
  const refresh = vi.fn(async () => {});
  const updateAlerts = vi.spyOn(fixture.client, 'updateAlertPreferences');
  const connection: ShellConnection = {
    capture: readable(null), snapshot: readable(null), homeWindow: readable(emptyHomeLiveWindow()),
    selectHomeFight: vi.fn(), reconnect: vi.fn(),
    alerts: { preferences: readable(preferences()), status: readable('ready'), refresh, update: vi.fn(async () => {}), dispose: vi.fn() },
  };
  render(Settings, { props: { client: fixture.client }, context: new Map([[SHELL_CONNECTION, connection]]) });
  await flush();
  expect(refresh).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Hardware Acceleration' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Apply' })); await flush();
  expect(fixture.client.updateSettings).toHaveBeenCalledTimes(1);
  expect(refresh).toHaveBeenCalledTimes(1);
  expect(updateAlerts).not.toHaveBeenCalled();

  refresh.mockClear();
  vi.mocked(fixture.client.updateSettings).mockRejectedValueOnce(new Error('PRIVATE save failure'));
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Hardware Acceleration' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Apply' })); await flush();
  expect(fixture.client.updateSettings).toHaveBeenCalledTimes(2);
  expect(screen.getByText(/Couldn.t save/)).toBeTruthy();
  expect(refresh).not.toHaveBeenCalled();
  expect(updateAlerts).not.toHaveBeenCalled();
  expect(connection.alerts!.update).not.toHaveBeenCalled();
});

it.each(['armed', 'writing'] as const)('phase2_recording_states_poll_and_disposal (%s)', async state => {
  const fixture = setupFixture();
  const status: RecordingStatus = { state, retained_bytes: 123, dropped_frames: 0, limit_bytes: 1_000_000_000, failure: null };
  fixture.client.queryRecording = vi.fn(async () => status);
  const session = createRecordingSession(fixture.client); await session.refresh();
  expect(get(session).status).toEqual(status);
  await advance(999); expect(fixture.client.queryRecording).toHaveBeenCalledTimes(1);
  await advance(1); expect(fixture.client.queryRecording).toHaveBeenCalledTimes(2);
  const pending = deferred<RecordingStatus>(); vi.mocked(fixture.client.queryRecording).mockImplementationOnce(() => pending.promise);
  await advance(1000); expect(fixture.client.queryRecording).toHaveBeenCalledTimes(3);
  await advance(10000); await session.refresh(); expect(fixture.client.queryRecording).toHaveBeenCalledTimes(3);
  session.dispose(); pending.resolve(status); await microtasks();
  await advance(10000); expect(fixture.client.queryRecording).toHaveBeenCalledTimes(3);
  expect(vi.getTimerCount()).toBe(0);
});

it.each(['disabled', 'failed'] as const)('recording %s does not poll or invent inventoried usage', async state => {
  const fixture = setupFixture(); fixture.client.queryRecording = vi.fn(async () => ({
    state, retained_bytes: 0, dropped_frames: 0, limit_bytes: 1_000_000_000, failure: null,
  }));
  render(Settings, { client: fixture.client }); await flush(); await advance(10000);
  expect(fixture.client.queryRecording).toHaveBeenCalledTimes(1);
  if (state === 'disabled') {
    expect(screen.queryByText(/not inventoried|not measured|usage.*unavailable/i)).toBeNull();
    expect(screen.queryByText(/^0 (B|bytes).*retained/i)).toBeNull();
  } else expect(screen.getByText(/last known|stale|failed/i)).toBeTruthy();
});

it('recording polling suspends while the document is hidden', async () => {
  const fixture = setupFixture(); fixture.client.queryRecording = vi.fn(async () => ({
    state: 'armed' as const, retained_bytes: 0, dropped_frames: 0, limit_bytes: 1_000_000_000, failure: null,
  }));
  let hidden = false; vi.spyOn(document, 'hidden', 'get').mockImplementation(() => hidden);
  vi.spyOn(document, 'visibilityState', 'get').mockImplementation(() => hidden ? 'hidden' : 'visible');
  const session = createRecordingSession(fixture.client); await session.refresh();
  hidden = true; document.dispatchEvent(new Event('visibilitychange')); await advance(5000);
  expect(fixture.client.queryRecording).toHaveBeenCalledTimes(1);
  hidden = false; document.dispatchEvent(new Event('visibilitychange')); await advance(1000);
  expect(vi.mocked(fixture.client.queryRecording).mock.calls.length).toBeGreaterThan(1);
  session.dispose(); expect(vi.getTimerCount()).toBe(0);
});

it('phase2_settings_assets_update_and_demo_isolation', async () => {
  const fixture = setupFixture();
  const check = vi.spyOn(fixture.client, 'checkForUpdates');
  const download = vi.spyOn(fixture.client, 'downloadUpdate');
  const install = vi.spyOn(fixture.client, 'installUpdate');
  const view = render(Settings, { client: fixture.client }); await flush();
  expect(check).not.toHaveBeenCalled(); expect(download).not.toHaveBeenCalled(); expect(install).not.toHaveBeenCalled();
  expect(document.querySelector('form form')).toBeNull();
  await open(/Assets Path/i);
  expect(screen.getByRole('textbox', { name: 'Assets Path' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Extract' })).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Recording quota' })).toBeTruthy();
  expect(screen.getByText('Limit: 1 GB')).toBeTruthy();
  expect(screen.getByText('Oldest recordings auto-delete at the limit.')).toBeTruthy();
  expect(screen.queryByText(/Installed version.*1\b/i)).toBeNull();
  view.unmount(); await microtasks();
  render(Settings, { client: fixture.client, preview: true }); await flush(); await open(/Assets Path/i);
  expect(disabled(screen.getByRole('checkbox', { name: 'Save Recordings' }))).toBe(true);
  expect(disabled(screen.getByRole('button', { name: 'Apply' }))).toBe(true);
  expect(disabled(screen.getByRole('switch', { name: 'Demo Mode' }))).toBe(true);
  expect(disabled(screen.getByRole('textbox', { name: 'Assets Path' }))).toBe(true);
  expect(fixture.client.updateSettings).not.toHaveBeenCalled(); expect(fixture.client.setDemo).not.toHaveBeenCalled();
});

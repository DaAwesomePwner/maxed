import { isDebugMode, setDebugMode } from '../../src/lib/debug/debug';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { tick } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { RecordingStatus } from '../../src/lib/bindings/RecordingStatus';
import Settings from '../../src/routes/Settings.svelte';
import { setupFixture } from '../setup/fixture';

afterEach(() => { setDebugMode(false); vi.unstubAllGlobals(); });

describe('local Settings', () => {
  it('keeps both acceleration choices persisted and identifies the test laptop behind the memory estimate', async () => {
    const f = setupFixture(); f.setSettings({ hardware_acceleration: false });
    render(Settings, { client: f.client });
    const toggle = await screen.findByRole('checkbox', { name: 'Hardware Acceleration' }) as HTMLInputElement;
    const note = document.getElementById(toggle.getAttribute('aria-describedby')!)!;
    expect(note.textContent).toMatch(/Restart required/);
    expect(note.textContent).toMatch(/Maxed.s tests/i);
    expect(note.textContent).toMatch(/one Intel Core Ultra 7 laptop/i);
    expect(note.textContent).not.toMatch(/this PC|\bbudget\b/i);
    expect(note.textContent).toMatch(/2026-10-04/);
    expect(note.textContent).toMatch(/WebView2 154/);
    expect(note.textContent).toMatch(/MB/);
    expect(note.textContent).toMatch(/vary|differ/i);
    expect(note.textContent).not.toMatch(/always|every (PC|computer|system)|all hardware/i);
    const user = userEvent.setup();
    for (const enabled of [true, false]) {
      await user.click(toggle);
      await user.click(screen.getByRole('button', { name: 'Apply' }));
      await waitFor(async () => expect((await f.client.querySettings()).hardware_acceleration).toBe(enabled));
      expect(toggle.checked).toBe(enabled);
    }
  });
  it('persists acceleration for the next launch, supports reverting, and disables preview edits', async () => {
    const f = setupFixture(); f.setSettings({ hardware_acceleration: false });
    const view = render(Settings, { client: f.client });
    const toggle = await screen.findByRole('checkbox', { name: 'Hardware Acceleration' }) as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    expect(screen.getByText('Restart required')).toBeTruthy();
    expect(screen.queryByText(/more memory/)).toBeNull();
    const user = userEvent.setup();
    await user.click(toggle);
    await user.click(within(screen.getByRole('button', { name: 'Apply' }).closest('form')!).getByRole('button', { name: 'Revert' }));
    expect(toggle.checked).toBe(false);
    await user.click(toggle);
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText('Saved', { selector: 'p[role="status"]' });
    expect((await f.client.querySettings()).hardware_acceleration).toBe(true);
    expect(f.client.setDemo).not.toHaveBeenCalled();
    expect(f.client.startCapture).not.toHaveBeenCalled();
    view.unmount();
    render(Settings, { client: f.client, preview: true });
    const persisted = await screen.findByRole('checkbox', { name: 'Hardware Acceleration' }) as HTMLInputElement;
    expect(persisted.checked).toBe(true);
    expect(persisted.disabled).toBe(true);
  });

  it('loads persisted adapter/channel, explains off recordings and fixed 1 GB cap, and reverts edits', async () => {
    const f = setupFixture(); render(Settings, { client: f.client });
    const channel = await screen.findByRole('combobox', { name: 'Update Channel' });
    expect((channel as HTMLSelectElement).value).toBe('stable');
    const recordings = screen.getByRole('checkbox', { name: 'Save Recordings' }) as HTMLInputElement;
    expect(recordings.checked).toBe(false); expect(recordings.disabled).toBe(false);
    expect(screen.queryByText(/Recording files can contain/)).toBeNull();
    await userEvent.setup().click(screen.getByText('Retention', { selector: 'summary' }));
    expect(screen.getByText('Limit: 1 GB')).toBeTruthy();
    expect(screen.queryByRole('checkbox', { name: /account|sync/i })).toBeNull();
    const user = userEvent.setup();
    await user.selectOptions(channel, 'beta');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Capture Adapter' }), 'synthetic-ethernet');
    await user.click(within(screen.getByRole('button', { name: 'Apply' }).closest('form')!).getByRole('button', { name: 'Revert' }));
    expect((channel as HTMLSelectElement).value).toBe('stable');
    expect((screen.getByRole('combobox', { name: 'Capture Adapter' }) as HTMLSelectElement).value).toBe('');
    expect(f.client.updateSettings).not.toHaveBeenCalled();
  });

  it('keeps failed edits for retry, sanitizes errors, then reflects saved results', async () => {
    const f = setupFixture();
    const persist = f.client.updateSettings;
    f.client.updateSettings = vi.fn().mockRejectedValueOnce(new Error('PRIVATE settings')).mockImplementation(persist);
    render(Settings, { client: f.client });
    const channel = await screen.findByRole('combobox', { name: 'Update Channel' });
    const user = userEvent.setup(); await user.selectOptions(channel, 'beta');
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText(/Couldn.t save/);
    expect((channel as HTMLSelectElement).value).toBe('beta');
    expect(document.body.textContent).not.toContain('PRIVATE');
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply' }).getAttribute('aria-disabled')).toBe('false'));
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText('Saved', { selector: 'p[role="status"]' });
    expect(f.client.updateSettings).toHaveBeenCalledTimes(2);
    expect(await f.client.querySettings()).toMatchObject({ update_channel: 'beta', save_recordings: false, recording_limit_bytes: 1_000_000_000 });
    expect((within(screen.getByRole('button', { name: 'Apply' }).closest('form')!).getByRole('button', { name: 'Revert' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('preserves independently changed asset paths when applying settings and can turn existing recordings off', async () => {
    const f = setupFixture(); f.setSettings({ save_recordings: true });
    render(Settings, { client: f.client });
    const recordings = await screen.findByRole('checkbox', { name: 'Save Recordings' });
    await userEvent.setup().click(recordings);
    f.setSettings({ assets_path: 'C:\\NewSynthetic\\resources.assets' });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText('Saved', { selector: 'p[role="status"]' });
    expect(f.client.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ save_recordings: false, assets_path: 'C:\\NewSynthetic\\resources.assets' }));
  });

  it('does not invent defaults when loading fails, and retries a read before enabling edits', async () => {
    const f = setupFixture(); const read = f.client.querySettings;
    f.client.querySettings = vi.fn().mockRejectedValueOnce(new Error('PRIVATE settings')).mockImplementation(read);
    render(Settings, { client: f.client });
    await screen.findByText(/Couldn.t load settings/);
    expect(screen.queryByRole('combobox', { name: 'Update Channel' })).toBeNull();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry Settings' }));
    await screen.findByRole('combobox', { name: 'Update Channel' });
    expect(document.body.textContent).not.toContain('PRIVATE');
    expect(f.client.updateSettings).not.toHaveBeenCalled();
  });

  it('does not edit newer settings versions', async () => {
    const f = setupFixture(); f.setSettings({ version: 2 });
    render(Settings, { client: f.client });
    await screen.findByText('Update Required');
    expect((screen.getByRole('combobox', { name: 'Update Channel' }) as HTMLSelectElement).disabled).toBe(true);
    expect(screen.getByRole('button', { name: 'Apply' }).getAttribute('aria-disabled')).toBe('true');
  });

  it('changes demo only through the bridge and displays confirmed mode', async () => {
    const f = setupFixture(); render(Settings, { client: f.client });
    const control = screen.getByRole('switch', { name: 'Demo Mode' });
    await waitFor(() => expect(control.getAttribute('aria-disabled')).toBe('false'));
    await userEvent.setup().click(control);
    await waitFor(() => expect(control.getAttribute('aria-checked')).toBe('true'));
    expect(f.client.setDemo).toHaveBeenCalledWith(true);
    expect(f.client.updateSettings).not.toHaveBeenCalled();
    await waitFor(() => expect(control.getAttribute('aria-disabled')).toBe('false'));
    await userEvent.setup().click(control);
    await waitFor(() => expect(control.getAttribute('aria-checked')).toBe('false'));
    expect(f.client.setDemo).toHaveBeenLastCalledWith(false);
  });

  it('keeps fixture mutations disabled and exposes the setup route', async () => {
    const f = setupFixture(); render(Settings, { client: f.client, preview: true });
    await screen.findByRole('combobox', { name: 'Update Channel' });
    expect(screen.getByRole('link', { name: 'Setup' }).getAttribute('href')).toBe('#/setup');
    expect(screen.getByRole('switch', { name: 'Demo Mode' }).getAttribute('aria-disabled')).toBe('true');
    await userEvent.setup().click(screen.getByText('Assets Path', { selector: 'summary' }));
    expect((screen.getByRole('textbox', { name: 'Assets Path' }) as HTMLInputElement).disabled).toBe(true);
    expect(f.client.updateSettings).not.toHaveBeenCalled();
  });

  it('keeps rejected paths and extraction errors visible while the last valid assets stay ready', async () => {
    const f = setupFixture();
    f.client.setAssetsPath = vi.fn().mockRejectedValue(new Error('PRIVATE secret path'));
    render(Settings, { client: f.client });
    await userEvent.setup().click(screen.getByText('Assets Path', { selector: 'summary' }));
    const path = screen.getByRole('textbox', { name: 'Assets Path' });
    await fireEvent.input(path, { target: { value: 'C:\\Synthetic\\resources.assets' } });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Extract' }));
    await screen.findByText(/Couldn.t extract assets/);
    expect(screen.getByText('Assets Ready')).toBeTruthy();
    await userEvent.setup().click(screen.getByText('Recovery', { selector: 'summary' }));
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry Checks' }));
    expect(screen.getByRole('alert').textContent).toContain('Couldn’t extract assets');
    expect((path as HTMLInputElement).value).toBe('C:\\Synthetic\\resources.assets');
    expect(document.body.textContent).not.toContain('PRIVATE');
    expect(screen.getByRole('alert').textContent).not.toContain('C:\\');
  });

  it('retains an Apply failure and unsaved channel after capture becomes live', async () => {
    const f = setupFixture();
    f.client.updateSettings = vi.fn().mockRejectedValue(new Error('PRIVATE settings'));
    render(Settings, { client: f.client });
    const channel = await screen.findByRole('combobox', { name: 'Update Channel' }) as HTMLSelectElement;
    const user = userEvent.setup();
    await user.selectOptions(channel, 'beta');
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText(/Couldn.t save/);
    await waitFor(() => expect(screen.getByRole('button', { name: 'Apply' }).getAttribute('aria-disabled')).toBe('false'));
    f.capture('WaitingForGame'); await tick();
    f.capture('Live'); await tick();
    expect(screen.getByRole('alert').textContent).toContain('Couldn’t save');
    expect(channel.value).toBe('beta');
    expect((await f.client.querySettings()).update_channel).toBe('stable');
  });
});


it.each([
  ['in_use', 'Close other Maxed instances.'],
  ['quota', 'Check recording storage.'],
  ['queue_full', 'Toggle recording to retry.'],
  ['disk', 'Check disk space and access.'],
  ['invalid_frame', 'Recording Stopped'],
] as const)('explains recording failure %s without a raw code', async (failure, message) => {
  const f = setupFixture();
  f.client.queryRecording = vi.fn(async () => ({ state: 'failed' as const, failure, retained_bytes: 123,
    limit_bytes: 1_000_000_000, dropped_frames: 5 }));
  render(Settings, { client: f.client });
  expect(document.querySelector('[data-debug-note]')).toBeNull();
  setDebugMode(true);
  const explanation = await screen.findByText(text => text.includes(message));
  expect(explanation.textContent).not.toContain(`Failure: ${failure}`);
  expect(screen.getByText('5 packets were skipped in the last recording.')).toBeTruthy();
  expect(screen.queryByText(/the recording continues/)).toBeNull();
});

it('shows skipped packets only in debug mode while recording continues', async () => {
  setDebugMode(true);
  const f = setupFixture();
  f.client.queryRecording = vi.fn(async () => ({ state: 'writing' as const, failure: null, retained_bytes: 123,
    limit_bytes: 1_000_000_000, dropped_frames: 5000 }));
  render(Settings, { client: f.client });
  expect(await screen.findByText('5,000 packets were skipped during traffic bursts or recording changes; the recording continues.')).toBeTruthy();
});


it.each(['armed', 'disabled'] as const)('uses past wording after recording stops (%s)', async stopped => {
  setDebugMode(true);
  const f = setupFixture();
  let state: RecordingStatus['state'] = 'writing';
  f.client.queryRecording = vi.fn(async (): Promise<RecordingStatus> => ({ state, failure: null,
    retained_bytes: 123, limit_bytes: 1_000_000_000, dropped_frames: 37 }));
  render(Settings, { client: f.client });
  expect(await screen.findByText('37 packets were skipped during traffic bursts or recording changes; the recording continues.')).toBeTruthy();
  state = stopped;
  await fireEvent.click(screen.getByText('Retention', { selector: 'summary' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Retry Recording' }));
  expect(await screen.findByText('37 packets were skipped in the last recording.')).toBeTruthy();
  expect(screen.queryByText(/the recording continues/)).toBeNull();
});


it('applies Debug immediately without editing or saving app settings', async () => {
  const values = new Map<string, string>();
  vi.stubGlobal('localStorage', { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value), removeItem: (key: string) => values.delete(key) });
  const f = setupFixture();
  render(Settings, { client: f.client });
  await screen.findByRole('combobox', { name: 'Update Channel' });
  const toggle = screen.getByRole('checkbox', { name: 'Debug mode' }) as HTMLInputElement;
  const apply = screen.getByRole('button', { name: 'Apply' });
  expect(toggle.checked).toBe(false);
  expect(isDebugMode()).toBe(false);
  expect(document.querySelector('[data-debug-note]')).toBeNull();
  await fireEvent.click(toggle);
  expect(isDebugMode()).toBe(true);
  expect(window.localStorage.getItem('maxed.debug')).toBe('1');
  expect(await screen.findByText('Disk usage is not inventoried.')).toBeTruthy();
  expect(apply.getAttribute('aria-disabled')).toBe('true');
  expect(screen.queryByText('Unsaved Changes')).toBeNull();
  expect(f.client.updateSettings).not.toHaveBeenCalled();
  await fireEvent.click(toggle);
  expect(isDebugMode()).toBe(false);
  expect(window.localStorage.getItem('maxed.debug')).toBeNull();
  expect(document.querySelector('[data-debug-note]')).toBeNull();
  expect(apply.getAttribute('aria-disabled')).toBe('true');
});

it('shows Chat with either Logs flag state while gating alert preferences', async () => {
  const f = setupFixture();
  const view = render(Settings, { client: f.client, features: { lootBrowsing: true, logs: false, stats: false } });
  await screen.findByRole('checkbox', { name: 'Hardware Acceleration' });
  expect(screen.getByRole('heading', { name: 'Chat' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Clear chat history' })).toBeTruthy();
  const unflaggedUpdate = vi.spyOn(f.client, 'updateChatSettings');
  await fireEvent.input(screen.getByLabelText('Keep chat for (days)'), { target: { value: '60' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(unflaggedUpdate).toHaveBeenCalledWith(expect.objectContaining({ retention_days: 60 })));
  expect(screen.queryByRole('heading', { name: 'Presentation' })).toBeNull();
  expect(screen.getByRole('link', { name: 'Alerts' }).getAttribute('href')).toBe('#/alerts');
  await view.rerender({ client: f.client, features: { logs: true, stats: false, lootBrowsing: true } });
  expect(await screen.findByRole('heading', { name: 'Chat' })).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Presentation' })).toBeTruthy();
  expect(screen.getByRole('link', { name: 'Alert history' }).getAttribute('href')).toBe('#/logs/alerts');
  const update = vi.spyOn(f.client, 'updateChatSettings');
  await fireEvent.input(screen.getByLabelText('Keep chat for (days)'), { target: { value: '30' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({ retention_days: 30 })));
});

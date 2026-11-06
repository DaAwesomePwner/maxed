import AlertPreferences from '../../src/lib/alerts/AlertPreferences.svelte';
import { createAlertSession } from '../../src/lib/alerts/session';
import { isDebugMode, setDebugMode } from '../../src/lib/debug/debug';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RecordingStatus } from '../../src/lib/bindings/RecordingStatus';
import Settings from '../../src/routes/Settings.svelte';
import { deferred, setupFixture } from '../setup/fixture';
import type { SetupStatus } from '../../src/lib/bindings/SetupStatus';

beforeEach(() => { localStorage.removeItem('maxed.settings.tab'); window.history.replaceState(null, '', '#/settings'); });
const switchTab = (name: string) => fireEvent.click(screen.getByRole('tab', { name }));
afterEach(() => { setDebugMode(false); vi.unstubAllGlobals(); });

it('mounts storage without maintenance and disables Demo Mode during an upgrade', async () => {
  const f = setupFixture();
  f.transport.setHistoryUpgradeFixture({ schemaVersion: 10, freeBytes: '1000000000' });
  await f.client.backupAndUpgradeHistory();
  const compact = vi.spyOn(f.client, 'compactStorage');
  render(Settings, { client: f.client });
  await screen.findByText('Demo Mode is unavailable while the history upgrade is running.');
  const demo = screen.getByRole('switch', { name: 'Demo Mode' });
  expect(demo.getAttribute('aria-disabled')).toBe('true');
  await fireEvent.click(demo);
  expect(f.client.setDemo).not.toHaveBeenCalled(); expect(compact).not.toHaveBeenCalled();
  expect(screen.getByText('Reusable pages')).toBeTruthy();
});

it('settings_apply_preserves_recap_image', async () => {
  const f = setupFixture();
  render(Settings, { client: f.client });
  await screen.findByRole('checkbox', { name: 'Hardware Acceleration' });
  const recap_image = { names: false, loot: false, fights: false };
  f.setSettings({ recap_image });
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Hardware Acceleration' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
  await waitFor(() => expect(f.client.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ recap_image, hardware_acceleration: true })));
});

it.each([false, true])('clears Party history with the tab hidden and marks_and_notes=%s', async checked => {
  const f = setupFixture();
  const clear = vi.spyOn(f.client, 'clearPartyHistory').mockResolvedValue(undefined);
  render(Settings, { client: f.client, features: { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: false } });
  await switchTab('Data & history');
  const trigger = screen.getByRole('button', { name: 'Clear party history' });
  expect(screen.getByText("Maxed links players you've seen across runs by name. This removes that history and other players' stored gear and stats from your runs.")).toBeTruthy();
  await fireEvent.click(trigger);
  const dialog = within(screen.getByRole('dialog', { name: 'Clear party history' }));
  const checkbox = dialog.getByRole('checkbox', { name: 'Also clear marks and notes' }) as HTMLInputElement;
  expect(checkbox.checked).toBe(false);
  if (checked) await fireEvent.click(checkbox);
  await fireEvent.click(dialog.getByRole('button', { name: 'Confirm' }));
  await screen.findByText('Party history cleared', { selector: '[role=status]' });
  expect(clear).toHaveBeenCalledExactlyOnceWith({ marks_and_notes: checked });
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(document.activeElement).toBe(trigger);
});

it('cancels Party clear and shows a safe error with retry and success feedback', async () => {
  const f = setupFixture();
  const clear = vi.spyOn(f.client, 'clearPartyHistory').mockRejectedValueOnce(new Error('PRIVATE')).mockResolvedValue(undefined);
  render(Settings, { client: f.client });
  await switchTab('Data & history');
  const trigger = screen.getByRole('button', { name: 'Clear party history' });
  await fireEvent.click(trigger);
  await fireEvent.click(within(screen.getByRole('dialog', { name: 'Clear party history' })).getByRole('button', { name: 'Cancel' }));
  expect(clear).not.toHaveBeenCalled();
  await fireEvent.click(trigger);
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  const message = 'Could not clear party history. Retry to clear it.';
  await screen.findByText(message, { selector: '[role=status]' });
  expect(within(screen.getByRole('dialog')).getByRole('alert').textContent).toBe(message);
  expect(document.body.textContent).not.toContain('PRIVATE');
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  await screen.findByText('Party history cleared');
});

it('disables Party clearing in Settings preview', async () => {
  const f = setupFixture();
  render(Settings, { client: f.client, preview: true });
  await switchTab('Data & history');
  expect((screen.getByRole('button', { name: 'Clear party history' }) as HTMLButtonElement).disabled).toBe(true);
  const hint = screen.getByText('Clearing is not available in preview.');
  expect(hint.hidden).toBe(false);
  expect(screen.getByRole('button', { name: 'Clear party history' }).getAttribute('aria-describedby')).toBe(hint.id);
});

it.each(['success', 'failure'] as const)('keeps Settings Party clear disabled while setup is pending, then handles %s', async outcome => {
  const f = setupFixture(), setup = await f.client.querySetup(), pending = deferred<SetupStatus>();
  vi.mocked(f.client.querySetup).mockReturnValue(pending.promise);
  render(Settings, { client: f.client });
  await switchTab('Data & history');
  const button = screen.getByRole('button', { name: 'Clear party history' }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  button.click(); expect(screen.queryByRole('dialog')).toBeNull();
  if (outcome === 'success') pending.resolve(setup);
  else pending.reject(new Error('PRIVATE'));
  if (outcome === 'success') await waitFor(() => expect(button.disabled).toBe(false));
  else {
    const hint = await screen.findByText('Could not check Demo Mode. Reload to try again.');
    expect(button.disabled).toBe(true); expect(hint.hidden).toBe(false);
    expect(button.getAttribute('aria-describedby')).toBe(hint.id);
    expect(document.body.textContent).not.toContain('PRIVATE');
  }
});

it('disables Party clearing in Demo Mode until Demo Mode is turned off', async () => {
  const f = setupFixture({ demo: true });
  const clear = vi.spyOn(f.client, 'clearPartyHistory').mockResolvedValue(undefined);
  render(Settings, { client: f.client });
  await switchTab('Data & history');
  const trigger = screen.getByRole('button', { name: 'Clear party history' }) as HTMLButtonElement;
  await waitFor(() => expect(trigger.disabled).toBe(true));
  const hint = screen.getByText('Turn off Demo Mode to clear your party history.');
  expect(hint.hidden).toBe(false);
  expect(trigger.getAttribute('aria-describedby')).toBe(hint.id);
  const user = userEvent.setup();
  await user.click(trigger);
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(clear).not.toHaveBeenCalled();
  await switchTab('General');
  const demo = screen.getByRole('switch', { name: 'Demo Mode' });
  await waitFor(() => expect(demo.getAttribute('aria-disabled')).toBe('false'));
  await user.click(demo);
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(screen.queryByText('Turn off Demo Mode to clear your party history.')).toBeNull();
  expect(trigger.hasAttribute('aria-describedby')).toBe(false);
  await switchTab('Data & history');
  await user.click(trigger);
  expect(screen.getByRole('dialog', { name: 'Clear party history' })).toBeTruthy();
});

it.each(['Cancel', 'Escape'])('clears a failed Party clear message when closed with %s', async action => {
  const f = setupFixture();
  const clear = vi.spyOn(f.client, 'clearPartyHistory').mockRejectedValue(new Error('Synthetic failure'));
  render(Settings, { client: f.client });
  await switchTab('Data & history');
  const trigger = screen.getByRole('button', { name: 'Clear party history' });
  await fireEvent.click(trigger);
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  const message = 'Could not clear party history. Retry to clear it.';
  await screen.findByText(message, { selector: '[role=status]' });
  if (action === 'Cancel') await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' }));
  else await fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.queryByText(message)).toBeNull();
  expect(document.activeElement).toBe(trigger);
  expect(clear).toHaveBeenCalledTimes(1);
});

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
    await screen.findByText('Saved', { selector: '[role="status"]' });
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
    await switchTab('Data & history');
    const recordings = screen.getByRole('checkbox', { name: 'Save Recordings' }) as HTMLInputElement;
    expect(recordings.checked).toBe(false); expect(recordings.disabled).toBe(false);
    expect(screen.queryByText(/Recording files can contain/)).toBeNull();
    expect(screen.getByText('Limit: 1 GB')).toBeTruthy();
    expect(screen.queryByRole('checkbox', { name: /account|sync/i, hidden: true })).toBeNull();
    await switchTab('General');
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
    await screen.findByText('Saved', { selector: '[role="status"]' });
    expect(f.client.updateSettings).toHaveBeenCalledTimes(2);
    expect(await f.client.querySettings()).toMatchObject({ update_channel: 'beta', save_recordings: false, recording_limit_bytes: 1_000_000_000 });
    expect((within(screen.getByRole('button', { name: 'Apply' }).closest('form')!).getByRole('button', { name: 'Revert' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('preserves independently changed asset paths when applying settings and can turn existing recordings off', async () => {
    const f = setupFixture(); f.setSettings({ save_recordings: true });
    render(Settings, { client: f.client });
    await switchTab('Data & history');
    const recordings = await screen.findByRole('checkbox', { name: 'Save Recordings' });
    await userEvent.setup().click(recordings);
    f.setSettings({ assets_path: 'C:\\NewSynthetic\\resources.assets' });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText('Saved', { selector: '[role="status"]' });
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
    await switchTab('Game assets');
    await userEvent.setup().click(screen.getByText('Assets Path', { selector: 'summary' }));
    expect((screen.getByRole('textbox', { name: 'Assets Path' }) as HTMLInputElement).disabled).toBe(true);
    expect(f.client.updateSettings).not.toHaveBeenCalled();
  });

  it('keeps rejected paths and extraction errors visible while the last valid assets stay ready', async () => {
    const f = setupFixture();
    f.client.setAssetsPath = vi.fn().mockRejectedValue(new Error('PRIVATE secret path'));
    render(Settings, { client: f.client });
    await switchTab('Game assets');
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
    await switchTab('General');
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
  await switchTab('Data & history');
  state = stopped;
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
  const view = render(Settings, { client: f.client, features: { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: false } });
  await screen.findByRole('checkbox', { name: 'Hardware Acceleration' });
  await switchTab('Data & history');
  expect(screen.getByRole('heading', { name: 'Chat' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Clear chat history' })).toBeTruthy();
  const unflaggedUpdate = vi.spyOn(f.client, 'updateChatSettings');
  await fireEvent.input(screen.getByLabelText('Keep chat for (days)'), { target: { value: '60' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(unflaggedUpdate).toHaveBeenCalledWith(expect.objectContaining({ retention_days: 60 })));
  expect(screen.queryByRole('heading', { name: 'Presentation', hidden: true })).toBeNull();
  expect(screen.getByRole('link', { name: 'Alerts' }).getAttribute('href')).toBe('#/alerts');
  await view.rerender({ client: f.client, features: { logs: true, stats: false, party: false, character: false, homeRedesign: false, lootBrowsing: true } });
  expect(await screen.findByRole('heading', { name: 'Chat' })).toBeTruthy();
  await switchTab('Notifications');
  expect(screen.getByRole('heading', { name: 'Presentation' })).toBeTruthy();
  await switchTab('Data & history');
  expect(screen.getByRole('link', { name: 'Alert history' }).getAttribute('href')).toBe('#/logs/alerts');
  const update = vi.spyOn(f.client, 'updateChatSettings');
  await fireEvent.input(screen.getByLabelText('Keep chat for (days)'), { target: { value: '30' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({ retention_days: 30 })));
});

it('defaults key-pop alerts to Off and prompts for an empty dungeon selection', async () => {
  const f = setupFixture();
  const session = createAlertSession(f.client);
  await session.refresh();
  const view = render(AlertPreferences, { client: f.client, session });
  const section = await screen.findByRole('region', { name: 'Key Popped' });
  const loudness = within(section).getByLabelText('Key Popped') as HTMLSelectElement;
  await waitFor(() => expect(loudness.value).toBe('off'));
  expect(within(section).getByText('Key-pop alerts are off. Choose dungeons, then select Toast or Banner and save to enable them.')).toBeTruthy();
  expect(within(section).getByText('Choose dungeons to receive key-pop alerts. None selected means no alerts.')).toBeTruthy();
  await waitFor(() => expect(loudness.disabled).toBe(false));
  await fireEvent.change(loudness, { target: { value: 'toast' } });
  await waitFor(() => expect(within(section).queryByText(/^Key-pop alerts are off/)).toBeNull());
  expect(within(section).getByText('Choose dungeons to receive key-pop alerts. None selected means no alerts.')).toBeTruthy();
  view.unmount(); session.dispose();
});

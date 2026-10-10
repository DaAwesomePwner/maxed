import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { get } from 'svelte/store';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import Setup from '../../src/routes/Setup.svelte';
import Settings from '../../src/routes/Settings.svelte';
import { createSetupSession } from '../../src/lib/setup/session';
import { setupFixture, deferred } from './fixture';

describe('U5 review regressions', () => {
  it('applies adapter runtime changes before saving other fields, and skips unchanged adapters', async () => {
    const f = setupFixture();
    f.setSettings({ adapter_id: 'synthetic-ethernet' });
    render(Settings, { client: f.client });
    const adapter = await screen.findByRole('combobox', { name: 'Capture Adapter' });
    const user = userEvent.setup();
    await user.selectOptions(adapter, '');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Update Channel' }), 'beta');
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText('Saved', { selector: 'p[role="status"]' });
    expect(f.client.setAdapter).toHaveBeenCalledExactlyOnceWith(null);
    expect(f.client.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ adapter_id: null, update_channel: 'beta' }));
    expect(vi.mocked(f.client.setAdapter).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(f.client.updateSettings).mock.invocationCallOrder[0]);
    await user.selectOptions(screen.getByRole('combobox', { name: 'Update Channel' }), 'stable');
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText('Saved', { selector: 'p[role="status"]' });
    expect(f.client.setAdapter).toHaveBeenCalledOnce();
    expect(f.client.updateSettings).toHaveBeenCalledTimes(2);
  });

  it('does not save other fields after adapter rejection and retains the selection for retry', async () => {
    const f = setupFixture();
    f.client.setAdapter = vi.fn().mockRejectedValue(new Error('PRIVATE adapter'));
    render(Settings, { client: f.client });
    await screen.findByRole('option', { name: 'Synthetic Ethernet' });
    const adapter = screen.getByRole('combobox', { name: 'Capture Adapter' }) as HTMLSelectElement;
    const user = userEvent.setup();
    await user.selectOptions(adapter, 'synthetic-ethernet');
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await screen.findByText(/Couldn.t save/);
    expect(adapter.value).toBe('synthetic-ethernet');
    expect(f.client.updateSettings).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toContain('PRIVATE');
  });

  it('clears failed actions on new capture state and failed extraction on ready evidence', async () => {
    vi.useFakeTimers();
    const f = setupFixture({ assets: 'failed' });
    const session = createSetupSession(f.client);
    session.start(); await vi.advanceTimersByTimeAsync(0);
    await session.action(async () => { throw new Error(); }, 'Capture could not start.', { captureAction: true });
    expect(get(session).errors.action).toBe('Capture could not start.');
    f.capture('Stopped'); // Same state/counter updates do not erase an actionable failure.
    expect(get(session).errors.action).toBe('Capture could not start.');
    f.capture('WaitingForGame');
    expect(get(session).errors.action).toBe('');
    f.client.setAssetsPath = vi.fn().mockRejectedValue(new Error());
    await session.selectAssets('C:\\Synthetic\\resources.assets');
    expect(get(session).errors.assets).toContain('Couldn’t extract assets');
    f.setSetup({ assets: 'ready' });
    await session.refreshSetup();
    expect(get(session).errors.assets).toBe('');
    session.dispose();
  });

  it.each(['setup', 'settings'] as const)('does not enumerate adapters or offer Npcap error guidance in %s preview', async (route) => {
    const f = setupFixture();
    f.client.listAdapters = vi.fn().mockRejectedValue(new Error('unsupported fixture command'));
    if (route === 'setup') render(Setup, { client: f.client, preview: true });
    else render(Settings, { client: f.client, preview: true });
    await screen.findAllByText('Preview');
    expect(f.client.listAdapters).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Retry Adapters' })).toBeNull();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe('');
    const session = createSetupSession(f.client, true);
    await session.refreshAdapters(); session.dispose();
    expect(f.client.listAdapters).not.toHaveBeenCalled();
  });

  it.each(['ready', 'extracting'] as const)('retains rejection through the final ready refresh after observing %s', async (observed) => {
    vi.useFakeTimers();
    const f = setupFixture();
    const session = createSetupSession(f.client);
    await session.refreshSetup();
    const pending = deferred<void>();
    f.client.setAssetsPath = vi.fn(() => pending.promise);
    const extraction = session.selectAssets('C:\\Rejected\\resources.assets');
    f.setSetup({ assets: observed });
    await session.refreshSetup();
    f.setSetup({ assets: 'ready' });
    pending.reject(new Error('PRIVATE rejected selection'));
    expect(await extraction).toBe(false);
    expect(get(session).setup?.assets).toBe('ready');
    expect(get(session).errors.assets).toContain('Couldn’t extract assets');
    await session.refreshSetup();
    expect(get(session).errors.assets).toContain('Couldn’t extract assets');
    // A later observed extraction followed by ready is fresh recovery evidence.
    f.setSetup({ assets: 'extracting' }); await session.refreshSetup();
    f.setSetup({ assets: 'ready' }); await session.refreshSetup();
    expect(get(session).errors.assets).toBe('');
    session.dispose();
  });

  it.each(['Settings apply', 'Adapter change', 'Demo toggle'])('keeps a %s failure through capture transitions', async (label) => {
    vi.useFakeTimers();
    const f = setupFixture();
    const session = createSetupSession(f.client);
    session.start(); await vi.advanceTimersByTimeAsync(0);
    const failure = `${label} failed.`;
    await session.action(async () => { throw new Error(); }, failure);
    f.capture('WaitingForGame'); await vi.advanceTimersByTimeAsync(0);
    f.capture('Live'); await vi.advanceTimersByTimeAsync(0);
    expect(get(session).errors.action).toBe(failure);
    session.dispose();
  });

  it('resyncs a successful assets selection to the normalized and externally updated saved path', async () => {
    const f = setupFixture();
    f.client.setAssetsPath = vi.fn(async () => { f.setSettings({ assets_path: 'C:\\Normalized\\resources.assets' }); });
    render(Settings, { client: f.client });
    await screen.findByRole('combobox', { name: 'Update Channel' });
    const user = userEvent.setup();
    const assets = screen.getByText('Assets Path', { selector: 'summary' });
    await user.click(assets);
    expect(assets.closest('details')?.open).toBe(true);
    const input = screen.getByRole('textbox', { name: 'Assets Path' }) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: ' C:\\Synthetic\\resources.assets ' } });
    await user.click(screen.getByRole('button', { name: 'Extract' }));
    await waitFor(() => expect(input.value).toBe('C:\\Normalized\\resources.assets'));
    expect((screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement).disabled).toBe(true);
    f.setSettings({ assets_path: 'C:\\External\\resources.assets' });
    const recovery = screen.getByText('Recovery', { selector: 'summary' });
    await user.click(recovery);
    expect(recovery.closest('details')?.open).toBe(true);
    await user.click(screen.getByRole('button', { name: 'Retry Settings' }));
    await waitFor(() => expect(input.value).toBe('C:\\External\\resources.assets'));
  });

  it('does not discard newer path edits when a submitted extraction finishes', async () => {
    const f = setupFixture(); const pending = deferred<void>();
    f.client.setAssetsPath = vi.fn(() => pending.promise);
    render(Settings, { client: f.client });
    await screen.findByRole('combobox', { name: 'Update Channel' });
    const user = userEvent.setup();
    const assets = screen.getByText('Assets Path', { selector: 'summary' });
    await user.click(assets);
    expect(assets.closest('details')?.open).toBe(true);
    const input = screen.getByRole('textbox', { name: 'Assets Path' }) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'C:\\First\\resources.assets' } });
    await user.click(screen.getByRole('button', { name: 'Extract' }));
    await fireEvent.input(input, { target: { value: 'C:\\Next\\resources.assets' } });
    pending.resolve();
    await screen.findByRole('button', { name: 'Extract' });
    expect(input.value).toBe('C:\\Next\\resources.assets');
    expect((screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('omits P5 sound controls and keeps both route sources free of BOMs', async () => {
    render(Settings, { client: setupFixture().client });
    expect(screen.queryByRole('heading', { name: 'Sounds and notifications' })).toBeNull();
    expect(screen.queryByText('Synthetic data and session behavior')).toBeNull();
    for (const route of ['Setup', 'Settings']) {
      expect(readFileSync(resolve(process.cwd(), `src/routes/${route}.svelte`), 'utf8').startsWith('<script lang="ts">')).toBe(true);
    }
  });
});

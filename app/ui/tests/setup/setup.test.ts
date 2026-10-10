import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import Setup from '../../src/routes/Setup.svelte';
import type { SetupStatus } from '../../src/lib/bindings/SetupStatus';
import { setupFixture, deferred } from './fixture';

describe('three-step setup', () => {
  it.each([
    [{ npcap: 'missing' }, 'Install Npcap'],
    [{ npcap: 'incompatible' }, 'Repair Npcap'],
    [{ npcap: 'permission_denied', access: 'denied' }, 'Access Denied'],
    [{ npcap: 'compatible', access: 'unknown' }, 'Checking Access'],
    [{ npcap: 'failed' }, 'Couldn’t check Npcap. Retry.'],
    [{ npcap: 'unchecked' }, 'Unchecked'],
  ] satisfies [Partial<SetupStatus>, string][])('shows actionable Npcap evidence %j', async (state, text) => {
    const { client } = setupFixture(state);
    render(Setup, { client });
    await screen.findByText(text);
    const steps = screen.getByRole('list', { name: 'Setup' });
    expect(within(steps).getAllByRole('listitem')).toHaveLength(3);
    expect(within(steps).getAllByRole('listitem')[0].getAttribute('aria-current')).toBe('step');
    if (['missing', 'incompatible', 'permission_denied'].includes(state.npcap!)) {
      expect((screen.getByRole('textbox', { name: 'Npcap URL' }) as HTMLInputElement).value).toBe('https://npcap.com/#download');
      expect(document.querySelector('a[href^="https:"]')).toBeNull();
    }
    expect(screen.queryByText('Connected')).toBeNull();
  });

  it.each([
    [{ assets: 'missing', client_found: false }, 'Select Client'],
    [{ assets: 'finding' }, 'Finding Client'],
    [{ assets: 'unsupported' }, 'Unsupported Client'],
    [{ assets: 'failed' }, 'Couldn’t extract assets. Retry.'],
    [{ assets: 'ready', client_found: false }, 'Select Client'],
  ] satisfies [Partial<SetupStatus>, string][])('handles client and asset evidence %j', async (state, text) => {
    const { client } = setupFixture(state);
    render(Setup, { client });
    await screen.findByText(text);
    expect(screen.queryByText('Connected')).toBeNull();
  });

  it.each(['fingerprinting', 'reading', 'catalog', 'sprites', 'publishing'] as const)('announces %s without invented percentages', async (stage) => {
    const { client } = setupFixture({ assets: 'extracting', extraction_stage: stage });
    render(Setup, { client });
    const status = await screen.findByText(`Extracting: ${stage[0].toUpperCase()}${stage.slice(1)}.`);
    expect(status.getAttribute('role')).toBe('status');
    expect(screen.queryByRole('progressbar')).toBeNull();
    expect(screen.getByRole('button', { name: 'Replace' })).toBeTruthy();
  });

  it('separates empty adapters, enumeration errors and permission checks', async () => {
    const { client } = setupFixture({ access: 'unknown' });
    client.listAdapters = vi.fn().mockResolvedValueOnce([]).mockRejectedValueOnce(new Error('PRIVATE adapter'));
    render(Setup, { client });
    await screen.findByText('No Adapters');
    expect(screen.getByRole('option', { name: 'Auto' })).toBeTruthy();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Check Access' }));
    expect(client.startCapture).toHaveBeenCalledOnce();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry Adapters' }));
    await screen.findByText(/Couldn.t list adapters/);
    expect(document.body.textContent).not.toContain('PRIVATE');
    expect(screen.queryByText('Connected')).toBeNull();
  });

  it('requires decoded live evidence, revokes confirmation on recovery and stop, and labels only observed locations', async () => {
    const f = setupFixture();
    render(Setup, { client: f.client });
    await screen.findByText('Assets Ready');
    expect(screen.queryByText('Connected')).toBeNull();
    expect(screen.queryByText('Location not yet observed.')).toBeNull();
    f.capture('Live');
    await screen.findByText('Capture: Live');
    expect(screen.queryByText('Connected')).toBeNull();
    f.setSetup({ live_confirmed: true });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry Checks' }));
    await screen.findByText('Connected');
    f.capture('Recovering');
    await screen.findByText('Reconnecting');
    expect(screen.queryByText('Connected')).toBeNull();
    f.capture('Stopped');
    await screen.findByText('Capture: Stopped');
    expect(screen.queryByText('Connected')).toBeNull();
  });

  it.each([false, true])('never completes demo/fixture Live (preview=%s)', async (preview) => {
    const f = setupFixture({ demo: !preview, live_confirmed: true });
    f.capture('Live');
    render(Setup, { client: f.client, preview });
    await screen.findByText('Capture: Live');
    expect(screen.getByText('Demo')).toBeTruthy();
    expect(screen.queryByText('Connected')).toBeNull();
  });

  it('sanitizes probe rejection, retries without extraction, and retains initiating focus', async () => {
    const f = setupFixture();
    f.client.querySetup = vi.fn().mockRejectedValue(new Error('PRIVATE path'));
    render(Setup, { client: f.client });
    await screen.findByText(/Couldn.t check setup/);
    const retry = screen.getByRole('button', { name: 'Retry Checks' });
    await userEvent.setup().click(retry);
    expect(document.activeElement).toBe(retry);
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(document.body.textContent).not.toContain('PRIVATE');
    expect(f.client.setAssetsPath).not.toHaveBeenCalled();
  });

  it('applies an explicit adapter and Auto without re-extracting assets', async () => {
    const f = setupFixture(); render(Setup, { client: f.client });
    await screen.findByRole('option', { name: 'Synthetic Ethernet' });
    const adapter = screen.getByRole('combobox', { name: 'Capture Adapter' });
    const user = userEvent.setup();
    await user.selectOptions(adapter, 'synthetic-ethernet');
    await user.click(screen.getByRole('button', { name: 'Apply Adapter' }));
    await waitFor(() => expect(f.client.setAdapter).toHaveBeenCalledWith('synthetic-ethernet'));
    await waitFor(() => expect((adapter as HTMLSelectElement).disabled).toBe(false));
    await user.selectOptions(adapter, '');
    await user.click(screen.getByRole('button', { name: 'Apply Adapter' }));
    await waitFor(() => expect(f.client.setAdapter).toHaveBeenLastCalledWith(null));
    expect(f.client.setAssetsPath).not.toHaveBeenCalled();
  });

  it('keeps failed Start and Stop commands retryable with fixed explanations', async () => {
    const f = setupFixture();
    const start = f.client.startCapture, stop = f.client.stopCapture;
    f.client.startCapture = vi.fn().mockRejectedValueOnce(new Error('PRIVATE Start')).mockImplementation(start);
    f.client.stopCapture = vi.fn().mockRejectedValueOnce(new Error('PRIVATE Stop')).mockImplementation(stop);
    render(Setup, { client: f.client });
    const user = userEvent.setup();
    const startButton = screen.getByRole('button', { name: 'Start Capture' });
    await waitFor(() => expect(startButton.getAttribute('aria-disabled')).toBe('false'));
    await user.click(startButton);
    await screen.findByText(/Couldn.t start capture/);
    await waitFor(() => expect(startButton.getAttribute('aria-disabled')).toBe('false'));
    await user.click(startButton);
    await screen.findByText('Capture: Waiting for Game');
    const stopButton = screen.getByRole('button', { name: 'Stop Capture' });
    await waitFor(() => expect(stopButton.getAttribute('aria-disabled')).toBe('false'));
    await user.click(stopButton);
    await screen.findByText('Couldn’t stop capture. Retry.');
    await waitFor(() => expect(stopButton.getAttribute('aria-disabled')).toBe('false'));
    await user.click(stopButton);
    await screen.findByText('Capture: Stopped');
    expect(f.client.startCapture).toHaveBeenCalledTimes(2);
    expect(f.client.stopCapture).toHaveBeenCalledTimes(2);
    expect(document.body.textContent).not.toContain('PRIVATE');
  });

  it('retains the permanent worker-failure remedy instead of offering futile capture retries', async () => {
    const f = setupFixture();
    f.client.queryCapture = vi.fn().mockResolvedValue({ state: 'Failed', message: 'Capture worker failed; restart Maxed', dropped_frames: 0, unknown_packets: 0, decode_failures: 0 });
    render(Setup, { client: f.client });
    await screen.findByText('Restart required');
    for (const name of ['Start Capture', 'Stop Capture']) expect(screen.getByRole('button', { name }).getAttribute('aria-disabled')).toBe('true');
  });

  it('cancels path edits without clearing saved assets and replaces a pending extraction without stale errors', async () => {
    const f = setupFixture();
    f.setSettings({ assets_path: 'C:\\Synthetic\\resources.assets' });
    const old = deferred<void>(), next = deferred<void>();
    f.client.setAssetsPath = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
    render(Setup, { client: f.client });
    const input = await screen.findByRole('textbox', { name: 'Assets Path' });
    await waitFor(() => expect((input as HTMLInputElement).value).toContain('Synthetic'));
    await fireEvent.input(input, { target: { value: 'C:\\Changed\\resources.assets' } });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Cancel' }));
    expect((input as HTMLInputElement).value).toBe('C:\\Synthetic\\resources.assets');
    expect(f.client.setAssetsPath).not.toHaveBeenCalled();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Extract' }));
    await fireEvent.input(input, { target: { value: 'C:\\Replacement\\resources.assets' } });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Replace' }));
    old.reject(new Error('PRIVATE cancelled generation'));
    next.resolve();
    await waitFor(() => expect(screen.getByRole('button', { name: 'Extract' })).toBeTruthy());
    expect(f.client.setAssetsPath).toHaveBeenLastCalledWith('C:\\Replacement\\resources.assets');
    expect(document.body.textContent).not.toContain('could not be extracted');
  });
});

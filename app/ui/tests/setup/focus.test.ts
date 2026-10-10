import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import Setup from '../../src/routes/Setup.svelte';
import Settings from '../../src/routes/Settings.svelte';
import Shell from '../../src/lib/shell/Shell.svelte';
import { fakeHistory } from '../navigation/fake-history';
import { setupFixture, deferred } from './fixture';

describe('initiating controls retain focus while pending and after status updates', () => {
  it.each([
    ['Check Access', 'startCapture'], ['Apply Adapter', 'setAdapter'],
    ['Start Capture', 'startCapture'], ['Stop Capture', 'stopCapture'],
  ] as const)('%s remains focusable and rejects repeat keyboard activation', async (name, command) => {
    const f = setupFixture();
    f.capture(command === 'stopCapture' ? 'Live' : 'Stopped');
    const pending = deferred<void>();
    const invoke = vi.fn(() => pending.promise);
    f.client[command] = invoke;
    render(Setup, { client: f.client });
    const button = screen.getByRole('button', { name }) as HTMLButtonElement;
    await waitFor(() => expect(button.getAttribute('aria-disabled')).toBe('false'));
    const user = userEvent.setup(); button.focus();
    await user.keyboard('{Enter}');
    await waitFor(() => expect(button.getAttribute('aria-disabled')).toBe('true'));
    expect(button.disabled).toBe(false);
    expect(document.activeElement).toBe(button);
    await user.keyboard('{Enter} ');
    expect(invoke).toHaveBeenCalledOnce();
    if (command !== 'setAdapter') f.capture(command === 'stopCapture' ? 'Stopped' : 'WaitingForGame');
    pending.resolve();
    await waitFor(() => expect((screen.getByRole('combobox', { name: 'Capture Adapter' }) as HTMLSelectElement).disabled).toBe(false));
    expect(document.activeElement).toBe(button);
  });

  it.each(['Apply', 'Demo Mode'] as const)('%s keeps focus and guards pending activation', async (name) => {
    const f = setupFixture(); const pending = deferred<void>();
    const command = name === 'Demo Mode' ? 'setDemo' : 'updateSettings';
    const invoke = vi.fn(() => pending.promise); f.client[command] = invoke;
    render(Settings, { client: f.client });
    const channel = await screen.findByRole('combobox', { name: 'Update Channel' });
    const user = userEvent.setup();
    if (name === 'Apply') await user.selectOptions(channel, 'beta');
    const button = screen.getByRole(name === 'Demo Mode' ? 'switch' : 'button', { name }) as HTMLButtonElement;
    await waitFor(() => expect(button.getAttribute('aria-disabled')).toBe('false'));
    button.focus(); await user.keyboard('{Enter}');
    await waitFor(() => expect(invoke).toHaveBeenCalledOnce());
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(button.disabled).toBe(false); expect(document.activeElement).toBe(button);
    await user.keyboard('{Enter} '); expect(invoke).toHaveBeenCalledOnce();
    pending.resolve();
    await waitFor(() => expect((channel as HTMLSelectElement).disabled).toBe(false));
    expect(document.activeElement).toBe(button);
  });

  it.each(['Start Capture', 'Stop Capture'] as const)('rail %s retains focus while capture changes', async (name) => {
    const f = setupFixture(); const pending = deferred<void>();
    f.capture(name === 'Stop Capture' ? 'Live' : 'Stopped');
    const invoke = vi.fn(() => pending.promise);
    if (name === 'Stop Capture') f.client.stopCapture = invoke;
    else f.client.startCapture = invoke;
    render(Shell, { client: f.client, historyAdapter: fakeHistory().adapter });
    const button = await screen.findByRole('button', { name }) as HTMLButtonElement;
    await waitFor(() => expect(button.getAttribute('aria-disabled')).toBe('false'));
    const user = userEvent.setup(); button.focus(); await user.keyboard('{Enter}');
    await waitFor(() => expect(button.getAttribute('aria-disabled')).toBe('true'));
    expect(button.disabled).toBe(false); expect(document.activeElement).toBe(button);
    await user.keyboard('{Enter} '); expect(invoke).toHaveBeenCalledOnce();
    f.capture(name === 'Stop Capture' ? 'Stopped' : 'WaitingForGame'); pending.resolve();
    await waitFor(() => expect(button.closest('[aria-busy]')?.getAttribute('aria-busy')).toBe('false'));
    expect(screen.getByRole('button', { name: name === 'Stop Capture' ? 'Start Capture' : 'Stop Capture' })).toBe(button);
    expect(document.activeElement).toBe(button);
  });

  it('expanded Settings Recovery keeps retry focus through a delayed settings read', async () => {
    const f = setupFixture();
    render(Settings, { client: f.client });
    await screen.findByRole('combobox', { name: 'Update Channel' });
    const user = userEvent.setup();
    const summary = screen.getByText('Recovery', { selector: 'summary' });
    expect(summary.closest('details')?.open).toBe(false);
    await user.click(summary);
    expect(summary.closest('details')?.open).toBe(true);
    const saved = await f.client.querySettings();
    const pending = deferred<typeof saved>();
    f.client.querySettings = vi.fn(() => pending.promise);
    const button = screen.getByRole('button', { name: 'Retry Settings' });
    button.focus(); await user.keyboard('{Enter}');
    await waitFor(() => expect(f.client.querySettings).toHaveBeenCalledOnce());
    expect(document.activeElement).toBe(button);
    pending.resolve({ ...saved, hardware_acceleration: true });
    await waitFor(() => expect((screen.getByRole('checkbox', { name: 'Hardware Acceleration' }) as HTMLInputElement).checked).toBe(true));
    expect(summary.closest('details')?.open).toBe(true);
    expect(document.activeElement).toBe(button);
  });
});

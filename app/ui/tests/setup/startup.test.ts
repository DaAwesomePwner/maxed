import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import App from '../../src/App.svelte';
import Shell from '../../src/lib/shell/Shell.svelte';
import { setupFixture, deferred } from './fixture';
import { fakeHistory } from '../navigation/fake-history';
import { flushHomeRender } from '../home/shell-fixture';

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  window.history.replaceState(null, '', '#/');
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  window.history.replaceState(null, '', '#/');
});

describe('startup setup routing', () => {
  it.each([1, 10])('routes legacy v%s to Setup without waiting for assets or the startup hint', async schemaVersion => {
    const f = setupFixture();
    f.transport.setHistoryUpgradeFixture({ schemaVersion, freeBytes: '1000000000' });
    f.client.queryStartupSetupRequired = vi.fn(() => new Promise<never>(() => {}));
    f.client.querySetup = vi.fn(() => new Promise<never>(() => {}));
    render(App, { client: f.client }); await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Setup', level: 1 })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'History upgrade required' })).toBeTruthy();
    expect(f.client.backupAndUpgradeHistory).not.toHaveBeenCalled();
  });
  it('preserves navigation made before a delayed legacy-upgrade reply', async () => {
    const f = setupFixture();
    f.client.queryStartupSetupRequired = vi.fn(async () => false);
    const pending = deferred<Awaited<ReturnType<typeof f.client.queryHistoryUpgradeStatus>>>();
    f.client.queryHistoryUpgradeStatus = vi.fn(() => pending.promise);
    render(App, { client: f.client }); await flushHomeRender();
    await fireEvent.click(within(screen.getByRole('complementary', { name: 'Character' })).getByRole('link', { name: 'Runs' }));
    await flushHomeRender();
    pending.resolve({ phase: 'blocked', schema_version: 10, required_bytes: '100', free_bytes: null, failure: 'space_unknown', last_failure: null });
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Runs', level: 1 })).toBeTruthy();
  });
  it('ignores retired-client upgrade routing replies', async () => {
    const a = setupFixture(), b = setupFixture();
    a.client.queryStartupSetupRequired = vi.fn(async () => false);
    b.client.queryStartupSetupRequired = vi.fn(async () => false);
    const pending = deferred<Awaited<ReturnType<typeof a.client.queryHistoryUpgradeStatus>>>();
    a.client.queryHistoryUpgradeStatus = vi.fn(() => pending.promise);
    const view = render(App, { client: a.client }); await flushHomeRender();
    await view.rerender({ client: b.client }); await flushHomeRender();
    pending.resolve({ phase: 'required', schema_version: 10, required_bytes: '100', free_bytes: '1000', failure: null, last_failure: null });
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy();
  });
  it.each([true, false])('opens the readiness-selected first route (required=%s)', async required => {
    const { client } = setupFixture();
    const pending = deferred<boolean>();
    client.queryStartupSetupRequired = vi.fn(() => pending.promise);
    render(App, { client });
    await flushHomeRender();
    expect(screen.getByRole('complementary', { name: 'Character' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy();
    pending.resolve(required);
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: required ? 'Setup' : 'Home', level: 1 })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Back' }).hasAttribute('disabled')).toBe(true);
    expect(client.queryStartupSetupRequired).toHaveBeenCalledTimes(1);
  });

  it('mounts configured Home while full setup checks are still pending', async () => {
    const { client } = setupFixture();
    client.queryStartupSetupRequired = vi.fn(async () => false);
    const checks = deferred<Awaited<ReturnType<typeof client.querySetup>>>();
    client.querySetup = vi.fn(() => checks.promise);
    render(App, { client });
    await flushHomeRender();
    expect(client.querySetup).toHaveBeenCalled();
    expect(screen.getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy();
    expect(screen.getByRole('complementary', { name: 'Character' })).toBeTruthy();
  });

  it('routes first run to interactive Setup while extraction reports progress', async () => {
    const { client } = setupFixture({ assets: 'extracting', extraction_stage: 'fingerprinting' });
    client.queryStartupSetupRequired = vi.fn(async () => true);
    render(App, { client });
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Setup', level: 1 })).toBeTruthy();
    expect(screen.getByRole('complementary', { name: 'Character' })).toBeTruthy();
    expect(screen.getByText(/Fingerprinting/i)).toBeTruthy();
  });

  it('never overwrites navigation made before the cheap startup reply', async () => {
    const { client } = setupFixture();
    const pending = deferred<boolean>();
    client.queryStartupSetupRequired = vi.fn(() => pending.promise);
    render(App, { client });
    await flushHomeRender();
    await fireEvent.click(within(screen.getByRole('complementary', { name: 'Character' })).getByRole('link', { name: 'Runs' }));
    await flushHomeRender();
    pending.resolve(true); await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Runs', level: 1 })).toBeTruthy();
  });

  it('opens repair Setup when startup checks reject', async () => {
    const { client } = setupFixture();
    client.queryStartupSetupRequired = vi.fn(async () => { throw new Error('unavailable'); });
    render(App, { client });
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Setup', level: 1 })).toBeTruthy();
  });

  it('replaces the initial entry and preserves subsequent Back and Forward', async () => {
    const { client } = setupFixture();
    const browser = fakeHistory();
    render(Shell, { client, historyAdapter: browser.adapter, initialSetupRequired: true });
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Setup', level: 1 })).toBeTruthy();
    expect(browser.length).toBe(1);
    await fireEvent.click(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Home' }));
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Setup', level: 1 })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Forward' }));
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy();
  });

  it('keeps a non-alert partial-mode notice until a catalog is available', async () => {
    const fixture = setupFixture();
    let available = false;
    fixture.client.queryCatalogAvailable = vi.fn(async () => available);
    render(Shell, { client: fixture.client, historyAdapter: fakeHistory().adapter });
    await flushHomeRender();
    fixture.capture('Live');
    await flushHomeRender();
    const notice = screen.getByRole('link', { name: 'Open Setup' });
    expect(notice.closest('[role="alert"], [aria-live]')).toBeNull();
    expect(screen.queryByText(/names, classes, bag ranks/)).toBeNull();
    await fireEvent.click(notice);
    await flushHomeRender();
    expect(screen.getByRole('heading', { name: 'Setup', level: 1 })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Open Setup' })).toBeTruthy();
    fixture.capture('Stopped');
    await flushHomeRender();
    expect(screen.queryByRole('link', { name: 'Open Setup' })).toBeNull();
    fixture.capture('WaitingForGame');
    await flushHomeRender();
    expect(screen.getByRole('link', { name: 'Open Setup' })).toBeTruthy();
    available = true;
    fixture.capture('Live');
    await flushHomeRender();
    expect(screen.queryByRole('link', { name: 'Open Setup' })).toBeNull();
    expect(fixture.client.queryCatalogAvailable).toHaveBeenCalled();
  });
});

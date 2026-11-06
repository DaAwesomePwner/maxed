import { setDebugMode } from '../../src/lib/debug/debug';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { productionFeatures } from '../../src/lib/navigation/routes';
import FixtureApp from './fixtures/FixtureApp.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';
import { fakeHistory } from '../navigation/fake-history';
import { homeSnapshot, homeSummary, saved } from '../home/fixtures';
import { advance, flush, mountRuns, ready } from '../runs/fixtures';

afterEach(() => setDebugMode(false));

describe('P4 strengthened P3 regressions (NON-RED)', () => {
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

  it('non_red_dungeon_zero_history_reads_and_one_shell_event_owner', async () => {
    const initial = homeSnapshot();
    const page = mountRuns({ hash: '#/', snapshot: initial }); await flush();
    expect(within(screen.getByRole('main')).getByRole('heading', { name: 'Party damage' })).toBeTruthy();
    const focus = within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Home' });
    focus.focus();
    for (let sequence = 13; sequence <= 20; sequence++) {
      const next = structuredClone(initial); next.fights[0].players[0].dps = sequence * 10;
      page.event({ fight_updated: next.fights[0] }, String(sequence));
      page.publish({ ...next, sequence: String(sequence) }); await advance(250);
    }
    expect(page.client.queryHome).toHaveBeenCalledTimes(0);
    for (const query of Object.values(page.queries)) expect(query).toHaveBeenCalledTimes(0);
    expect(within(screen.getByRole('main')).getAllByText('200', { exact: true }).length).toBeGreaterThan(0);
    expect(document.activeElement).toBe(focus);
    const rail = screen.getByRole('complementary', { name: 'Character' });
    page.setStore(saved('21')); page.publish({ ...initial, sequence: '21', area_kind: 'nexus', run: null }); await flush();
    await fireEvent.click(screen.getByRole('button', { name: 'View recap' })); await flush();
    expect(within(screen.getByRole('region', { name: 'Recap' })).getByRole('heading', { name: initial.run!.dungeon })).toBeTruthy();
    expect(document.activeElement).toBe(focus);
    await fireEvent.click(within(screen.getByRole('region', { name: 'Recap' })).getByRole('button', { name: 'Back' })); await flush();
    expect(screen.queryByRole('region', { name: 'Recap' })).toBeNull();
    expect(within(screen.getByRole('main')).getByRole('link', { name: /Synthetic Sanctum/ })).toBeTruthy();
    expect(page.client.queryHome).toHaveBeenCalledTimes(1);
    for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('complementary', { name: 'Character' })).toBe(rail);
    page.unmount(); await flush(); expect(page.browser.listeners).toBe(0);
  });

  it('non_red_home_fight_updated_does_not_invalidate_but_run_revision_does', async () => {
    const page = mountRuns({ hash: '#/' }); await flush();
    expect(page.client.queryHome).toHaveBeenCalledTimes(1);
    page.setStore(saved('13'));
    page.event({ fight_updated: homeSnapshot().fights[0] }, '13'); await advance(2000);
    expect(page.client.queryHome).toHaveBeenCalledTimes(1);
    expect(page.client.queryStore).toHaveBeenCalledTimes(1);
    expect(within(screen.getByRole('main')).getByRole('link', { name: /Synthetic Sanctum/ })).toBeTruthy();
    vi.mocked(page.client.queryHome).mockResolvedValue(ready(homeSummary({ recent_runs: [{ ...homeSnapshot().run!, dungeon: 'Accepted Home revision' }] })));
    page.event({ run_updated: homeSnapshot().run! }, '14'); page.setStore(saved('14')); await advance();
    expect(page.client.queryHome).toHaveBeenCalledTimes(2);
    expect(within(screen.getByRole('main')).getByRole('link', { name: /Accepted Home revision/ })).toBeTruthy();
  });

  it('non_red_retired_demo_entries_cannot_restore_route_or_filters', async () => {
    const snapshot = homeSnapshot({ stream_id: 'opaque-demo', area_kind: 'nexus', run: null });
    const page = mountRuns({ hash: '#/', snapshot, demo: true }); await flush();
    const primary = within(screen.getByRole('navigation', { name: 'Navigation' }));
    await fireEvent.click(primary.getByRole('link', { name: 'Runs' })); await flush();
    expect(page.browser.adapter.hash).toBe('#/runs');
    await fireEvent.click(primary.getByRole('link', { name: 'Home' })); await flush();
    const next = { ...snapshot, stream_id: 'replacement-live', sequence: '1' };
    page.setStore(saved('1', next.stream_id)); page.publish(next); await flush();
    page.browser.adapter.go(-1); await flush();
    expect(page.browser.adapter.hash).toBe('#/');
    expect(page.browser.adapter.state).toMatchObject({ focusedRowId: null, homeScope: null });
    expect(within(screen.getByRole('main')).getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy();
    expect(within(screen.getByRole('main')).getByRole('link', { name: /Synthetic Sanctum/ })).toBeTruthy();
    for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
  });
});

function setup(hash = '#/', lootBrowsing = false) {
  const transport = createFixtureTransport(fixture);
  const client = createClient(transport);
  const browser = fakeHistory(hash);
  const rendered = render(Shell, { client, historyAdapter: browser.adapter, features: { lootBrowsing, logs: false, stats: false, party: false, character: false, homeRedesign: productionFeatures.homeRedesign } });
  return { ...rendered, transport, client, browser };
}
describe('persistent shell', () => {
  it('keeps the rail and subscriptions mounted, focuses explicit routes and never focuses snapshot updates', async () => {
    const transport = createFixtureTransport(fixture); const client = createClient(transport);
    // Runs now waits for the accepted snapshot's durable checkpoint before reading.
    // Let the fixture's unsupported list handler reject so this tests terminal-error readiness.
    client.queryStore = async () => ({ state: 'ready', queued: 0, durable: fixture.sequence, stream_id: fixture.stream_id, message: '' });
    const onSnapshot = vi.spyOn(client, 'onSnapshot'); const onCapture = vi.spyOn(client, 'onCapture');
    render(Shell, { client, historyAdapter: fakeHistory().adapter });
    await screen.findByRole('heading', { name: 'Demo Knight' });
    const rail = screen.getByRole('complementary', { name: 'Character' });
    const primary = screen.getByRole('navigation', { name: 'Navigation' });
    await userEvent.setup().click(within(primary).getByRole('link', { name: 'Runs' }));
    await screen.findByRole('heading', { name: 'Run error' });
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Runs', level: 1 })));
    expect(screen.getByRole('complementary', { name: 'Character' })).toBe(rail);
    const home = within(primary).getByRole('link', { name: 'Home' }); home.focus();
    transport.publishSnapshot({ ...fixture, sequence: '13', map: 'Changed map' });
    await screen.findByText('Changed map'); expect(document.activeElement).toBe(home);
    expect(onSnapshot).toHaveBeenCalledTimes(1); expect(onCapture).toHaveBeenCalledTimes(1);
  });
  it('hides Loot when opted out, rejects its direct URL, and exposes it when included', async () => {
    const { unmount } = setup('#/loot');
    await screen.findByRole('heading', { name: 'Page Not Found', level: 1 });
    expect(within(screen.getByRole('navigation', { name: 'Navigation' })).queryByRole('link', { name: 'Loot' })).toBeNull();
    unmount(); setup('#/loot', true);
    await screen.findByRole('heading', { name: 'Loot', level: 1 });
    expect(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Loot' })).toBeTruthy();
  });
  it('keeps capture failures retryable with fixed text and storage failures separate', async () => {
    const transport = createFixtureTransport(fixture); const client = createClient(transport);
    client.queryCapture = async () => ({ ...fixture.capture, state: 'Failed', message: 'Capture could not continue. Check Npcap and your network adapter, then retry.' });
    client.queryStore = async () => ({ state: 'failed', queued: 1, durable: '9007199254740993', stream_id: 'demo-stream', message: 'SECRET store' });
    client.startCapture = vi.fn().mockRejectedValue(new Error('SECRET failure'));
    render(Shell, { client, historyAdapter: fakeHistory().adapter });
    await screen.findByText("Couldn't save. Retry.");
    const rail = screen.getByRole('complementary', { name: 'Character' });
    expect(within(rail).getByRole('heading', { name: fixture.character.name })).toBeTruthy();
    transport.publishSnapshot({ ...fixture, sequence: '13', map: 'Still capturing' });
    await within(rail).findByText('Still capturing');
    expect(within(rail).getByRole('heading', { name: fixture.character.name })).toBeTruthy();
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Retry Capture' }));
    await screen.findByRole('alert');
    expect(screen.getByRole('alert').textContent).toBe('Capture failed. Retry.');
    expect(screen.getByText("Couldn't save. Retry.")).toBeTruthy();
    expect(document.body.textContent).not.toContain('SECRET');
    expect(client.startCapture).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('button', { name: 'Reconnect' })).toBeNull();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry Capture' }));
    expect(client.startCapture).toHaveBeenCalledTimes(2);
    expect(screen.getByText("Couldn't save. Retry.")).toBeTruthy();
  });
  it('keeps capture usable when snapshot and store queries are unavailable', async () => {
    vi.useFakeTimers();
    const transport = createFixtureTransport(fixture); const client = createClient(transport);
    client.querySnapshot = vi.fn().mockRejectedValue(new Error('PRIVATE missing snapshot command'));
    client.queryStore = vi.fn().mockRejectedValue(new Error('PRIVATE missing store command'));
    client.startCapture = vi.fn(async () => { transport.publishCapture({ ...fixture.capture, state: 'WaitingForGame', message: 'Start the game and enter a realm. Reconnect if capture started during play.' }); });
    client.stopCapture = vi.fn(async () => { transport.publishCapture({ ...fixture.capture, message: 'Capture stopped.' }); });
    render(Shell, { client, historyAdapter: fakeHistory().adapter });
    await vi.advanceTimersByTimeAsync(2000);
    expect(screen.getByText('Disconnected. Reconnect.')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Reconnect' })).toBeTruthy();
    expect(screen.getByText('Storage Error')).toBeTruthy();
    const rail = screen.getByRole('complementary', { name: 'Character' });
    expect(within(rail).queryByRole('alert')).toBeNull();
    expect(within(rail).queryByRole('button', { name: 'Reconnect Capture' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Start Capture' }));
    expect(client.startCapture).toHaveBeenCalledTimes(1);
    await fireEvent.click(screen.getByRole('button', { name: 'Stop Capture' }));
    expect(client.stopCapture).toHaveBeenCalledTimes(1);
    expect(within(rail).queryByRole('alert')).toBeNull();
    expect(screen.getByText('Disconnected. Reconnect.')).toBeTruthy();
    expect(screen.getByText('Storage Error')).toBeTruthy();
    expect(document.body.textContent).not.toContain('PRIVATE');
  });
  it('exposes an ordered registration failure without issuing dependent queries', async () => {
    const client = createClient(createFixtureTransport(fixture));
    client.onCapture = vi.fn().mockRejectedValue(new Error('PRIVATE capture'));
    client.querySnapshot = vi.fn().mockRejectedValue(new Error('PRIVATE snapshot'));
    client.queryStore = vi.fn().mockRejectedValue(new Error('PRIVATE store'));
    render(Shell, { client, historyAdapter: fakeHistory().adapter });
    await screen.findByText('Disconnected. Reconnect.');
    expect(screen.getByRole('button', { name: 'Reconnect' })).toBeTruthy();
    expect(client.querySnapshot).not.toHaveBeenCalled();
    expect(client.queryStore).not.toHaveBeenCalled();
    expect(screen.getByText('Storage Error')).toBeTruthy();
    expect(document.body.textContent).not.toContain('PRIVATE');
  });
  it('shows a storage failure discovered by the slow poll after launch', async () => {
    vi.useFakeTimers();
    const client = createClient(createFixtureTransport(fixture));
    client.queryStore = vi.fn()
      .mockResolvedValueOnce({ state: 'ready', queued: 0, durable: '12', stream_id: 'demo-stream', message: '' })
      .mockResolvedValue({ state: 'failed', queued: 1, durable: '12', stream_id: 'demo-stream', message: '' });
    const { unmount } = render(Shell, { client, historyAdapter: fakeHistory().adapter });
    await vi.advanceTimersByTimeAsync(0);
    expect(screen.queryByText(/Saving failed/)).toBeNull();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(screen.getByText("Couldn't save. Retry.")).toBeTruthy();
    expect(within(screen.getByRole('complementary', { name: 'Character' })).queryByRole('alert')).toBeNull();
    unmount();
  });
  it('orders history before the single capture toggle and keeps skip-to-content', async () => {
    const { container } = setup('#/runs');
    await screen.findByRole('heading', { name: 'Runs', level: 1 });
    const controls = [...container.querySelectorAll<HTMLAnchorElement | HTMLButtonElement>('a, button')];
    const labels = controls.map(element => element.getAttribute('aria-label') ?? element.textContent?.trim());
    expect(labels.slice(0, 8)).toEqual(['Skip to Content', 'Back', 'Forward', 'Start Capture', 'Home', 'Runs', 'Alerts', 'Settings']);
    expect(screen.getByRole('main').querySelector('.route-toolbar')).toBeNull();
    const user = userEvent.setup(); await user.tab();
    expect(document.activeElement).toBe(screen.getByRole('link', { name: 'Skip to Content' }));
    await user.keyboard('{Enter}'); expect(document.activeElement).toBe(screen.getByRole('main'));
  });

  it('freezes the run timer after capture stops and uses an ended timestamp', async () => {
    setDebugMode(true);
    const { transport } = setup();
    await screen.findByRole('heading', { name: 'Demo Knight' });
    transport.publishCapture({ ...fixture.capture, state: 'Live' });
    transport.publishSnapshot({ ...fixture, sequence: '13', at: '7000000' });
    await screen.findByText('0:06');
    transport.publishCapture(fixture.capture);
    transport.publishSnapshot({ ...fixture, sequence: '14', at: '9000000' });
    await waitFor(() => expect(screen.getByText('0:06')).toBeTruthy());
    transport.publishSnapshot({ ...fixture, sequence: '15', at: '10000000', run: { ...fixture.run, ended: '8000000' } });
    await screen.findByText('0:07');
  });
  it('adapts Home without changing focus and retires demo navigation on a client/mode change', async () => {
    const liveFixture = { ...fixture, capture: { ...fixture.capture, state: 'Live' as const } };
    const transport = createFixtureTransport(liveFixture);
    const browser = fakeHistory();
    const { rerender } = render(Shell, { client: createClient(transport), preview: true, historyAdapter: browser.adapter });
    await within(screen.getByRole('main')).findByRole('heading', { name: 'Party damage' });
    const primary = screen.getByRole('navigation', { name: 'Navigation' });
    const home = within(primary).getByRole('link', { name: 'Home' }); home.focus();
    transport.publishSnapshot({ ...liveFixture, sequence: '13', area_kind: 'nexus', map: 'Nexus' });
    await fireEvent.click(await screen.findByRole('button', { name: 'View recap' }));
    const recap = await screen.findByRole('region', { name: 'Recap' });
    expect(within(recap).getByRole('heading', { name: liveFixture.run.dungeon })).toBeTruthy();
    expect(document.activeElement).toBe(home);
    await fireEvent.click(within(recap).getByRole('button', { name: 'Back' }));
    expect(screen.queryByRole('region', { name: 'Recap' })).toBeNull();
    await within(screen.getByRole('main')).findByRole('heading', { name: 'Nexus' });
    expect(document.activeElement).toBe(home);
    expect(browser.adapter.hash).toBe('#/');
    await userEvent.setup().click(within(primary).getByRole('link', { name: 'Runs' }));
    await screen.findByRole('heading', { name: 'Runs', level: 1 });
    await rerender({ client: createClient(createFixtureTransport({ ...fixture, stream_id: 'live-test' })), preview: false, historyAdapter: browser.adapter });
    await screen.findByRole('heading', { name: 'Home', level: 1 });
    expect((screen.getByRole('button', { name: 'Back' }) as HTMLButtonElement).disabled).toBe(true);
    browser.adapter.go(-1);
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy());
  });
});

describe('synthetic P4 consumption example', () => {
  it('restores filters, scroll and row focus after a story and Settings round trip', async () => {
    const browser = fakeHistory();
    render(FixtureApp, { historyAdapter: browser.adapter });
    const user = userEvent.setup();
    await user.click(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Runs' }));
    const input = await screen.findByRole('textbox', { name: 'Dungeon' });
    await user.type(input, 'Sanctum');
    const pane = screen.getByRole('main');
    Object.defineProperties(pane, { scrollHeight: { configurable: true, value: 1800 }, clientHeight: { configurable: true, value: 600 } });
    const row = screen.getByRole('link', { name: /Synthetic Sanctum/ });
    row.focus(); pane.scrollTop = 600; await fireEvent.scroll(pane);
    await user.click(row);
    await screen.findByRole('heading', { name: 'Run', level: 1 });
    expect(screen.getAllByRole('heading', { level: 2 }).map((heading) => heading.textContent)).toEqual(expect.arrayContaining(['Timeline', 'Fights', 'Loot']));
    await user.click(screen.getByRole('button', { name: 'Back' }));
    await waitFor(() => expect((screen.getByRole('textbox', { name: 'Dungeon' }) as HTMLInputElement).value).toBe('Sanctum'));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('link', { name: /Synthetic Sanctum/ })));
    expect(pane.scrollTop).toBe(600);
    await user.click(screen.getByRole('link', { name: 'Settings' }));
    await screen.findByRole('heading', { name: 'Settings', level: 1 });
    await user.click(screen.getByRole('button', { name: 'Back' }));
    await waitFor(() => expect((screen.getByRole('textbox', { name: 'Dungeon' }) as HTMLInputElement).value).toBe('Sanctum'));
    await waitFor(() => expect(pane.scrollTop).toBe(600));
  });
  it('focuses a timeline anchor and provides a removed-row fallback', async () => {
    const browser = fakeHistory('#/runs');
    const { rerender, container } = render(FixtureApp, { historyAdapter: browser.adapter });
    const announcement = container.querySelector<HTMLElement>('.restoration-status')!;
    expect(announcement).not.toBeNull();
    expect(announcement.getAttribute('role')).toBe('status');
    expect(announcement.textContent).toBe('');
    const user = userEvent.setup();
    await user.click(await screen.findByRole('link', { name: /Synthetic Sanctum/ }));
    await user.click(screen.getByRole('button', { name: 'Jump to observed fight' }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('region', { name: 'Observed fight' })));
    await user.click(screen.getByRole('button', { name: 'Back' }));
    await rerender({ historyAdapter: browser.adapter, runs: [] });
    await user.click(screen.getByRole('button', { name: 'Back' }));
    await screen.findByText('Page Heading');
    expect(container.querySelector('.restoration-status')).toBe(announcement);
    expect(announcement.textContent).toBe('Page Heading');
    expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Runs', level: 1 }));
    await user.click(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Home' }));
    await waitFor(() => expect(announcement.textContent).toBe(''));
    expect(container.querySelector('.restoration-status')).toBe(announcement);
  });
  it('cancels stale focus work during rapid navigation while a list is loading', async () => {
    const browser = fakeHistory();
    let ready!: () => void;
    const pending = new Promise<void>((resolve) => { ready = resolve; });
    render(FixtureApp, { historyAdapter: browser.adapter, rowsReady: () => browser.adapter.hash === '#/runs' ? pending : Promise.resolve() });
    const user = userEvent.setup();
    await user.click(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Runs' }));
    await user.click(screen.getByRole('link', { name: 'Settings' }));
    const heading = await screen.findByRole('heading', { name: 'Settings', level: 1 });
    await waitFor(() => expect(document.activeElement).toBe(heading));
    ready();
    await waitFor(() => expect(document.activeElement).toBe(heading));
    await user.click(screen.getByRole('button', { name: 'Back' }));
    await screen.findByRole('heading', { name: 'Runs', level: 1 });
    await user.click(screen.getByRole('button', { name: 'Forward' }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Settings', level: 1 })));
  });
});

describe('Shell-owned Character controls', () => {
  afterEach(cleanup);
  it('preserves defense through routes and back/forward with one live subscription; remount resets it', async () => {
    const client = createClient(createFixtureTransport(homeSnapshot()));
    const onSnapshot = vi.spyOn(client, 'onSnapshot'), onCapture = vi.spyOn(client, 'onCapture');
    const browser = fakeHistory('#/character');
    const props = { client, preview: true, features: { ...productionFeatures, character: true }, historyAdapter: browser.adapter };
    const page = render(Shell, props);
    const input = await screen.findByRole('spinbutton', { name: 'Enemy defense' });
    await fireEvent.input(input, { target: { value: '50' } });
    await fireEvent.click(screen.getByRole('link', { name: 'Settings' }));
    await screen.findByRole('heading', { name: 'Settings', level: 1 });
    browser.adapter.go(-1);
    expect((await screen.findByRole('spinbutton', { name: 'Enemy defense' }) as HTMLInputElement).value).toBe('50');
    browser.adapter.go(1);
    await screen.findByRole('heading', { name: 'Settings', level: 1 });
    browser.adapter.go(-1);
    expect((await screen.findByRole('spinbutton', { name: 'Enemy defense' }) as HTMLInputElement).value).toBe('50');
    expect(onSnapshot).toHaveBeenCalledTimes(1); expect(onCapture).toHaveBeenCalledTimes(1);
    page.unmount();
    render(Shell, props);
    expect((await screen.findByRole('spinbutton', { name: 'Enemy defense' }) as HTMLInputElement).value).toBe('0');
  });
  it('shows Character in production but cannot calculate without an input revision', async () => {
    const client = createClient(createFixtureTransport(homeSnapshot()));
    const calculate = vi.spyOn(client, 'getCharacterCalculation');
    render(Shell, { client, preview: true, historyAdapter: fakeHistory('#/character').adapter });
    await screen.findByRole('heading', { name: 'Character', level: 1 });
    expect(screen.getByRole('spinbutton', { name: 'Enemy defense' })).toBeTruthy();
    expect(await screen.findByText('Character calculation unavailable. Reconnect to obtain an input revision.')).toBeTruthy();
    expect(calculate).not.toHaveBeenCalled();
  });
});

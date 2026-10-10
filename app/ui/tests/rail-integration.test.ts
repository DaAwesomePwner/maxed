import { setDebugMode } from '../src/lib/debug/debug';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
afterEach(() => setDebugMode(false));
import Shell from '../src/lib/shell/Shell.svelte';
import Setup from '../src/routes/Setup.svelte';
import Settings from '../src/routes/Settings.svelte';
import SpriteImage from '../src/lib/components/SpriteImage.svelte';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { createSetupSession } from '../src/lib/setup/session';
import { spriteUrl } from '../src/lib/bridge/visuals';
import type { LiveSnapshot } from '../src/lib/bindings/LiveSnapshot';
import type { VisualPage } from '../src/lib/bindings/VisualPage';
import type { SpriteRef } from '../src/lib/bindings/SpriteRef';
import fixture from './fixtures/t0-live';
import { fakeHistory } from './navigation/fake-history';
import { setupFixture } from './setup/fixture';

const sprite: SpriteRef = { key: 'synthetic', sheet: 'characters.png', x: 0, y: 0, width: 8, height: 8 };
const flush = async () => { for (let i = 0; i < 40; i++) await Promise.resolve(); };

it('projects realm, dungeon, character swap, recovery and Nexus while the rail persists through routes', async () => {
  setDebugMode(true);
  const baseline: LiveSnapshot = { ...fixture, sequence: '0', character: null, run: null, map: null, fights: [], capture: { ...fixture.capture, state: 'WaitingForGame' } };
  const transport = createFixtureTransport(baseline); const client = createClient(transport);
  const snapshots = vi.spyOn(client, 'onSnapshot'); const events = vi.spyOn(client, 'onEngineEvent');
  const { unmount } = render(Shell, { client, historyAdapter: fakeHistory().adapter });
  const rail = await screen.findByRole('complementary', { name: 'Character' });
  await within(rail).findByText('Waiting for Game');
  expect(within(rail).getByRole('heading', { name: 'No Character' })).toBeTruthy();
  expect(within(rail).queryByText('Character not yet observed.')).toBeNull();
  expect(within(rail).queryByText('Live')).toBeNull();
  const hero = { ...fixture.character, identity_verified: true };
  const realm: LiveSnapshot = { ...baseline, sequence: '1', map: 'Realm', area_kind: 'realm', character: hero, capture: { ...fixture.capture, state: 'Live' } };
  transport.publishCapture(realm.capture); transport.publishSnapshot(realm);
  await within(rail).findByText('Realm');
  await fireEvent.click(within(rail).getByRole('link', { name: 'Runs' }));
  const dungeon: LiveSnapshot = { ...fixture, fights: fixture.fights.map(row => ({ ...row, target_kind: 'boss' })), character: hero, sequence: '2', capture: realm.capture };
  transport.publishSnapshot(dungeon);
  await within(rail).findByText(fixture.map);
  expect(within(rail).getByText('150')).toBeTruthy();
  const swapped: LiveSnapshot = { ...dungeon, sequence: '3', character: { ...hero, id: 'second', name: 'Next character', fame: null, maxed: null }, fights: [] };
  transport.publishSnapshot(swapped);
  await within(rail).findByRole('heading', { name: 'Next character' });
  expect(within(rail).queryByText('150')).toBeNull();
  await fireEvent.click(within(rail).getByRole('link', { name: 'Settings' }));
  transport.publishCapture({ ...fixture.capture, state: 'Recovering' });
  await within(rail).findByText('Recovering');
  transport.publishSnapshot({ ...swapped, sequence: '4', map: 'Nexus', area_kind: 'nexus', run: null, fights: [{ ...fixture.fights[0], ended: fixture.at, end_reason: 'map_changed' }] });
  await within(rail).findByText('Nexus');
  expect(within(rail).getByText('No Boss')).toBeTruthy();
  expect(within(rail).queryByText('150')).toBeNull();
  transport.publishCapture({ ...fixture.capture, state: 'Stopped' });
  await within(rail).findByText('Stopped');
  expect(screen.getByRole('complementary', { name: 'Character' })).toBe(rail);
  expect(snapshots).toHaveBeenCalledOnce(); expect(events).toHaveBeenCalledOnce();
  unmount();
});

it('refreshes the same object IDs on fingerprint change and degrades unavailable visuals without losing character data', async () => {
  const transport = createFixtureTransport(fixture); const client = createClient(transport);
  let page: VisualPage = { client_fingerprint: 'a'.repeat(64), objects: [{ object_type: fixture.character.class_id, name: 'Synthetic Knight', sprite, bag: null }] };
  client.getObjectVisuals = vi.fn(async () => page);
  const { container } = render(Shell, { client, historyAdapter: fakeHistory().adapter });
  await waitFor(() => expect(container.querySelector('img')?.src).toBe(spriteUrl(page.client_fingerprint, sprite)));
  page = { ...page, client_fingerprint: 'b'.repeat(64) };
  vi.spyOn(transport, 'invoke').mockResolvedValue(undefined);
  await client.setAssetsPath('synthetic');
  await waitFor(() => expect(container.querySelector('img')?.src).toBe(spriteUrl(page.client_fingerprint, sprite)));
  client.getObjectVisuals = vi.fn().mockRejectedValue(new Error('PRIVATE path'));
  await client.setAssetsPath('synthetic');
  await waitFor(() => expect(container.querySelector('img')).toBeNull());
  expect(screen.getByRole('heading', { name: fixture.character.name })).toBeTruthy();
  expect(screen.getByRole('img', { name: 'Class' })).toBeTruthy();
  expect(document.body.textContent).not.toContain('PRIVATE');
});

it.each([
  ['http://maxed-sprite.localhost', true],
  ['https://maxed-sprite.localhost', false],
  ['http://maxed-sprite.localhost:1234', false],
  ['http://maxed-sprite.localhost.evil.invalid', false],
  ['http://user@maxed-sprite.localhost', false],
  ['https://remote.invalid', false],
] as const)('restricts sprite origin %s', (origin, accepted) => {
  const { container } = render(SpriteImage, { sprite, sheetUrl: `${origin}/${'a'.repeat(64)}/characters.png/0-0-8-8.png`, alt: 'Synthetic sprite' });
  expect(container.querySelector('img') !== null).toBe(accepted);
});

it('exposes sticky delivery failure and disables capture retry that cannot repair the native publisher', async () => {
  const transport = createFixtureTransport(fixture); const client = createClient(transport);
  render(Shell, { client, historyAdapter: fakeHistory().adapter });
  await screen.findByRole('heading', { name: fixture.character.name });
  transport.publishCapture({ ...fixture.capture, state: 'Failed', message: 'Live updates interrupted; reconnect or restart Maxed' });
  await screen.findByText('Restart Maxed.');
  expect(screen.getByRole('button', { name: 'Start Capture' }).getAttribute('aria-disabled')).toBe('true');
  expect(screen.queryByRole('button', { name: 'Reconnect' })).toBeNull();
});

it.each([Setup, Settings])('passes explicit checks and detection retries while mount calls remain passive', async (component) => {
  const { client } = setupFixture({ assets: 'failed' });
  render(component, { client });
  await screen.findByRole('button', { name: 'Retry Detection' }); await flush();
  expect(vi.mocked(client.querySetup).mock.calls.every(args => args.length === 0)).toBe(true);
  vi.mocked(client.querySetup).mockClear();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry Checks' }));
  await waitFor(() => expect(client.querySetup).toHaveBeenCalledWith({ retry: true }));
  vi.mocked(client.querySetup).mockClear();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry Detection' }));
  await waitFor(() => expect(client.querySetup).toHaveBeenCalledWith({ retry: true }));
});

it('does not persist retry into pending-operation polls or completion refreshes', async () => {
  vi.useFakeTimers();
  const f = setupFixture({ assets: 'extracting' });
  const session = createSetupSession(f.client);
  await session.refreshSetup({ retry: true });
  expect(f.client.querySetup).toHaveBeenLastCalledWith({ retry: true });
  await vi.advanceTimersByTimeAsync(250);
  expect(f.client.querySetup).toHaveBeenLastCalledWith();
  f.setSetup({ assets: 'ready' });
  await session.action(async () => {}, 'Failed');
  expect(f.client.querySetup).toHaveBeenLastCalledWith();
  session.dispose();
});

it('shares the ordered capture and snapshot subscription with mounted Setup and Settings', async () => {
  const f = setupFixture();
  const snapshots = vi.spyOn(f.client, 'onSnapshot');
  const captures = vi.spyOn(f.client, 'onCapture');
  const events = vi.spyOn(f.client, 'onEngineEvent');
  render(Shell, { client: f.client, historyAdapter: fakeHistory('#/setup').adapter });
  await screen.findByRole('button', { name: 'Retry Checks' }); await flush();
  expect(snapshots).toHaveBeenCalledOnce(); expect(captures).toHaveBeenCalledOnce(); expect(events).toHaveBeenCalledOnce();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  await fireEvent.click(within(rail).getByRole('link', { name: 'Settings' }));
  await screen.findByRole('heading', { name: 'Settings', level: 1 }); await flush();
  expect(snapshots).toHaveBeenCalledOnce(); expect(captures).toHaveBeenCalledOnce(); expect(events).toHaveBeenCalledOnce();
  f.capture('Recovering');
  await within(rail).findByText('Recovering');
});

it('shows catch-up only on history routes after a checkpoint waits more than five seconds', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  let unmount = () => {};
  try {
    const pollIntervalMs = 1000, lagThresholdMs = 5000;
    let caughtUp = false;
    const transport = createFixtureTransport(fixture); const client = createClient(transport);
    client.getObjectVisuals = vi.fn().mockResolvedValue({ client_fingerprint: '', objects: [] });
    client.queryStore = vi.fn(async () => ({ state: 'ready' as const, stream_id: fixture.stream_id, queued: caughtUp ? 0 : 1, durable: caughtUp ? '13' : '12', message: '' }));
    unmount = render(Shell, { client, historyAdapter: fakeHistory('#/runs').adapter }).unmount;
    await vi.advanceTimersByTimeAsync(0); await flush();
    expect(client.queryStore).toHaveBeenCalledOnce();
    const rail = screen.getByRole('complementary', { name: 'Character' });

    // Positive control: the ordinary snapshot is rendered, but its newer
    // sequence cannot create a history checkpoint even with durability behind.
    transport.publishSnapshot({ ...fixture, sequence: '13', map: 'Snapshot-only location' });
    await vi.advanceTimersByTimeAsync(0); await flush();
    expect(within(rail).getByText('Snapshot-only location')).toBeTruthy();
    await vi.advanceTimersByTimeAsync(lagThresholdMs + pollIntervalMs); await flush();
    expect(screen.getByRole('heading', { name: 'Runs', level: 1 })).toBeTruthy();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Loading History');
    expect(screen.queryByText(/Changes not saved yet/)).toBeNull();
    // Initial join + one snapshot-expedited poll, then the healthy 10 s idle
    // interval: ordinary snapshots do not force one-second catch-up polling.
    expect(client.queryStore).toHaveBeenCalledTimes(1 + 1);

    const pollsBeforeCheckpoint = vi.mocked(client.queryStore).mock.calls.length;
    // A received discrete event may follow the snapshot that already covers it.
    // This is the next contiguous event, so lag comes from history invalidation,
    // not an artificial event gap/resync. Keep the store behind at sequence 12.
    transport.publishEngineEvent({ stream_id: fixture.stream_id, sequence: '13', at: fixture.at,
      event: { run_updated: { ...fixture.run, party_size: 2 } } });
    await vi.advanceTimersByTimeAsync(0); await flush();
    await vi.advanceTimersByTimeAsync(lagThresholdMs); await flush();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Loading History');
    expect(client.queryStore).toHaveBeenCalledTimes(pollsBeforeCheckpoint + lagThresholdMs / pollIntervalMs);

    await fireEvent.click(within(rail).getByRole('link', { name: 'Settings' }));
    await vi.advanceTimersByTimeAsync(1); await flush();
    expect(screen.getByRole('heading', { name: 'Settings', level: 1 })).toBeTruthy();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Loading History');
    await fireEvent.click(within(rail).getByRole('link', { name: 'Runs' }));
    await vi.advanceTimersByTimeAsync(0); await flush();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toContain('Loading History');

    caughtUp = true;
    await vi.advanceTimersByTimeAsync(pollIntervalMs); await flush();
    expect(client.queryStore).toHaveBeenCalledTimes(pollsBeforeCheckpoint + lagThresholdMs / pollIntervalMs + 1);
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Loading History');
    expect(within(rail).getByRole('heading', { name: fixture.character.name })).toBeTruthy();
  } finally {
    unmount();
    vi.useRealTimers();
    vi.restoreAllMocks();
  }
});

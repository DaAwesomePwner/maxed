import { readable } from 'svelte/store';
import { SHELL_CONNECTION } from '../../src/lib/shell/context';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { render, screen, waitFor, fireEvent, cleanup } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import Home from '../../src/routes/Home.svelte';
import AttributionNotice from '../../src/lib/home/AttributionNotice.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createHomeObservation } from '../../src/lib/home/mode';
import { homeSnapshot, homeSummary, saved } from './fixtures';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';

afterEach(cleanup);
it('wires privacy-safe account notices and releases the listener', async () => {
  const fixture = createFixtureTransport(homeSnapshot()), receive = vi.fn();
  const release = await createClient(fixture).onAccountHistoryChanged(receive);
  fixture.publishAccountHistoryChanged({ revision: 1, reason: 'account_switched' });
  expect(receive).toHaveBeenCalledWith({ revision: 1, reason: 'account_switched' });
  release();
  fixture.publishAccountHistoryChanged({ revision: 2, reason: 'legacy_recovered' });
  expect(receive).toHaveBeenCalledTimes(1);
});

it.each([true, false, undefined])('notice visibility follows %s', visible => {
  render(AttributionNotice, { visible });
  expect(screen.queryAllByRole('status')).toHaveLength(visible ? 1 : 0);
});
it('dismissal survives route remount within the session', async () => {
  const session = {};
  const page = render(AttributionNotice, { visible: true, session });
  await fireEvent.click(screen.getByRole('button', { name: 'Dismiss older history notice' }));
  page.unmount();
  render(AttributionNotice, { visible: true, session });
  expect(screen.queryByRole('status')).toBeNull();
});
it.each(['account_switched', 'legacy_recovered'] as const)('refetches Home reads for %s and rejects late A', async reason => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', map: 'Nexus', run: null });
  const fixture = createFixtureTransport(snapshot, undefined, homeSummary());
  const client = createClient(fixture);
  let finish!: (value: HistoryReply<HomeSummary>) => void;
  const query = vi.spyOn(client, 'queryHome').mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue({ status: 'ready', value: homeSummary({ unattributed_history: true }) });
  const bests = vi.spyOn(client, 'queryDungeonBests');
  const hud = vi.spyOn(client, 'getHomeHud');
  const loot = vi.spyOn(client, 'queryScopeLoot');
  render(Home, { snapshot, client, store: saved(), redesign: true });
  await waitFor(() => expect(query).toHaveBeenCalledTimes(1));
  fixture.publishAccountHistoryChanged({ revision: 1, reason });
  await waitFor(() => expect(query).toHaveBeenCalledTimes(2));
  finish({ status: 'superseded' });
  await waitFor(() => expect(screen.getByText(/Some older history/)).toBeTruthy());
  expect(bests.mock.calls.length).toBeGreaterThanOrEqual(2);
  expect(hud.mock.calls.length).toBeGreaterThanOrEqual(2);
  expect(loot).toHaveBeenCalled();
});

it('rail retains map-change recovery but clears a confirmed different account', () => {
  const observe = createHomeObservation({ pauseOnCaptureRecovery: true });
  const snapshot = homeSnapshot();
  const identity = (account_id: string | null, character_id: string | null) => ({
    stream_id: snapshot.stream_id, sequence: '1', at: snapshot.at,
    event: { local_identity_changed: { at: snapshot.at, account_id, character_id } },
  });
  observe(snapshot, snapshot.capture, null, 0);
  observe.onEvent(identity('synthetic-a', snapshot.character!.id));
  observe.onEvent(identity(null, null));
  observe.onEvent(identity(null, 'provisional'));
  const recovering = { ...snapshot.capture, state: 'Recovering' as const };
  expect(observe(snapshot, recovering, null, 1).retained?.character?.id).toBe(snapshot.character!.id);
  expect(observe.onEvent(identity('synthetic-b', 'synthetic-b-character'))).toBe(true);
  expect(observe(snapshot, recovering, null, 2).retained).toBeNull();
  const next = { ...snapshot, character: { ...snapshot.character!, id: 'synthetic-b-character' } };
  expect(observe(next, next.capture, null, 3).retained?.character?.id).toBe('synthetic-b-character');
  observe.historyChanged(); // later native revision must not hide B again
  expect(observe(next, next.capture, null, 4).retained?.character?.id).toBe('synthetic-b-character');
});

it.each(['account_switched', 'legacy_recovered'] as const)('invalidates dungeon streak, profile and boss baselines for %s', async reason => {
  const snapshot = homeSnapshot();
  const fixture = createFixtureTransport(snapshot);
  const client = createClient(fixture);
  const streak = vi.spyOn(client, 'getHomeDungeonStreak');
  const profile = vi.spyOn(client, 'getDungeonProfile');
  const bosses = vi.spyOn(client, 'getHomeBossBaselines');
  const window = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, runId: snapshot.run!.id, fights: snapshot.fights };
  render(Home, {
    props: { snapshot, client, store: saved(), redesign: true },
    context: new Map([[SHELL_CONNECTION, { homeWindow: readable(window) }]]),
  });
  await waitFor(() => {
    expect(streak).toHaveBeenCalled();
    expect(profile).toHaveBeenCalled();
    expect(bosses).toHaveBeenCalled();
  });
  streak.mockClear(); profile.mockClear(); bosses.mockClear();
  fixture.publishAccountHistoryChanged({ revision: 1, reason });
  await waitFor(() => {
    expect(streak).toHaveBeenCalled();
    expect(profile).toHaveBeenCalled();
    expect(bosses).toHaveBeenCalled();
  });
});

import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { AlertPreferences } from '../../src/lib/bindings/AlertPreferences';
import type { AlertView } from '../../src/lib/bindings/AlertView';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
import type { Route } from '../../src/lib/navigation/routes';
import type { AppSettings } from '../../src/lib/bindings/AppSettings';
import { invalidatesAlertHistory } from '../../src/lib/alerts/invalidation';
import { advance, alert, deferred, envelope, fight, flush, homeSnapshot, microtasks, mountPhase2, preferences, run, saved } from './fixtures';

// Boundary subscriber to the real Shell watermark. The integrated variant lives
// in alerts.test.ts with the real RouteFrame; this is not a second log controller.
const { frames } = vi.hoisted(() => ({ frames: [] as Array<{
  route: Route; required: string | null; historyReady: StoreStatus | null;
}> }));
vi.mock('../../src/routes/RouteFrame.svelte', () => ({
  default: (_anchor: unknown, props: typeof frames[number]) => { frames.push(props); },
}));

// Published controller contract. Test-time loading keeps module failures local
// to these cases rather than failing collection for every suite.
interface Moment { id: string; view: AlertView; committedAt: string; loudness: 'toast' | 'banner' }
interface PresentationState { current: Moment | null; queued: number; overflow: number }
interface Controller {
  accept(event: EventEnvelope): void; configure(value: AlertPreferences, streamId: string | null): void;
  setPaused(value: boolean): void; dismiss(id: string): void; dispose(): void;
}
async function controller() {
  const path = '../../src/lib/alerts/presentation.ts';
  const module: { createAlertController(receive: (state: PresentationState) => void,
    now: () => number, eventNow: () => string | null): Controller } = await import(/* @vite-ignore */ path);
  let state: PresentationState = { current: null, queued: 0, overflow: 0 };
  let sourceNow = '10000000';
  const history: PresentationState[] = [];
  const value = module.createAlertController(next => { state = next; history.push(next); }, () => Date.now(), () => sourceNow);
  value.configure(preferences(), 'live-phase2');
  return { value, history, get state() { return state; }, source(at: string) { sourceNow = at; } };
}

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(0); });
afterEach(() => { cleanup(); frames.length = 0; vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('phase2_alert_ordered_queue_freshness_and_bounds', async () => {
  const c = await controller();
  // Same synchronous batch must retain every accepted event, FIFO, without kind preemption.
  for (let i = 0; i < 500; i++) c.value.accept(envelope(`moment-${i}`, i + 1, i === 1 ? { kind: 'white_bag', details: null } : {}));
  expect(c.state.current?.id).toBe('moment-0');
  expect(c.state.queued).toBe(5); expect(c.state.overflow).toBe(494);
  c.value.dismiss('moment-0'); expect(c.state.current?.id).toBe('moment-1');
  for (let i = 1; i < 6; i++) c.value.dismiss(`moment-${i}`);
  expect(c.state.current).toBeNull(); expect(c.state.queued).toBe(0);
  // Recent IDs remain consumed; ID 0 has left the 256-ID cache after 500 accepts.
  c.value.accept(envelope('moment-499', 501)); expect(c.state.current).toBeNull();
  c.value.accept(envelope('moment-0', 502)); expect(c.state.current?.id).toBe('moment-0');
  c.value.dispose(); expect(vi.getTimerCount()).toBe(0);
});

it('late PB freshness uses committed time, retains happened time and expires at first display', async () => {
  const c = await controller(); c.source('90000000');
  c.value.accept(envelope('late-pb', 1, { at: '1000000' }, '89000000'));
  expect(c.state.current).toMatchObject({ id: 'late-pb', committedAt: '89000000', view: { at: '1000000' } });
  c.value.accept(envelope('queued', 2, {}, '89000000'));
  c.source('120000001'); c.value.dismiss('late-pb');
  expect(c.state.current).toBeNull();
  c.value.accept(envelope('stale', 3, { at: '120000000' }, '89000000'));
  expect(c.state.current).toBeNull(); c.value.dispose();
});

it('banner/toast dwell pauses and hidden backlog never replays', async () => {
  const c = await controller(); c.value.accept(envelope('banner', 1));
  await vi.advanceTimersByTimeAsync(7999); expect(c.state.current?.id).toBe('banner');
  c.value.setPaused(true); await vi.advanceTimersByTimeAsync(60000);
  expect(c.state.current?.id).toBe('banner');
  c.value.accept(envelope('hidden-backlog', 2)); c.source('80000000');
  c.value.setPaused(false); await vi.advanceTimersByTimeAsync(1);
  expect(c.state.current).toBeNull();
  c.value.accept(envelope('toast', 3, { presentation: 'toast' }, '80000000'));
  await vi.advanceTimersByTimeAsync(4999); expect(c.state.current?.id).toBe('toast');
  await vi.advanceTimersByTimeAsync(1); expect(c.state.current).toBeNull();
  c.value.dispose(); expect(vi.getTimerCount()).toBe(0);
});

it('phase2_alert_preferences_reconfigure_without_replay', async () => {
  const c = await controller();
  c.value.accept(envelope('current', 1)); c.value.accept(envelope('waiting', 2));
  c.value.configure(preferences({ personal_best: 'quiet_log_only' }), 'live-phase2');
  expect(c.state.current).toBeNull(); expect(c.state.queued).toBe(0);
  c.value.configure(preferences(), 'live-phase2');
  c.value.accept(envelope('current', 3)); c.value.accept(envelope('waiting', 4));
  expect(c.state.current).toBeNull();
  c.value.accept(envelope('legacy', 5, { presentation: null })); expect(c.state.current).toBeNull();
  const missing = alert({ id: 'omitted' }); Reflect.deleteProperty(missing, 'presentation');
  c.value.accept({ ...envelope('omitted', 6), event: { alert_fired: missing } }); expect(c.state.current).toBeNull();
  c.value.accept(envelope('toast', 7, { presentation: 'toast' })); expect(c.state.current?.loudness).toBe('toast');
  c.value.configure(preferences(), 'replacement'); expect(c.state.current).toBeNull();
  c.value.accept(envelope('retired-stream', 8)); expect(c.state.current).toBeNull();
  c.value.dispose(); expect(vi.getTimerCount()).toBe(0);
});

it('phase2_alert_host_focus_policy_and_namespace', async () => {
  const page = mountPhase2({ hash: '#/alerts' }); await flush();
  const liveRegions = [...document.querySelectorAll('[aria-live="polite"]')];
  page.event({ alert_fired: alert({ id: 'host-first' }) }, '13');
  page.event({ alert_fired: alert({ id: 'host-second', explanation: 'Second invented moment' }) }, '14');
  await flush();
  const dismiss = screen.getByRole('button', { name: /dismiss/i });
  expect(document.activeElement).not.toBe(dismiss);
  dismiss.focus(); await fireEvent.focusIn(dismiss); await advance(9000);
  expect(document.activeElement).toBe(dismiss);
  expect(screen.queryByText('Invented personal best')).toBeNull();
  expect(screen.getAllByText('Personal best').length).toBeGreaterThan(0);
  await fireEvent.click(dismiss); await flush();
  expect(document.activeElement?.textContent).toMatch(/Alerts/i);
  expect(screen.queryByText('Second invented moment')).toBeNull();
  expect(screen.getAllByText('Personal best').length).toBeGreaterThan(0);
  const announcements = [...document.querySelectorAll('[aria-live="polite"]')]
    .filter(node => node.textContent?.includes('Personal best'));
  expect(announcements).toHaveLength(1); expect(liveRegions).toContain(announcements[0]);
  expect(page.permission).not.toHaveBeenCalled(); expect(page.sound).not.toHaveBeenCalled();
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
  page.setStore(saved('1', 'replacement'));
  page.publish({ ...page.snapshot, stream_id: 'replacement', sequence: '1' }); await flush();
  expect(screen.queryByText('Second invented moment')).toBeNull();
  page.unmount(); await microtasks(); expect(vi.getTimerCount()).toBe(0);
});

const sourceEvents: EngineEvent[] = [
  { fight_ended: fight() }, { bag_dropped: homeSnapshot().drops[0] }, { drop_updated: homeSnapshot().drops[0] },
  { player_died: { at: '10000000', character_id: null, run_id: 'run-1' } },
  { character_merged: { provisional_id: 'old', verified_id: 'new' } },
  { character_updated: homeSnapshot().character! }, { run_updated: run() }, { run_ended: run() },
];
it.each(sourceEvents)('phase2_alert_quiet_source_revision_durable_invalidation: %j', async event => {
  const page = mountPhase2({ hash: '#/alerts' }); await flush();
  const inputs = frames.at(-1)!;
  expect(inputs.required).toBe('12');
  expect(invalidatesAlertHistory({ kind: 'alerts' }, { ...envelope('unused', 13), event })).toBe(true);
  expect(invalidatesAlertHistory({ kind: 'runs' }, { ...envelope('unused', 13), event })).toBe(false);
  page.event(event, '13'); page.event(event, '13'); await flush();
  expect(inputs.required).toBe('13'); expect(inputs.historyReady?.durable).toBe('12');
  page.setStore(saved('13', 'live-phase2')); await advance();
  expect(inputs.historyReady?.durable).toBe('13');
  const count = vi.mocked(page.client.queryStore).mock.calls.length;
  page.event(event, '13'); await advance(); expect(page.client.queryStore).toHaveBeenCalledTimes(count);
  expect(page.sound).not.toHaveBeenCalled();
});

it('heartbeat, fight update and numeric snapshot do not invalidate the alert checkpoint', async () => {
  const page = mountPhase2({ hash: '#/alerts' }); await flush();
  const inputs = frames.at(-1)!;
  page.event({ fight_updated: fight() }, '13');
  page.event({ session_updated: homeSnapshot().session! }, '14');
  page.publish({ ...page.snapshot, sequence: '15', at: '11000000' }); await advance();
  expect(inputs.required).toBe('12');
  page.event({ run_updated: run() }, '16'); await flush();
  expect(inputs.required).toBe('16');
});

it('unknown policy and late retired settings replies cannot authorize a visual effect', async () => {
  const old = deferred<AppSettings>();
  const page = mountPhase2({ hash: '#/alerts', configure: client => {
    const read = client.querySettings;
    client.querySettings = vi.fn().mockImplementationOnce(() => old.promise).mockImplementation(read);
  } }); await flush();
  expect(page.client.querySettings).toHaveBeenCalledTimes(1);
  page.event({ alert_fired: alert({ id: 'unknown-policy' }) }, '13'); await flush();
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  const previous = structuredClone(page.settings);
  page.setSettings({ alerts: preferences({ personal_best: 'off' }) });
  page.setStore(saved('1', 'replacement')); page.publish({ ...page.snapshot, stream_id: 'replacement', sequence: '1' }); await flush();
  expect(page.client.querySettings).toHaveBeenCalledTimes(2);
  old.resolve(previous); await flush();
  page.event({ alert_fired: alert({ id: 'new-stream-off' }) }, '2'); await flush();
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  expect(page.permission).not.toHaveBeenCalled(); expect(page.sound).not.toHaveBeenCalled();
});

it('failed policy refresh remains silent instead of guessing banner defaults', async () => {
  const page = mountPhase2({ hash: '#/alerts', configure: client => {
    client.querySettings = vi.fn().mockRejectedValue(new Error('Synthetic policy unavailable'));
  } }); await flush();
  page.event({ alert_fired: alert() }, '13'); await advance(10000);
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  expect(page.permission).not.toHaveBeenCalled(); expect(page.sound).not.toHaveBeenCalled();
});

it('key-pop toast names only its dungeon', async () => {
  const c = await controller();
  c.value.configure({ ...preferences(), key_popped: 'toast' }, 'live-phase2');
  c.value.accept(envelope('key-pop', 1, {
    kind: 'key_popped', details: { type: 'key_popped', dungeon: 'Lost Halls' },
    explanation: 'Lost Halls key popped', presentation: 'toast',
  }));
  expect(c.state.current).toMatchObject({ title: 'Lost Halls key popped', context: '' });
  c.value.dispose();
});


it.each([{ selected: null }, { selected: ['Lost Halls'] }, { selected: ['The Void'] }, { selected: [] }])('key-pop reconfiguration filters visible and queued banners: %j', async ({ selected }) => {
  const c = await controller();
  c.value.configure(preferences({ key_popped: 'banner' }), 'live-phase2');
  const key = { kind: 'key_popped' as const, details: { type: 'key_popped' as const, dungeon: 'Lost Halls' }, presentation: 'banner' as const };
  c.value.accept(envelope('visible-key', 1, key));
  c.value.accept(envelope('waiting-key', 2, key));
  c.value.accept(envelope('waiting-pb', 3));
  expect(c.state.current?.id).toBe('visible-key'); expect(c.state.queued).toBe(2);
  c.value.configure(preferences({ key_popped: 'banner', key_pop_dungeons: selected }), 'live-phase2');
  if (selected === null || selected.includes('Lost Halls')) {
    expect(c.state.current?.id).toBe('visible-key'); expect(c.state.queued).toBe(2);
    c.value.dismiss('visible-key'); expect(c.state.current?.id).toBe('waiting-key');
    c.value.dismiss('waiting-key');
  } else {
    expect(c.state.queued).toBe(0);
    c.value.accept(envelope('filtered-arrival', 4, key));
    expect(c.state.queued).toBe(0);
  }
  expect(c.state.current?.id).toBe('waiting-pb');
  c.value.dismiss('waiting-pb');
  c.value.configure(preferences({ key_popped: 'banner' }), 'live-phase2');
  c.value.accept(envelope('visible-key', 5, key)); c.value.accept(envelope('waiting-key', 6, key));
  expect(c.state.current).toBeNull();
  c.value.dispose(); expect(vi.getTimerCount()).toBe(0);
});

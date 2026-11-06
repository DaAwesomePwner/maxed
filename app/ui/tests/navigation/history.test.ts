import { describe, expect, it, vi } from 'vitest';
import { createRouter, installHistoryPersistence, installNavigationInput } from '../../src/lib/navigation/history';
import { breadcrumbs, parseRoute, routeHref, type Route } from '../../src/lib/navigation/routes';
import { defaultFilters, defaultLogsPosition, emptyListState, normalizeListState, type AlertsPosition } from '../../src/lib/navigation/state';
import { fakeHistory } from './fake-history';
import { advance, flush, mountRuns, saved } from '../runs/fixtures';
import { homeSnapshot } from '../home/fixtures';

const alertPosition = (): AlertsPosition => ({
  filter: { kind: 'white_bag', range: { start: '0', end: '9223372036854775807' }, query: '  literal %_\\  ', cursor: { at: '12', id: 'alert-a' }, limit: 50 },
  previous: [null, { at: '20', id: 'alert-b' }], window: 2,
});

it('keeps whisper filters between partners but starts Chat-to-whisper seeds clean', () => {
  const browser = fakeHistory('#/logs/chat/whisper');
  const router = createRouter(browser.adapter);
  const position = defaultLogsPosition();
  position.whisper = { query: 'ready', showHidden: true, partner: 'Vexlin' };
  router.save({ logsPosition: position });
  router.navigate({ kind: 'logs', view: 'chat', channel: 'whisper', partner: 'Quenby' });
  expect(router.state.entry.logsPosition?.whisper).toEqual({ query: 'ready', showHidden: true, partner: 'Quenby' });
  router.navigate({ kind: 'logs', view: 'chat', channel: 'whisper', partner: 'Morrow' });
  expect(router.state.entry.logsPosition?.whisper).toEqual({ query: 'ready', showHidden: true, partner: 'Morrow' });
  router.back();
  expect(router.state.entry.logsPosition?.whisper).toEqual({ query: 'ready', showHidden: true, partner: 'Quenby' });
  router.forward();
  expect(router.state.entry.logsPosition?.whisper).toEqual({ query: 'ready', showHidden: true, partner: 'Morrow' });
  router.navigate({ kind: 'logs', view: 'chat', channel: 'all' });
  router.navigate({ kind: 'logs', view: 'chat', channel: 'whisper', partner: 'Vexlin' });
  expect(router.state.entry.logsPosition?.whisper).toEqual({ query: '', showHidden: false, partner: 'Vexlin' });
  router.dispose();
});

describe('additive saved positions', () => {
  it('defaults legacy and malformed entries without changing the Runs query limit', () => {
    for (const value of [undefined, null, false, [], {}, { alertsPosition: [], fightPosition: [] }]) {
      expect(normalizeListState(value)).toEqual(emptyListState());
    }
    const legacy = { filters: { ...defaultFilters(), limit: 100 }, focusedRowId: 'run-a' };
    expect(normalizeListState(legacy)).toMatchObject({ filters: { limit: 25 }, focusedRowId: 'run-a', alertsPosition: null, runsWindow: 0, fightPosition: null });
  });

  it('keeps JSON-only alert query fields, normalized literal search and the latest 100 cursors', () => {
    const position = alertPosition();
    position.previous = [null, ...Array.from({ length: 101 }, (_, index) => ({ at: String(index), id: `alert-${index}` }))];
    const state = normalizeListState({ alertsPosition: { ...position, pages: [{ rows: ['not saved'] }], filter: { ...position.filter, limit: 100, extra: true } }, runsWindow: 3,
      fightPosition: { playerWindow: 2, sourceWindow: 4, selectedPlayerId: 'player-a', extra: true } });
    expect(state.alertsPosition).toEqual({ filter: { ...position.filter, limit: 50, query: 'literal %_\\' }, previous: position.previous.slice(-100), window: 2 });
    expect(state.fightPosition).toEqual({ playerWindow: 2, sourceWindow: 4, selectedPlayerId: 'player-a', plotted: null });
    expect(JSON.parse(JSON.stringify(state))).toEqual(state);
    expect(state.runsWindow).toBe(3);
    expect(position.filter.query).toBe('  literal %_\\  ');
  });

  it('rejects unsafe alert query, timestamp and cursor bounds without leaking partial filters', () => {
    const filter = alertPosition().filter;
    const badFilters = [
      { kind: 'unknown' }, { range: [] }, { range: { start: '2', end: '2' } },
      { range: { start: '-1', end: '2' } }, { range: { start: '0', end: '9223372036854775808' } },
      { query: 'x'.repeat(129) }, { query: '\u{1f600}'.repeat(129) }, { query: 'has\u0000nul' },
      { cursor: { at: '01', id: 'a' } }, { cursor: { at: '1', id: '\u00e9'.repeat(129) } },
      { cursor: { at: '1', id: 'bad\n' } }, { limit: 0 }, { limit: 101 }, { limit: 1.5 },
    ];
    for (const patch of badFilters) {
      expect(normalizeListState({ alertsPosition: { ...alertPosition(), filter: { ...filter, ...patch } } }).alertsPosition).toBeNull();
    }
    expect(normalizeListState({ alertsPosition: { ...alertPosition(), previous: [null, {}] } }).alertsPosition).toBeNull();
    const boundary = normalizeListState({ alertsPosition: { ...alertPosition(), filter: { ...filter, query: '\u{1f600}'.repeat(128) } } });
    expect(boundary.alertsPosition?.filter.query).toBe('\u{1f600}'.repeat(128));
  });

  it('normalizes each window independently and preserves known zero', () => {
    for (const invalid of [-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, '2', null]) {
      const state = normalizeListState({ runsWindow: invalid, alertsPosition: { ...alertPosition(), window: invalid },
        fightPosition: { playerWindow: invalid, sourceWindow: 0, selectedPlayerId: '\u0000' } });
      expect(state.runsWindow).toBe(0);
      expect(state.alertsPosition?.window).toBe(0);
      expect(state.fightPosition).toEqual({ playerWindow: 0, sourceWindow: 0, selectedPlayerId: null, plotted: null });
    }
  });

  it('restores positions per entry and retires them with their namespace', () => {
    const browser = fakeHistory(); const router = createRouter(browser.adapter, undefined, 'demo:a');
    router.navigate({ kind: 'alerts' }); router.save({ alertsPosition: alertPosition(), focusedRowId: 'alert-a' });
    router.navigate({ kind: 'runs' });
    expect(router.state.entry.alertsPosition).toBeNull();
    router.save({ runsWindow: 4 });
    router.navigate({ kind: 'fight', id: 'f' });
    expect(router.state.entry.runsWindow).toBe(0);
    router.save({ fightPosition: { playerWindow: 1, selectedPlayerId: 'p', sourceWindow: 2 } });
    router.back(); expect(router.state.entry.runsWindow).toBe(4);
    router.back(); expect(router.state.entry.alertsPosition?.window).toBe(2);
    expect(router.state.entry.fightPosition).toBeNull();
    router.forward(); router.forward();
    expect(router.state.entry.fightPosition?.selectedPlayerId).toBe('p');
    router.setScope('live:b'); browser.adapter.go(-1);
    expect(router.state.entry).toMatchObject({ alertsPosition: null, runsWindow: 0, fightPosition: null });
    router.dispose();
  });
});

describe('browser-authoritative history', () => {
  it('reads the current route across navigation, initial replacement, saves and traversal', () => {
    const browser = fakeHistory(); const router = createRouter(browser.adapter);
    const matches = (route: Route) => {
      expect(router.route).toEqual(route);
      expect(router.route).toEqual(router.state.entry.route);
    };
    matches({ kind: 'home' });
    router.replaceInitial({ kind: 'runs' }); matches({ kind: 'runs' });
    router.navigate({ kind: 'run', id: 'a' }); matches({ kind: 'run', id: 'a' });
    const route = router.route;
    router.save({ scrollTop: 42 }); matches({ kind: 'run', id: 'a' });
    expect(router.route).toBe(route);
    router.back(); matches({ kind: 'runs' });
    router.forward(); matches({ kind: 'run', id: 'a' });
    browser.adapter.go(-1); matches({ kind: 'runs' });
    const state = router.state; state.entry.route = { kind: 'settings' };
    matches({ kind: 'runs' });
    router.dispose();
  });
  it('does not serialize the history entry when a routed Shell accepts a live event', async () => {
    vi.useFakeTimers();
    const page = mountRuns({ hash: '#/runs' }); await flush();
    const stringify = vi.spyOn(JSON, 'stringify');
    try {
      // Fight updates on Runs visit all five predicates without invalidating history.
      page.event({ fight_updated: homeSnapshot().fights[0] }, '13');
      await flush();
      expect(page.queries.listRuns).toHaveBeenCalledTimes(1);
      // Positive control: the same Shell accepts and refreshes a durable run revision.
      page.event({ run_updated: homeSnapshot().run! }, '14'); await flush();
      expect(stringify.mock.calls.filter(([value]) => value && typeof value === 'object' && 'owner' in value && 'route' in value)).toEqual([]);
      // A refreshed page legitimately saves its position after the writer catches up.
      page.setStore(saved('14')); await advance();
      expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
    } finally { stringify.mockRestore(); page.unmount(); }
  });
  it('keeps frame-rate saves in memory and writes once after a 250 ms quiet period', () => {
    vi.useFakeTimers();
    const browser = fakeHistory(); const replace = vi.spyOn(browser.adapter, 'replace');
    const router = createRouter(browser.adapter); router.navigate({ kind: 'runs' }); replace.mockClear();
    for (let i = 1; i <= 600; i++) {
      router.save({ scrollTop: i, focusedRowId: 'row-a' });
      vi.advanceTimersByTime(16);
    }
    expect(router.state.entry.scrollTop).toBe(600);
    expect(replace).not.toHaveBeenCalled();
    vi.advanceTimersByTime(233); expect(replace).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1); expect(replace).toHaveBeenCalledTimes(1);
    expect(browser.adapter.state).toMatchObject({ scrollTop: 600, focusedRowId: 'row-a' });
    router.dispose();
    vi.advanceTimersByTime(1000); expect(replace).toHaveBeenCalledTimes(1);
  });
  it('flushes before push/back/forward and restores fresh cache after native traversal before the debounce', () => {
    vi.useFakeTimers();
    const browser = fakeHistory(); const router = createRouter(browser.adapter);
    const push = vi.spyOn(browser.adapter, 'push'); const replace = vi.spyOn(browser.adapter, 'replace');
    const go = vi.spyOn(browser.adapter, 'go');
    router.navigate({ kind: 'runs' }); router.save({ scrollTop: 600 });
    router.navigate({ kind: 'run', id: 'a' });
    expect(replace.mock.lastCall?.[0]).toMatchObject({ scrollTop: 600 });
    expect(replace.mock.invocationCallOrder.at(-1)!).toBeLessThan(push.mock.invocationCallOrder.at(-1)!);
    router.save({ scrollTop: 90 }); router.back();
    const outgoingBack = replace.mock.calls.findIndex(([state]) => (state as { scrollTop: number }).scrollTop === 90);
    expect(outgoingBack).toBeGreaterThanOrEqual(0);
    expect(replace.mock.invocationCallOrder[outgoingBack]).toBeLessThan(go.mock.invocationCallOrder.at(-1)!);
    router.save({ scrollTop: 700 }); router.forward();
    const outgoingForward = replace.mock.calls.findIndex(([state]) => (state as { scrollTop: number }).scrollTop === 700);
    expect(outgoingForward).toBeGreaterThanOrEqual(0);
    expect(replace.mock.invocationCallOrder[outgoingForward]).toBeLessThan(go.mock.invocationCallOrder.at(-1)!);
    router.save({ scrollTop: 123 });
    browser.adapter.go(-1); // Native mouse traversal has no pre-departure hook.
    expect(router.state.entry.scrollTop).toBe(700);
    vi.advanceTimersByTime(300);
    expect(browser.adapter.state).toMatchObject({ scrollTop: 700 });
    browser.adapter.go(1);
    expect(router.state.entry.scrollTop).toBe(123);
    router.dispose();
  });
  it('flushes dirty state on visibilitychange/pagehide and removes lifecycle handlers', () => {
    vi.useFakeTimers();
    const browser = fakeHistory(); const router = createRouter(browser.adapter);
    const release = installHistoryPersistence(window, router);
    router.save({ scrollTop: 40 }); document.dispatchEvent(new Event('visibilitychange'));
    expect(browser.adapter.state).toMatchObject({ scrollTop: 40 });
    router.save({ scrollTop: 80 }); window.dispatchEvent(new Event('pagehide'));
    expect(browser.adapter.state).toMatchObject({ scrollTop: 80 });
    release(); router.save({ scrollTop: 120 }); window.dispatchEvent(new Event('pagehide'));
    expect(browser.adapter.state).toMatchObject({ scrollTop: 80 });
    router.dispose();
  });
  it('traverses Home → Runs → Run → Fight, then truncates the forward branch', () => {
    const browser = fakeHistory();
    const router = createRouter(browser.adapter);
    router.back(); expect(browser.length).toBe(1);
    router.navigate({ kind: 'runs' });
    router.navigate({ kind: 'run', id: 'a' });
    router.navigate({ kind: 'fight', id: 'b', runId: 'a' });
    router.back(); router.back(); expect(router.state.entry.route.kind).toBe('runs');
    router.forward(); expect(router.state.entry.route).toEqual({ kind: 'run', id: 'a' });
    router.navigate({ kind: 'settings' });
    expect(router.state.canForward).toBe(false);
    router.forward(); expect(router.state.entry.route.kind).toBe('settings');
    router.dispose(); expect(browser.listeners).toBe(0);
  });
  it('restores two independent filter/scroll/focus records and survives cache eviction', () => {
    const browser = fakeHistory();
    const router = createRouter(browser.adapter);
    router.navigate({ kind: 'runs' });
    const first = { ...defaultFilters(), dungeon: 'First', range: { start: '100', end: '200' } };
    router.save({ filters: first, scrollTop: 600, focusedRowId: 'row-a' });
    router.navigate({ kind: 'run', id: 'row-a' });
    router.navigate({ kind: 'runs' });
    router.save({ filters: { ...defaultFilters(), dungeon: 'Second', sort: 'oldest' }, scrollTop: 90, focusedRowId: 'row-b' });
    router.back(); router.back();
    expect(router.state.entry).toMatchObject({ filters: first, scrollTop: 600, focusedRowId: 'row-a' });
    router.forward(); router.forward();
    expect(router.state.entry).toMatchObject({ filters: { dungeon: 'Second', sort: 'oldest' }, scrollTop: 90, focusedRowId: 'row-b' });
    for (let i = 0; i < 60; i++) router.navigate({ kind: 'settings' });
    expect(router.cacheSize).toBe(50);
    for (let i = 0; i < 62; i++) router.back();
    expect(router.state.entry).toMatchObject({ filters: first, scrollTop: 600, focusedRowId: 'row-a' });
    router.dispose();
  });
  it('retires demo/stream state and does not recover it through native history', () => {
    const browser = fakeHistory();
    const router = createRouter(browser.adapter, undefined, 'demo:a');
    router.navigate({ kind: 'runs' });
    router.save({ filters: { ...defaultFilters(), dungeon: 'Demo only' }, scrollTop: 600 });
    router.navigate({ kind: 'run', id: 'demo' });
    router.setScope('live:b');
    expect(router.state).toMatchObject({ canBack: false, canForward: false, explicit: false });
    browser.adapter.go(-1);
    expect(router.state.entry.route.kind).toBe('home');
    expect(router.state.entry.filters.dungeon).toBeNull();
    router.dispose();
  });
  it('binds the first stream without resetting navigation made during its initial query', () => {
    const browser = fakeHistory(); const router = createRouter(browser.adapter);
    router.navigate({ kind: 'runs' });
    router.bindScope('live:first-stream');
    expect(router.state.entry.scope).toBe('live:first-stream');
    expect(router.state.entry.route.kind).toBe('runs');
    router.back(); expect(router.state.entry.route.kind).toBe('home');
    expect(router.state.entry.scope).toBe('live:first-stream');
    router.dispose();
  });
  it('breadcrumbs follow ancestry independent of the visit order', () => {
    const browser = fakeHistory('#/settings');
    const router = createRouter(browser.adapter);
    router.navigate({ kind: 'player', runId: 'r', fightId: 'f', objectId: 1 });
    const crumbs = breadcrumbs(router.state.entry.route);
    expect(crumbs.map((crumb) => crumb.label)).toEqual(['Home', 'Runs', 'Run', 'Fight', 'Player']);
    router.navigate(parseRoute(crumbs[2].href!));
    expect(router.state.entry.route).toEqual({ kind: 'run', id: 'r' });
    router.dispose();
  });
});

describe('safe addresses and feature boundaries', () => {
  it.each(['#/runs/%', '#/runs/%00', '#/runs/', '#/loot/65536', '#/fights/f/players/-0', '#/fights/f/players/NaN', '#/nope'])('recovers invalid address %s', (hash) => {
    expect(parseRoute(hash, { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: false }).kind).toBe('not-found');
  });
  it('round trips encoded IDs and gates direct Loot/item URLs', () => {
    const route: Route = { kind: 'fight', runId: 'run/# %', id: 'fight/日本語' };
    expect(parseRoute(routeHref(route))).toEqual(route);
    expect(parseRoute('#/loot', { lootBrowsing: false, logs: false, stats: false, party: false, character: false, homeRedesign: false }).kind).toBe('not-found');
    expect(parseRoute('#/loot/12', { lootBrowsing: false, logs: false, stats: false, party: false, character: false, homeRedesign: false }).kind).toBe('not-found');
    expect(parseRoute('#/loot/12', { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: false })).toEqual({ kind: 'item', type: 12 });
  });
});

describe('input ownership', () => {
  it('handles Alt arrows once and preserves editing, composition and modified events', () => {
    const router = { back: vi.fn(), forward: vi.fn() };
    const dispose = installNavigationInput(window, router);
    const send = (options: KeyboardEventInit) => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, cancelable: true, ...options }));
    expect(send({})).toBe(false);
    send({ key: 'ArrowRight' });
    for (const options of [{ ctrlKey: true }, { metaKey: true }, { shiftKey: true }, { isComposing: true }, { repeat: true }, { altKey: false }]) send(options);
    const prevented = new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, cancelable: true });
    prevented.preventDefault(); window.dispatchEvent(prevented);
    const input = document.createElement('input'); document.body.append(input);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', altKey: true, bubbles: true })); input.remove();
    expect(router.back).toHaveBeenCalledTimes(1); expect(router.forward).toHaveBeenCalledTimes(1);
    dispose(); send({}); expect(router.back).toHaveBeenCalledTimes(1);
  });
  it('leaves mouse buttons to native history by default, with one opt-in fallback owner', () => {
    const router = { back: vi.fn(), forward: vi.fn() };
    const native = installNavigationInput(window, router);
    window.dispatchEvent(new MouseEvent('mouseup', { button: 3, cancelable: true }));
    expect(router.back).not.toHaveBeenCalled(); native();
    const fallback = installNavigationInput(window, router, 'fallback');
    expect(window.dispatchEvent(new MouseEvent('mouseup', { button: 3, cancelable: true }))).toBe(false);
    window.dispatchEvent(new MouseEvent('mouseup', { button: 4, cancelable: true }));
    expect(router.back).toHaveBeenCalledTimes(1); expect(router.forward).toHaveBeenCalledTimes(1);
    fallback();
  });
});

it('normalizes chart selections while distinguishing defaults from an empty selection', () => {
  const position = { playerWindow: 0, sourceWindow: 0, selectedPlayerId: null };
  expect(normalizeListState({ fightPosition: position }).fightPosition?.plotted).toBeNull();
  expect(normalizeListState({ fightPosition: { ...position, plotted: [] } }).fightPosition?.plotted).toEqual([]);
  expect(normalizeListState({ fightPosition: { ...position, plotted: ['party', 'self', 'self'] } }).fightPosition?.plotted).toEqual(['party', 'self']);
  for (const plotted of ['party', [null], ['bad\n'], ['x'.repeat(257)]]) {
    expect(normalizeListState({ fightPosition: { ...position, plotted } }).fightPosition?.plotted).toBeNull();
  }
});

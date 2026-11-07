import { cleanup, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { PlayerContextView } from '../../src/lib/bindings/PlayerContextView';
import type { PlayerDamage } from '../../src/lib/bindings/PlayerDamage';
import type { RunFightStripRow } from '../../src/lib/bindings/RunFightStripRow';
import { advance, deferred, fight, flush, follow, main, mountRuns, player, ready, region, saved, source } from './fixtures';
import { setDebugMode } from '../../src/lib/debug/debug';

beforeEach(() => { vi.useFakeTimers(); setDebugMode(false); });
afterEach(() => { cleanup(); setDebugMode(false); vi.clearAllTimers(); vi.useRealTimers(); });
const players = [1, 2, 3].map(rank => player({ rank, object_id: rank, player_id: `p${rank}`, name: `DemoPlayer ${rank}`,
  damage: 1000 - rank * 100, hits: 10, dps: 100, sources: [source({ damage: 600 }), source({ damage: 200, kind: 'ability' })] }));
const strip: RunFightStripRow[] = ['fight-1', 'fight-2'].map((fight_id, index) => ({ fight_id, target_type: null, boss_type: null,
  target_name: `DemoBoss ${index + 1}`, started: '0', ended: '1000000', killed: true, player_count: 3,
  damage: index ? null : 900, rank: index ? null : 1, share: index ? null : .5 }));
function context(overrides: Partial<PlayerContextView> = {}): PlayerContextView {
  return { profile: { scope_id: 'run-1', player_id: 'p1', object_id: 1, name: 'DemoPlayer 1', class_id: 100, is_local: true, maxed_gated: true,
    level: 20, stars: 50, maxed: 1, maxed_stats: [20], fame: 1234, stats: [{ stat_id: 20, value: 75 }, { stat_id: 48, value: 5 }],
    equipment: [{ slot: 0, item_type: 1003, enchant: source().enchant }], first_seen: '3000000', last_seen: '7000000' },
    gear_log: [1001, 1002, 1003].map((item_type, i) => ({ at: String([1000000, 4000000, 8000000][i]),
      equipment: [{ slot: 0, item_type, enchant: source().enchant }] })),
    timeline: { buckets: [
      { from: '1000000', until: '2000000', party_total: 10, party_active: 1 },
      { from: '3000000', until: '4000000', party_total: 600, party_active: 3 },
      { from: '4000000', until: '6000000', party_total: 900, party_active: 3 },
      { from: '8000000', until: '9000000', party_total: 10, party_active: 1 },
    ], points: [10, 200, 600, 10], compacted: false, disabled: false },
    deaths: [{ at: '6000000', fight_id: 'fight-1' }], ...overrides };
}
const hash = '#/runs/run-1/fights/fight-1/players/1';
const kpi = (label: string) => main().getAllByRole('term').find(row => (row.querySelector('button')?.textContent ?? row.textContent) === label)!.nextElementSibling!;
const scopeTimelineNote = 'Includes damage to other targets during this fight';

it.each([true, false])('uses bucket presence for an open fight with stale last_seen: %s', async live => {
  mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ scope_id: 'run-1', players, ended: live ? null : '8000000', last_hit: '8000000' })),
    getPlayerBreakdown: async () => ready(players[0]),
    getPlayerContext: async () => ready(context({ profile: { ...context().profile!, last_seen: '4000000' },
      timeline: { buckets: [
        { from: '3000000', until: '4000000', party_total: 100, party_active: 2 },
        { from: '4000000', until: '8000000', party_total: 100, party_active: 1 },
      ], points: [100, 0], compacted: true, disabled: false },
    })),
  } }); await flush();
  expect(main().getByText(/Guild.*in run/, { selector: 'p' }).textContent).toContain(`(${live ? '0:05' : '0:01'})`);
  expect(kpi('Active').textContent).toContain(live ? '20%' : '100%');
  expect(kpi('Active').textContent).toContain(live ? '0:01 of 0:05' : '0:01 of 0:01');
});

it.each(['enabled', 'disabled', 'missing'] as const)('charts the retained player series with unknown party average when realm context is %s', async mode => {
  const party = [...players, ...Array.from({ length: 5 }, (_, i) => player({ player_id: `extra-${i}`, object_id: 10 + i }))];
  mountRuns({ hash: '#/fights/fight-1/players/1', queries: {
    getFight: async () => ready(fight({ scope_id: 'run-1', run_id: null, players: party,
      buckets: [{ from: '3000000', until: '4000000', total: 800, local: 100 }],
      player_series: [{ player_id: 'p1', object_id: 1, is_local: true, points: [100] }],
    })), getPlayerBreakdown: async () => ready(players[0]),
    getPlayerContext: async () => ready(context({ timeline: mode === 'missing' ? null : { ...context().timeline!, disabled: mode === 'disabled' } })),
  } }); await flush();
  const chart = region('DPS over time');
  expect(chart.queryByText('Timeline unavailable (resource limit)')).toBeNull();
  expect(chart.getByRole('img')).toBeTruthy();
  const row = within(chart.getByRole('table', { name: 'DPS by interval' })).getAllByRole('row')[1];
  expect(within(row).getAllByRole('cell').map(cell => cell.textContent)).toEqual(['100', '—']);
  expect(chart.getByRole('img').querySelectorAll('[stroke-dasharray]')).toHaveLength(0);
  expect(chart.queryByText(scopeTimelineNote)).toBeNull();
  expect(kpi('Active').textContent).toContain('25%');
  expect(kpi('Active').textContent).toContain('0:01 of 0:04');
});

it('loads scoped gear at fight start, in-fight swaps, stats, deaths and overlapping presence', async () => {
  const page = mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ scope_id: 'run-1', players })), getPlayerBreakdown: async () => ready(players[0]),
    getPlayerContext: async () => ready(context()), getRunFightStrip: async () => ready(strip),
  } }); await flush();
  expect(page.queries.getPlayerContext).toHaveBeenCalledWith('run-1', 'p1', '2000000', '8000000');
  expect(region('Gear').getByText('Item 1001')).toBeTruthy(); expect(region('Gear').getByText(/at fight start/)).toBeTruthy();
  expect(region('Gear').getByText('Weapon → Item 1002')).toBeTruthy();
  expect(region('Gear').queryByText(/Started with/)).toBeNull(); expect(region('Gear').queryByText(/Item 1003/)).toBeNull();
  expect(region('Stats').getByText('75').parentElement?.getAttribute('data-tone')).toBe('max');
  expect(region('Stats').getByText('Stats not observed')).toBeTruthy();
  expect(kpi('Deaths').firstElementChild?.textContent).toBe('1');
  expect(kpi('Active').textContent).toContain('75%'); expect(kpi('Active').textContent).toContain('0:03 of 0:04');
  expect(region('Activity').getByText('Died')).toBeTruthy();
  expect(within(region('Boss fights').getByRole('row', { name: /DemoBoss 1/ })).getAllByRole('cell').at(-1)?.textContent).toBe('1');
  expect(page.visuals.mock.calls.flatMap(([types]) => types)).toEqual(expect.arrayContaining([100, 1001, 1002, 1003]));
});
it('does not read context for a legacy fight and keeps matched legacy deaths', async () => {
  const p = player({ deaths: 2 });
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: {
    getFight: async () => ready(fight({ scope_id: null, players: [p] })), getPlayerBreakdown: async () => ready(p),
  } }); await flush();
  expect(page.queries.getPlayerContext).not.toHaveBeenCalled();
  for (const label of ['Gear', 'Stats', 'DPS over time']) expect(region(label).getByText('Not recorded for this run')).toBeTruthy();
  expect(kpi('Deaths').firstElementChild?.textContent).toBe('2'); expect(kpi('Active').firstElementChild?.textContent).toBe('—');
});
it('loads realm scope with last_hit as its open-fight bound and no scope tabs', async () => {
  const page = mountRuns({ hash: '#/fights/fight-1/players/1', queries: {
    getFight: async () => ready(fight({ scope_id: 'test:map:2', run_id: null, ended: null, players })),
    getPlayerBreakdown: async () => ready(players[0]), getPlayerContext: async () => ready(context({ profile: { ...context().profile!, scope_id: 'test:map:2' } })),
  } }); await flush();
  expect(page.queries.getPlayerContext).toHaveBeenCalledWith('test:map:2', 'p1', '2000000', '7000000');
  expect(page.queries.getRunFightStrip).not.toHaveBeenCalled(); expect(main().queryByRole('navigation', { name: 'Player scope' })).toBeNull();
  expect(main().queryByText('Whole run')).toBeNull(); expect(region('Gear').getByText('Item 1001')).toBeTruthy();
});
it.each(['complete series', 'absent series', 'incomplete party series', 'missing player row'])('selects the correct chart timeline with %s', async mode => {
  const party = mode === 'missing player row'
    ? [...players, ...Array.from({ length: 5 }, (_, i) => player({ player_id: `extra-${i}`, object_id: 10 + i }))] : players;
  const retained = mode === 'complete series' ? party : mode === 'missing player row' ? party.slice(1, 7) : party.slice(0, 1);
  const usesContext = mode === 'absent series' || mode === 'missing player row';
  mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ scope_id: 'run-1', players: party, buckets: [
      { from: '3000000', until: '4000000', total: 600, local: null }, { from: '4000000', until: '6000000', total: 900, local: null },
    ], player_series: mode === 'absent series' ? null : retained
      .map(p => ({ player_id: p.player_id, object_id: p.object_id, is_local: p.is_local, points: [100, 400] })) })),
    getPlayerBreakdown: async () => ready(players[0]), getPlayerContext: async () => ready(context()),
  } }); await flush();
  const chart = region('DPS over time');
  expect(chart.getByRole('img', { name: 'Player and party average DPS' })).toBeTruthy();
  const rows = within(chart.getByRole('table', { name: 'DPS by interval' })).getAllByRole('row');
  expect(rows).toHaveLength(3);
  expect(within(rows[1]).getAllByRole('cell').map(cell => cell.textContent)).toEqual(usesContext ? ['200', '200'] : ['100', mode === 'complete series' ? '200' : '—']);
  expect(within(rows[2]).getAllByRole('cell').map(cell => cell.textContent)).toEqual(usesContext ? ['300', '150'] : ['200', mode === 'complete series' ? '150' : '—']);
  if (usesContext) expect(chart.getByText(scopeTimelineNote)).toBeTruthy();
  else expect(chart.queryByText(scopeTimelineNote)).toBeNull();
  expect(kpi('Active').textContent).toContain('75%');
  expect(kpi('Active').textContent).toContain('0:03 of 0:04');
});
it('shows the resource limit state and unknown Active without usable series', async () => {
  mountRuns({ hash, queries: { getFight: async () => ready(fight({ scope_id: 'run-1', players })),
    getPlayerBreakdown: async () => ready(players[0]), getPlayerContext: async () => ready(context({ timeline: { ...context().timeline!, disabled: true } })),
  } }); await flush();
  expect(region('DPS over time').getByText('Timeline unavailable (resource limit)')).toBeTruthy();
  expect(kpi('Active').firstElementChild?.textContent).toBe('—');
});
it('shows KPIs, one source table, rank navigation and run context without a run fetch', async () => {
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/1', queries: {
    getFight: async () => ready(fight({ players })),
    getPlayerBreakdown: async (_, id) => ready(players.find(row => row.object_id === id)!), getRunFightStrip: async () => ready(strip),
  } }); await flush();
  expect(within(main().getByRole('navigation', { name: 'Breadcrumb' })).getAllByRole('listitem').map(row => row.textContent?.replace('/', '').trim())).toEqual(['Runs', 'Run', 'DemoBoss 1', 'DemoPlayer 1']);
  expect(main().getByText('+0% vs avg')).toBeTruthy();
  expect(main().getByText('vs Top', { selector: 'dt' }).nextElementSibling?.textContent).toContain('100%');
  expect(main().getByText('Rank', { selector: 'dt' }).nextElementSibling?.textContent).toMatch(/#1\s*\/\s*3/);
  expect(region('Damage by item').getAllByRole('row')).toHaveLength(3);
  expect(region('Damage by item').queryByText('Enchantments')).toBeNull();
  expect(main().getByRole('button', { name: 'Previous player' }).hasAttribute('disabled')).toBe(true);
  expect(main().getByRole('link', { name: 'Next player: DemoPlayer 2' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/2');
  expect(page.queries.getRunFightStrip).toHaveBeenCalledWith('run-1', 'p1');
  expect(region('Boss fights').getByRole('row', { name: /DemoBoss 2/ }).textContent).toContain('—————');
  expect(region('Boss fights').getByRole('link', { name: 'DemoBoss 1' }).getAttribute('aria-current')).toBe('page');
  expect(main().getByRole('link', { name: /Whole run/ }).getAttribute('href')).toBe('#/runs/run-1/players/p1'); expect(page.queries.getRun).not.toHaveBeenCalled();
  const nav = within(main().getByRole('navigation', { name: 'Player navigation' }));
  await follow('#/runs/run-1/fights/fight-1/players/3', nav);
  expect(page.browser.adapter.hash).toBe('#/runs/run-1/fights/fight-1/players/3');
  expect(main().getByRole('button', { name: 'Next player' }).hasAttribute('disabled')).toBe(true);
  await follow('#/runs/run-1/fights/fight-1/players/2');
  expect(main().getByRole('link', { name: 'Previous player: DemoPlayer 1' }).getAttribute('href')).toBe(hash);
  expect(main().getByRole('link', { name: 'Next player: DemoPlayer 3' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/3');
});
it.each(['busy', 'error'] as const)('keeps the player visible when the strip is %s', async status => {
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: { getRunFightStrip: async () => {
    if (status === 'error') throw new Error('Synthetic failure'); return { status };
  } } }); await flush();
  expect(region('Damage by item').getAllByRole('row')).toHaveLength(2);
  expect(region('Boss fights').getByText('No boss fights observed')).toBeTruthy();
  expect(main().queryByText('Player Error')).toBeNull();
});
it('waits for player identity before fetching the strip and retires a pending strip on navigation', async () => {
  const detail = deferred<HistoryReply<PlayerDamage | null>>();
  const pending = deferred<HistoryReply<RunFightStripRow[]>>();
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: {
    getPlayerBreakdown: () => detail.promise, getRunFightStrip: () => pending.promise,
  } }); await flush();
  expect(page.queries.getRunFightStrip).not.toHaveBeenCalled();
  detail.resolve(ready(player())); await flush();
  expect(page.queries.getRunFightStrip).toHaveBeenCalledWith('run-1', 'player-1');
  await follow('#/runs', within(screen.getByRole('complementary', { name: 'Character' }))); pending.resolve(ready(strip)); await flush();
  expect(page.browser.adapter.hash).toBe('#/runs');
  expect(main().queryByRole('region', { name: 'Invented Archer this run' })).toBeNull();
});
it('shows selected-player DPS with time-weighted buckets and a text alternative', async () => {
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/1', queries: {
    getFight: async () => ready(fight({ players: [players[0]], buckets: [
      { from: '0', until: '1000000', total: 600, local: null }, { from: '1000000', until: '3000000', total: 900, local: null },
    ], player_series: [{ player_id: 'p1', object_id: 1, is_local: true, points: [100, 400] }] })),
    getPlayerBreakdown: async () => ready(players[0]),
  } }); await flush();
  const chart = region('DPS over time');
  expect(chart.getByRole('img', { name: 'Player and party average DPS' })).toBeTruthy();
  const rows = within(chart.getByRole('table', { name: 'DPS by interval' })).getAllByRole('row');
  expect(rows[1].textContent).toContain('100'); expect(rows[2].textContent).toContain('200');
  expect(chart.getByText('Party average')).toBeTruthy();
});
it('shows legacy unavailable state when no timeline is available', async () => {
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7' }); await flush();
  expect(region('DPS over time').getByText('Not recorded for this run')).toBeTruthy();
});

it('orders the player menu like neighbor navigation when ranks are missing or nonpositive', async () => {
  const rows = [0, 2, null, -1, 1].map((rank, index) => player({ rank, object_id: index + 1,
    player_id: `p${index + 1}`, name: `DemoPlayer ${index + 1}` }));
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/2', queries: {
    getFight: async () => ready(fight({ players: rows })),
    getPlayerBreakdown: async (_, id) => ready(rows.find(row => row.object_id === id)!),
  } }); await flush();
  const nav = within(main().getByRole('navigation', { name: 'Player navigation' }));
  expect(nav.getAllByRole('link').slice(0, 5).map(link => link.getAttribute('href'))).toEqual([5, 2, 1, 3, 4].map(id => `#/runs/run-1/fights/fight-1/players/${id}`));
  expect(main().getByRole('link', { name: 'Previous player: DemoPlayer 5' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/5');
  expect(main().getByRole('link', { name: 'Next player: DemoPlayer 1' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/1');
});

it('waits for the strip before fetching context and retires pending context on navigation', async () => {
  const pendingStrip = deferred<HistoryReply<RunFightStripRow[]>>();
  const pendingContext = deferred<HistoryReply<PlayerContextView | null>>();
  const page = mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ scope_id: 'run-1', players })), getPlayerBreakdown: async () => ready(players[0]),
    getRunFightStrip: () => pendingStrip.promise, getPlayerContext: () => pendingContext.promise,
  } }); await flush();
  expect(page.queries.getRunFightStrip).toHaveBeenCalledWith('run-1', 'p1');
  expect(page.queries.getPlayerContext).not.toHaveBeenCalled();
  pendingStrip.resolve(ready(strip)); await flush();
  expect(page.queries.getPlayerContext).toHaveBeenCalledWith('run-1', 'p1', '2000000', '8000000');
  expect(main().getByText('Loading Player')).toBeTruthy();
  await follow('#/runs', within(screen.getByRole('complementary', { name: 'Character' })));
  pendingContext.resolve(ready(context())); await flush();
  expect(page.browser.adapter.hash).toBe('#/runs'); expect(main().queryByRole('region', { name: 'Gear' })).toBeNull();
});
it('keeps ambiguous entries separate without context calls', async () => {
  const page = mountRuns({ hash, queries: { getFight: async () => ready(fight({ scope_id: 'run-1', players: [players[0],
    { ...players[0], player_id: 'second', name: 'Demo Second', sources: [source({ damage: 222 })] }] })) } }); await flush();
  expect(region('DemoPlayer 1 Sources').getAllByRole('row')).toHaveLength(3);
  expect(region('Demo Second Sources').getByText('222', { exact: true })).toBeTruthy();
  expect(page.queries.getPlayerContext).not.toHaveBeenCalled(); expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
});
it('links available boss scope, disables absent contributions and highlights the current fight', async () => {
  mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ players })), getPlayerBreakdown: async () => ready(players[0]),
    getRunFightStrip: async () => ready([...strip, { ...strip[0], fight_id: 'fight-3', target_name: 'DemoBoss 3' }]),
  } }); await flush();
  const tabs = within(main().getByRole('navigation', { name: 'Player scope' }));
  expect(tabs.getByRole('link', { name: /DemoBoss 1/ }).getAttribute('aria-current')).toBe('page');
  expect(tabs.getByRole('button', { name: /DemoBoss 2/ }).hasAttribute('disabled')).toBe(true);
  expect(tabs.getByRole('link', { name: /DemoBoss 3/ }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-3/players/1');
  expect(region('Boss fights').getByRole('link', { name: 'DemoBoss 3' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-3/players/1');
  expect(region('Boss fights').queryByRole('link', { name: 'DemoBoss 2' })).toBeNull();
});

it.each(['busy', 'error', 'limit', 'superseded'] as const)('keeps fight data visible when player context is %s', async status => {
  mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ scope_id: 'run-1', players })), getPlayerBreakdown: async () => ready(players[0]),
    getRunFightStrip: async () => ready(strip),
    getPlayerContext: async () => {
      if (status === 'error' || status === 'limit') throw new Error(status === 'limit' ? 'StoreError::Limit' : 'Synthetic context failure');
      return { status };
    },
  } }); await flush();
  expect(region('Damage by item').getAllByRole('row')).toHaveLength(3);
  expect(kpi('Damage').firstElementChild?.textContent).toBe('900');
  expect(kpi('Active DPS').firstElementChild?.textContent).toBe('100');
  expect(region('Boss fights').getByRole('link', { name: 'DemoBoss 1' })).toBeTruthy();
  expect(region('Gear').getByText('Gear unavailable')).toBeTruthy();
  expect(region('Stats').getByText('Stats unavailable')).toBeTruthy();
  expect(region('DPS over time').getByText('Timeline unavailable')).toBeTruthy();
  expect(main().queryByText('Not recorded for this run')).toBeNull();
  expect(main().queryByText('Player Error')).toBeNull();
});
it.each(['joined mid-fight', 'empty gear log'])('keeps gear observation unknown at fight start with %s', async mode => {
  mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ scope_id: 'run-1', players })), getPlayerBreakdown: async () => ready(players[0]),
    getPlayerContext: async () => ready(context({ gear_log: mode === 'empty gear log' ? [] : context().gear_log.slice(1) })),
  } }); await flush();
  expect(region('Gear').getByText('Not seen at fight start')).toBeTruthy();
  expect(region('Gear').queryByText('Not recorded for this run')).toBeNull();
  if (mode === 'joined mid-fight') expect(region('Gear').getByText('Started with Item 1002')).toBeTruthy();
});
it('preserves the coherent view when a refreshed profile belongs to a different identity', async () => {
  const page = mountRuns({ hash, queries: { getFight: async () => ready(fight({ scope_id: 'run-1', players })),
    getPlayerBreakdown: async () => ready(players[0]), getPlayerContext: async () => ready(context()),
  } }); await flush();
  page.queries.getPlayerContext.mockResolvedValue(ready(context({ profile: { ...context().profile!, player_id: 'unrelated', name: 'Wrong identity' } })));
  page.event({ fight_updated: fight({ scope_id: 'run-1', players }) }, '13'); page.setStore(saved('13')); await advance();
  expect(main().queryByText('Wrong identity')).toBeNull(); expect(region('Gear').getByText('Item 1001')).toBeTruthy();
});
it('shows estimates normally and retains debug Damage Evidence', async () => {
  setDebugMode(true);
  const p = { ...players[0], sources: [source({ estimated: true, reasons: ['inferred-equipped-weapon'] })] };
  mountRuns({ hash, queries: { getFight: async () => ready(fight({ players: [p] })), getPlayerBreakdown: async () => ready(p) } }); await flush();
  expect(main().getByLabelText('Damage Evidence')).toBeTruthy();
  setDebugMode(false); await flush();
  expect(main().queryByLabelText('Damage Evidence')).toBeNull();
  expect(region('Damage by item').getByText('est.')).toBeTruthy(); expect(region('Damage by item').getByText('Inferred from the equipped weapon')).toBeTruthy();
});

it('orders tied ranks by name in the picker and neighbor links', async () => {
  const rows = ['Demo Zeta', 'Demo Alpha', 'Demo Beta'].map((name, i) => player({ name, rank: 1, object_id: i + 1, player_id: `p${i + 1}` }));
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/3', queries: {
    getFight: async () => ready(fight({ players: rows })), getPlayerBreakdown: async () => ready(rows[2]),
  } }); await flush();
  const nav = within(main().getByRole('navigation', { name: 'Player navigation' }));
  expect(nav.getAllByRole('link').slice(0, 3).map(link => link.textContent)).toEqual(['Demo Alpha (You)', 'Demo Beta (You)', 'Demo Zeta (You)']);
  expect(nav.getByRole('link', { name: 'Previous player: Demo Alpha' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/2');
  expect(nav.getByRole('link', { name: 'Next player: Demo Zeta' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/1');
});

it('keeps reused-object entries in the picker as disabled choices', async () => {
  mountRuns({ hash, queries: {
    getFight: async () => ready(fight({ players: [players[0],
      player({ object_id: 2, player_id: 'reused-a', name: 'Demo Reused A' }),
      player({ object_id: 2, player_id: 'reused-b', name: 'Demo Reused B' })] })),
    getPlayerBreakdown: async () => ready(players[0]),
  } }); await flush();
  const nav = within(main().getByRole('navigation', { name: 'Player navigation' }));
  for (const name of ['Demo Reused A (You)', 'Demo Reused B (You)']) {
    expect(nav.getByRole('button', { name }).hasAttribute('disabled')).toBe(true);
    expect(nav.queryByRole('link', { name })).toBeNull();
  }
});

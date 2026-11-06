import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { RunPlayerView } from '../../src/lib/bindings/RunPlayerView';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { setDebugMode } from '../../src/lib/debug/debug';
import { advance, deferred, fight, flush, follow, main, mountRuns, ready, region, run, saved, source } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); setDebugMode(false); });
afterEach(() => { cleanup(); setDebugMode(false); vi.clearAllTimers(); vi.useRealTimers(); });
function view(overrides: Partial<RunPlayerView> = {}): RunPlayerView {
  return {
    run: run(), scope_id: 'run-1', player_id: 'p1',
    player: { player_id: 'p1', name: 'Demo Archer', class_id: 100, is_local: true, damage: 600,
      hits: 6, deaths: 1, share: .5, rank: 1, dps: 60 },
    party: { total_damage: 1200, average_dps: 40, top_name: 'Demo Archer', top_dps: 60, players_with_damage: 2 },
    sources: [source({ damage: 600, hits: 6, biggest_hit: 200, estimated: true, reasons: ['inferred-equipped-weapon'] })],
    fights: [
      { fight_id: 'fight-1', dps_basis: 'player_active_window_v1', object_id: 7, target_name: 'Demo Sentinel', started: '2000000', ended: '8000000',
        killed: true, player_count: 2, damage: 600, dps: 100, rank: 1, share: .5, deaths: 1 },
      { fight_id: 'fight-2', dps_basis: 'whole_target_v0', object_id: null, target_name: 'Demo King', started: '8000000', ended: '10000000',
        killed: true, player_count: 2, damage: null, dps: null, rank: null, share: null, deaths: null },
    ],
    profile: { scope_id: 'run-1', run_id: 'run-1', player_id: 'p1', object_id: 7, is_local: true,
      name: 'Demo Archer', class_id: 100, level: 20, stars: 50, maxed: 1, maxed_stats: [20], fame: 1234,
      stats: [{ stat_id: 20, value: 75 }, { stat_id: 48, value: 5 }],
      equipment: [{ slot: 0, item_type: 1001, enchant: source().enchant }], first_seen: '1000000', last_seen: '11000000' },
    gear_log: [{ at: '1000000', equipment: [] }, { at: '4000000', equipment: [{ slot: 0, item_type: 1002, enchant: source().enchant }] },
      { at: '6000000', equipment: [{ slot: 0, item_type: 1001, enchant: source().enchant }] }],
    timeline: { buckets: [{ from: '2000000', until: '3000000', party_total: 300, party_active: 2 },
      { from: '3000000', until: '4000000', party_total: 900, party_active: 2 }], points: [200, 400], compacted: false, disabled: false },
    deaths: [{ at: '7000000', fight_id: 'fight-1' }], departed_at: '10000000',
    roster: [{ player_id: 'p1', name: 'Demo Archer', class_id: 100, is_local: true, damage: 600 },
      { player_id: 'p2', name: 'Demo Wizard', class_id: 101, is_local: false, damage: 500 },
      { player_id: 'quiet/player', name: 'Demo Quiet', class_id: 102, is_local: false, damage: null }], partial: true,
    ...overrides,
  };
}
const hash = '#/runs/run-1/players/p1';
const kpi = (label: string) => main().getAllByRole('term').find(row => (row.querySelector('button')?.textContent ?? row.textContent) === label)!.nextElementSibling!;

it.each([true, false])('uses live bucket presence for header, Active, tabs, boss bands and idle gaps: %s', async live => {
  const current = view();
  mountRuns({ hash, queries: { getRunPlayer: async () => ready({ ...current,
    run: run({ started: '0', ended: live ? null : '62000000' }),
    profile: { ...current.profile!, first_seen: '0', last_seen: '2000000' },
    fights: [{ ...current.fights[0], started: '0', ended: null }],
    timeline: { buckets: [
      { from: '0', until: '2000000', party_total: 100, party_active: 2 },
      { from: '2000000', until: '62000000', party_total: 100, party_active: 1 },
    ], points: [100, 0], compacted: true, disabled: false },
  }) } }); await flush();
  expect(main().getByText(/Guild.*in run/, { selector: 'p' }).textContent).toContain(`(${live ? '1:02' : '0:02'})`);
  expect(kpi('Active').textContent).toContain(live ? '3.2%' : '100%');
  expect(region('Activity').queryByText('Idle 1:00 (no damage)') !== null).toBe(live);
  const tabs = within(main().getByRole('navigation', { name: 'Player scope' }));
  expect(tabs.getByRole('link', { name: /Whole run/ }).textContent).toContain('1:02');
  expect(tabs.getByRole('link', { name: /Demo Sentinel/ }).textContent).toContain('1:02');
  const band = region('DPS over time').getByRole('img').querySelector('rect');
  expect(band?.getAttribute('width')).toBe('800');
});

it('renders the full run view with eight KPIs, activity, source totals, gear and stats', async () => {
  const page = mountRuns({ hash, queries: { getRunPlayer: async () => ready(view()) } }); await flush();
  expect(page.queries.getRunPlayer).toHaveBeenCalledWith('run-1', 'p1');
  expect(within(main().getByRole('navigation', { name: 'Breadcrumb' })).getAllByRole('listitem').map(row => row.textContent?.replace('/', '').trim())).toEqual(['Runs', 'Run', 'Demo Archer']);
  expect(main().getAllByRole('term').filter(row => ['Damage', 'DPS', 'vs Top', 'Rank', 'Hits', 'Best hit', 'Deaths', 'Active'].includes(row.querySelector('button')?.textContent ?? row.textContent!))).toHaveLength(8);
  expect(kpi('Damage').textContent).toContain('600'); expect(kpi('DPS').textContent).toContain('+50% vs avg');
  expect(kpi('Best hit').textContent).toContain('200'); expect(kpi('Active').textContent).toContain('20%');
  expect(main().getByText('partial')).toBeTruthy(); expect(main().getByText('You')).toBeTruthy();
  expect(main().queryByLabelText('Damage Evidence')).toBeNull();
  expect(region('DPS over time').getByRole('img', { name: 'Player and party average DPS' })).toBeTruthy();
  expect(region('DPS over time').getAllByText('Demo Sentinel').length).toBeGreaterThan(0);
  expect(region('Damage by item').getByText('1 source · 600 total')).toBeTruthy();
  expect(region('Damage by item').getByText('est.')).toBeTruthy();
  expect(region('Damage by item').getByText('Inferred from the equipped weapon')).toBeTruthy();
  expect(region('Boss fights').getAllByRole('row')).toHaveLength(3);
  expect(region('Gear').getByText('Item 1001')).toBeTruthy();
  expect(region('Gear').getByText('Started with Not seen')).toBeTruthy();
  expect(region('Gear').getByText('Weapon → Item 1001')).toBeTruthy();
  expect(region('Stats').getByText('75').parentElement?.getAttribute('data-tone')).toBe('max');
  expect(region('Stats').getByText('Stats not observed')).toBeTruthy();
  for (const text of ['Entered run', 'First hit', 'Died', 'Departed run', 'Last hit']) expect(region('Activity').getByText(text)).toBeTruthy();
  const requested = page.visuals.mock.calls.flatMap(([types]) => types);
  expect(requested).toEqual(expect.arrayContaining([100, 101, 102, 1000, 1001, 1002]));
  expect(page.queries.getRun).not.toHaveBeenCalled(); expect(page.queries.getFight).not.toHaveBeenCalled();
});
it('keeps no-damage roster players available with unknown damage and observed gear and stats', async () => {
  const current = view();
  mountRuns({ hash, queries: { getRunPlayer: async () => ready(view({ player: null, sources: [], deaths: [],
    party: { total_damage: 500, average_dps: 50, top_name: 'Demo Wizard', top_dps: 50, players_with_damage: 1 },
    roster: current.roster.map(row => row.player_id === 'p1' ? { ...row, damage: null } : row),
    timeline: { ...current.timeline!, points: [0, 0] },
    fights: current.fights.map(row => ({ ...row, object_id: null, damage: null, dps: null, rank: null, share: null, deaths: null })),
  })) } }); await flush();
  for (const label of ['Damage', 'DPS', 'vs Top', 'Hits', 'Best hit']) expect(kpi(label).firstElementChild?.textContent).toBe('—');
  expect(kpi('Rank').firstElementChild?.textContent).toMatch(/^—/);
  expect(region('Damage by item').getByText('No damage observed')).toBeTruthy();
  expect(region('Damage by item').queryByRole('table')).toBeNull();
  expect(region('Boss fights').getByRole('row', { name: /Demo Sentinel/ }).textContent).toContain('—————');
  expect(region('Gear').getByText('Item 1001')).toBeTruthy(); expect(region('Stats').getByText('75')).toBeTruthy();
  expect(main().getByRole('heading', { name: /Demo Archer/ })).toBeTruthy();
});
it('shows legacy unknowns while retaining sources and boss results', async () => {
  mountRuns({ hash, queries: { getRunPlayer: async () => ready(view({ profile: null, gear_log: [], timeline: null, deaths: [] })) } }); await flush();
  expect(kpi('Deaths').firstElementChild?.textContent).toBe('1');
  for (const label of ['Gear', 'Stats', 'DPS over time']) expect(region(label).getByText('Not recorded for this run')).toBeTruthy();
  expect(region('Damage by item').getByRole('table')).toBeTruthy(); expect(region('Boss fights').getAllByRole('row')).toHaveLength(3);
});
it('links boss scope with the fight object id and disables an unavailable boss scope', async () => {
  const page = mountRuns({ hash, queries: { getRunPlayer: async () => ready(view()) } }); await flush();
  const tabs = within(main().getByRole('navigation', { name: 'Player scope' }));
  expect(tabs.getByRole('link', { name: /Whole run/ }).getAttribute('aria-current')).toBe('page');
  expect(tabs.getByRole('link', { name: /Demo Sentinel/ }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/7');
  expect(tabs.getByRole('button', { name: /Demo King/ }).hasAttribute('disabled')).toBe(true);
  expect(region('Boss fights').queryByRole('link', { name: 'Demo King' })).toBeNull();
  await follow('#/runs/run-1/fights/fight-1/players/7', tabs);
  expect(page.browser.adapter.hash).toBe('#/runs/run-1/fights/fight-1/players/7');
});
it('uses roster order for the picker and neighbors and encodes player ids', async () => {
  const page = mountRuns({ hash, queries: { getRunPlayer: async (_, playerId) => {
    const current = view(), entry = current.roster.find(row => row.player_id === playerId)!;
    return ready({ ...current, player_id: playerId, player: playerId === 'p1' ? current.player : null,
      profile: { ...current.profile!, player_id: playerId, name: entry.name, class_id: entry.class_id, is_local: entry.is_local } });
  } } }); await flush();
  const nav = within(main().getByRole('navigation', { name: 'Player navigation' }));
  expect(nav.getByRole('button', { name: 'Previous player' }).hasAttribute('disabled')).toBe(true);
  expect(nav.getByRole('link', { name: 'Next player: Demo Wizard' }).getAttribute('href')).toBe('#/runs/run-1/players/p2');
  await fireEvent.click(nav.getByText('Demo Archer (You)', { selector: 'summary' })); await flush();
  expect(nav.getAllByRole('link').slice(0, 3).map(link => link.getAttribute('href'))).toEqual([
    '#/runs/run-1/players/p1', '#/runs/run-1/players/p2', '#/runs/run-1/players/quiet%2Fplayer',
  ]);
  await follow('#/runs/run-1/players/quiet%2Fplayer', nav);
  expect(page.queries.getRunPlayer).toHaveBeenLastCalledWith('run-1', 'quiet/player');
  expect(main().getByRole('heading', { name: /^Demo Quiet/ })).toBeTruthy();
  expect(main().getByRole('button', { name: 'Next player' }).hasAttribute('disabled')).toBe(true);
  expect(main().getByRole('link', { name: 'Previous player: Demo Wizard' }).getAttribute('href')).toBe('#/runs/run-1/players/p2');
});
it('shows Live for an open run and the resource limit timeline state', async () => {
  const current = view();
  mountRuns({ hash, queries: { getRunPlayer: async () => ready({ ...current, run: run({ ended: null }), timeline: { ...current.timeline!, disabled: true } }) } }); await flush();
  expect(main().getByText('Live')).toBeTruthy(); expect(region('DPS over time').getByText('Timeline unavailable (resource limit)')).toBeTruthy();
  expect(kpi('Active').firstElementChild?.textContent).toBe('—');
});
it('re-reads for a matching run event after its saved watermark, preserving the view while waiting', async () => {
  const page = mountRuns({ hash, queries: { getRunPlayer: async () => ready(view()) } }); await flush();
  page.event({ fight_updated: fight({ run_id: 'unrelated' }) }, '13'); await advance();
  expect(page.queries.getRunPlayer).toHaveBeenCalledTimes(1);
  page.event({ fight_updated: fight() }, '14'); await advance();
  expect(page.queries.getRunPlayer).toHaveBeenCalledTimes(1); expect(region('Gear').getByText('Item 1001')).toBeTruthy();
  page.setStore(saved('14', page.snapshot.stream_id)); await advance(2000);
  expect(page.queries.getRunPlayer).toHaveBeenCalledTimes(2);
});
it('waits for history and retires a pending read on navigation', async () => {
  const pending = deferred<HistoryReply<RunPlayerView | null>>();
  const page = mountRuns({ hash, queries: { getRunPlayer: () => pending.promise } }); await flush();
  expect(page.queries.getRunPlayer).toHaveBeenCalledTimes(1); expect(main().getByText('Loading Player')).toBeTruthy();
  await follow('#/runs', within(screen.getByRole('complementary', { name: 'Character' }))); pending.resolve(ready(view())); await flush();
  expect(page.browser.adapter.hash).toBe('#/runs'); expect(main().queryByRole('region', { name: 'Gear' })).toBeNull();
});
it('shows a missing player state for a null history result', async () => {
  mountRuns({ hash, queries: { getRunPlayer: async () => ready(null) } }); await flush();
  expect(main().getByText('Player Not Found')).toBeTruthy();
});
it('retries a failed read through the history controller', async () => {
  let fail = true;
  mountRuns({ hash, queries: { getRunPlayer: async () => { if (fail) throw new Error('Synthetic read failure'); return ready(view()); } } }); await flush();
  expect(main().getByText('Player Error')).toBeTruthy(); fail = false;
  await fireEvent.click(main().getByRole('button', { name: 'Retry' })); await flush();
  expect(region('Gear').getByText('Item 1001')).toBeTruthy();
});
it('keeps Damage Evidence and source reasons in debug mode', async () => {
  setDebugMode(true);
  mountRuns({ hash, queries: { getRunPlayer: async () => ready(view()) } }); await flush();
  const details = main().getByLabelText('Damage Evidence');
  await fireEvent.click(within(details).getByText('Damage Evidence')); await flush();
  expect(within(details).getByText(/Inferred from the equipped weapon/)).toBeTruthy();
  expect(within(details).getByText(/Partial capture/)).toBeTruthy();
});

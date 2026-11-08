import { setDebugMode } from '../../src/lib/debug/debug';
import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { CoverageIssue } from '../../src/lib/bindings/CoverageIssue';
import { breadcrumbs, parseRoute, routeHref, routeTitle } from '../../src/lib/navigation/routes';
import { advance, click, deferred, fight, flush, follow, main, mountRuns, player, ready, region, saved, source } from './fixtures';

beforeEach(() => { setDebugMode(true);
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});

it.each(['test:player:3:42:0', 'a b%2F/c', 'x:local:1'])('round trips run player identity (%s)', playerId => {
  const route = { kind: 'run-player' as const, runId: 'a b%2F/c', playerId };
  expect(parseRoute(routeHref(route))).toEqual(route);
  expect(routeTitle(route)).toBe('Player');
  expect(breadcrumbs(route)).toEqual([
    { label: 'Home', href: '#/' }, { label: 'Runs', href: '#/runs' },
    { label: 'Run', href: '#/runs/a%20b%252F%2Fc' }, { label: 'Player', href: routeHref(route) },
  ]);
});

it.each(['#/runs/run-1/players', '#/runs/run-1/players/'])('rejects a missing run player segment (%s)', hash => {
  expect(parseRoute(hash)).toEqual({ kind: 'not-found' });
});
afterEach(() => { setDebugMode(false);
  cleanup();
  vi.clearAllTimers();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it('run_fight_player_source_chain', async () => {
  const page = mountRuns(); await flush(); await follow('#/runs/run-1');
  expect(page.queries.getFight).not.toHaveBeenCalled();
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush(); await follow('#/runs/run-1/fights/fight-1', region('Fight damage'));
  expect(main().getByRole('heading', { name: 'Fight', level: 1 })).toBeTruthy();
  expect(page.queries.getFight).toHaveBeenCalledTimes(1);
  await follow('#/runs/run-1/fights/fight-1/players/7');
  expect(main().getByRole('heading', { name: 'Player', level: 1 })).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledWith('fight-1', 7);
  expect(region('Damage by item').getByRole('cell', { name: 'Weapon' })).toBeTruthy();
  expect(region('Damage by item').getByText('321', { exact: true })).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
});

it('unknown_estimated_and_known_zero_remain_distinct', async () => {
  const selected = player({ dps: null, rank: null, share: null, biggest_hit: null, sources: [
    source({ item_type: null, kind: 'unknown', damage: 0, hits: 1, biggest_hit: 0 }),
    source({ item_type: 1001, damage: 50, estimated: true, reasons: ['Missing target DEF; assumed zero'] }),
  ] });
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: {
    getFight: async () => ready(fight({ players: [selected] })), getPlayerBreakdown: async () => ready(selected),
  } }); await flush();
  expect(main().getAllByText(/unavailable/i).length).toBeGreaterThan(0);
  expect(region('Damage by item').getByRole('rowheader', { name: /Unknown Damage/ })).toBeTruthy();
  expect(region('Damage by item').getAllByText(/^0$/).length).toBeGreaterThan(0);
  expect(region('Damage by item').getAllByText(/estimated/i).length).toBeGreaterThan(0);
  expect(region('Damage by item').getByText(/Missing target DEF; assumed zero/)).toBeTruthy();
  expect(main().getByText(/Detailed reconstruction counts were not saved for this run/i)).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledTimes(1);
});

it('supplied_metrics_and_unattributed_total_are_not_recomputed', async () => {
  mountRuns({ hash: '#/runs/run-1/fights/fight-1' }); await flush();
  // Only one displayed player, deliberately rank 4, share 23%, DPS 17: never
  // reinterpret the subset as rank 1/share 100%, nor add unattributed 79 to 1400.
  const row = region('Players').getByRole('row', { name: /Invented Archer/ });
  expect(row.textContent).toMatch(/23\s*%/); expect(row.textContent).toMatch(/17/);
  expect(row.textContent).toMatch(/4/);
  expect(within(row).getByLabelText('Biggest Hit').textContent).toMatch(/123/);
  expect(within(row).getByLabelText('Deaths')).toBeTruthy();
  expect(main().getByText(/^1 players · 79 unattributed$/)).toBeTruthy();
  expect(main().getByText(/1[,.]?400/)).toBeTruthy(); expect(main().getAllByText(/79/).length).toBeGreaterThan(0);
  expect(main().queryByText(/1[,.]?479/)).toBeNull();
});

it.each([null, 0, NaN, Infinity, -1])('null_zero_and_nonfinite_metrics_are_guarded (%s)', async dps => {
  mountRuns({ hash: '#/fights/fight-1', queries: { getFight: async () => ready(fight({ run_id: null,
    first_hit: '3000000', last_hit: '3000000', players: [player({ dps, share: dps, biggest_hit: dps, rank: 4, deaths: 2 })],
  })) } }); await flush();
  const row = region('Players').getByRole('row', { name: /Invented Archer/ });
  expect(row.textContent).not.toMatch(/NaN|Infinity|-1/);
  for (const label of ['Active DPS', 'Share', 'Biggest Hit']) {
    const value = within(row).getByLabelText(label).textContent?.trim();
    expect(value).toMatch(dps === 0 ? /^0(?:\s*(?:%|DPS))?$/i : /^\u2014$/);
  }
  expect(main().getByRole('heading', { name: 'Fight', level: 1 })).toBeTruthy();
  expect(main().queryByRole('link', { name: 'Run' })).toBeNull();
});

it('reused_object_ids_show_separate_inline_sources_without_arbitrary_identity', async () => {
  const first = player({ player_id: 'first', name: 'First incarnation', sources: [source({ damage: 111 })] });
  const second = player({ player_id: 'second', name: 'Second incarnation', sources: [source({ damage: 222 })] });
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: {
    getFight: async () => ready(fight({ players: [first, second] })), getPlayerBreakdown: async () => ready(null),
  } }); await flush();
  expect(main().getByText(/ambiguous|reused object/i)).toBeTruthy();
  expect(main().getByRole('region', { name: 'First incarnation Sources' }).textContent).toContain('111');
  expect(main().getByRole('region', { name: 'Second incarnation Sources' }).textContent).toContain('222');
  expect(main().queryByRole('button', { name: /Retry/i })).toBeNull();
  expect(page.queries.getFight).toHaveBeenCalledTimes(1);
});

it('ambiguous_entries_keep_separate_full_tables_and_restore_a_long_entry_row', async () => {
  const first = player({ player_id: 'long-entry', name: 'Long incarnation', sources:
    Array.from({ length: 7 }, (_, index) => source({ item_type: 2000 + index })) });
  const second = player({ player_id: 'short-entry', name: 'Short incarnation', sources:
    Array.from({ length: 2 }, (_, index) => source({ item_type: 3000 + index })) });
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: {
    getFight: async () => ready(fight({ players: [first, second] })),
    getPlayerBreakdown: async () => ready(null),
  } }); await flush();
  const longer = () => region('Long incarnation Sources');
  const shorter = () => region('Short incarnation Sources');
  const rows = longer().getAllByRole('row').slice(1);
  expect(rows).toHaveLength(7);
  expect(shorter().getAllByRole('row').slice(1)).toHaveLength(2);
  expect(shorter().getByText('Item 3000')).toBeTruthy();
  expect(rows.map(row => within(row).getByText(/^Item/).textContent)).toEqual(Array.from({ length: 7 }, (_, index) => `Item ${2000 + index}`));
  expect(longer().queryByRole('button', { name: 'Next Window' })).toBeNull();
  const lastRow = rows.at(-1)!;
  lastRow.focus();
  await follow('#/runs/run-1/fights/fight-1');
  await click('Back', within(screen.getByRole('group', { name: 'History' })));
  await flush();
  const restored = longer().getAllByRole('row').slice(1);
  expect(restored).toHaveLength(7);
  expect(within(restored[6]).getByText('Item 2006')).toBeTruthy();
  expect(document.activeElement).toBe(restored[6]);
  expect(shorter().getAllByRole('row').slice(1)).toHaveLength(2);
  expect(shorter().getByText('Item 3000')).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
});

it('all seven players are visible and local chart selection does not hide rows', async () => {
  const players = Array.from({ length: 7 }, (_, index) => player({ player_id: `entry-${index}`,
    object_id: index, name: `Observed player ${index}`, rank: index + 1, is_local: index === 6 }));
  const page = mountRuns({ hash: '#/fights/fight-1', queries: { getFight: async () => ready(fight({ run_id: null, players })) } });
  await flush();
  expect(region('Players').getAllByRole('link')).toHaveLength(7);
  expect(region('Players').getByRole('link', { name: '7 Observed player 6' })).toBeTruthy();
  expect(region('Players').queryByRole('button', { name: 'Next Players' })).toBeNull();
  expect(region('Players').queryByRole('button', { name: /Details/ })).toBeNull();
  expect(region('Damage over time').getByRole('button', { name: 'Observed player 6' }).getAttribute('aria-pressed')).toBe('true');
  const row = region('Players').getByRole('row', { name: /Observed player 0/ });
  expect(within(row).getByLabelText('Active DPS').textContent).toBe('17');
  expect(within(row).getByLabelText('Share').textContent).toBe('23%');
  await fireEvent.click(within(row).getByLabelText('Active DPS')); await flush();
  expect(page.browser.adapter.hash).toBe('#/fights/fight-1/players/0');
});

it.each([-2147483648, -7, 0, 2147483647])('canonical_signed_object_id_round_trip_and_render (%s)', async objectId => {
  const route = { kind: 'player' as const, fightId: 'fight /?#', runId: 'run /?#', objectId };
  expect(parseRoute(routeHref(route))).toEqual(route);
  const selected = player({ object_id: objectId });
  const page = mountRuns({ hash: `#/fights/fight-1/players/${objectId}`, queries: {
    getFight: async () => ready(fight({ run_id: null, players: [selected] })), getPlayerBreakdown: async () => ready(selected),
  } }); await flush();
  expect(main().getByRole('heading', { name: 'Player', level: 1 })).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledWith('fight-1', objectId);
  expect(region('Damage by item').getByText('321', { exact: true })).toBeTruthy();
});

it.each(['-0', '01', '-01', '+1', '-2147483649', '2147483648', '1.5', 'NaN', 'Infinity'])('rejects_noncanonical_player_route (%s)', value => {
  expect(parseRoute(`#/fights/fight-1/players/${value}`).kind).toBe('not-found');
  expect(parseRoute('#/fights/fight-1/players/7')).toMatchObject({ kind: 'player', objectId: 7 });
});

it('nested_run_mismatch_never_reads_or_links_the_wrong_player', async () => {
  const page = mountRuns({ hash: '#/runs/wrong-run/fights/fight-1/players/7' }); await flush();
  expect(main().getByText(/Player Unavailable/i)).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  expect(main().queryByRole('link', { name: /Invented Archer/ })).toBeNull();
  expect(page.queries.getFight).toHaveBeenCalledWith('fight-1');
});

it.each(['fight', 'player'] as const)('missing_detail_is_distinct_from_read_failure (%s)', async missing => {
  const page = mountRuns({ hash: missing === 'fight' ? '#/fights/missing' : '#/fights/fight-1/players/7', queries: {
    getFight: async () => ready(missing === 'fight' ? null : fight({ run_id: null })), getPlayerBreakdown: async () => ready(null),
  } }); await flush();
  expect(main().getByText(/not found|unavailable|not saved/i)).toBeTruthy();
  expect(main().queryByRole('button', { name: /Retry/i })).toBeNull();
  expect(screen.getByRole('button', { name: 'Back' })).toBeTruthy();
  expect(page.queries.getFight).toHaveBeenCalledTimes(1);
});

it('sources_preserve_kind_enchant_provenance_and_escaped_reasons', async () => {
  const reasons = ['Crucible/Blood Ritual neutral 1.0', 'missing target stats default zero',
    'matching-ability enchant neutral 1.0', 'missing scaling stat zero bonus',
    'unknown positive enchant omits reconstruction', '<img src=x onerror=alert(1)>'];
  const sources = (['weapon', 'ability', 'summon', 'item_effect', 'other', 'unknown'] as const)
    .map((kind, i) => source({ kind, ability: kind === 'ability', damage: 101 + i, estimated: true,
      reasons: [reasons[i]], enchant: { state: 'recorded', slots: 1, applied: 1, ids: [i + 1] } }));
  sources.push(source({ estimated: true, reasons: [], damage: 777 }));
  const selected = player({ sources });
  mountRuns({ hash: '#/fights/fight-1/players/7', queries: {
    getFight: async () => ready(fight({ run_id: null, players: [selected] })),
    getPlayerBreakdown: async () => ready(selected),
  } }); await flush();
  const scope = region('Damage by item');
  expect(scope.getAllByRole('row').slice(1)).toHaveLength(7);
  for (const kind of ['Weapon', 'Ability', 'Summon', 'Item Effect', 'Other', 'Unknown']) {
    expect(scope.getAllByRole('cell', { name: new RegExp(kind) }).length).toBeGreaterThan(0);
  }
  expect(scope.queryByText('Enchantments', { selector: 'summary' })).toBeNull();
  for (const reason of reasons) expect(scope.getByText(reason, { exact: false })).toBeTruthy();
  expect(scope.getAllByText(/estimated/i).length).toBeGreaterThan(0);
  expect(scope.getByText(/Estimated damage; reconstruction reasons unavailable/i)).toBeTruthy();
  expect(scope.getByText('777', { exact: true })).toBeTruthy();
  expect(screen.getByRole('region', { name: 'Damage by item' }).querySelector('img[src="x"], img[onerror]')).toBeNull();
  expect(scope.getAllByRole('img', { name: 'Uncommon, 1 enchant' })).toHaveLength(6);

});

it.each(['player_id', 'object_id'] as const)('player_context_read_is_sequential_and_mismatched_revision_keeps_coherent_view (%s)', async mismatch => {
  const pending = deferred<HistoryReply<FightView | null>>();
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: { getFight: () => pending.promise } }); await flush();
  expect(page.queries.getFight).toHaveBeenCalledTimes(1);
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  pending.resolve(ready(fight())); await flush();
  expect(main().getByRole('heading', { name: 'Player', level: 1 })).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledTimes(1);
  page.queries.getFight.mockResolvedValue(ready(fight()));
  page.queries.getPlayerBreakdown.mockResolvedValue(ready(player({
    ...(mismatch === 'player_id' ? { player_id: 'unrelated' } : { object_id: 99 }), name: 'Wrong identity', damage: 999,
  })));
  page.event({ fight_updated: fight() }, '13'); page.setStore(saved('13')); await advance();
  expect(main().queryByText('Wrong identity')).toBeNull();
  expect(main().getByRole('heading', { name: 'Invented Archer You' })).toBeTruthy();
  page.queries.getPlayerBreakdown.mockResolvedValue(ready(player({ name: 'Coherent revision', damage: 654 })));
  page.queries.getFight.mockResolvedValue(ready(fight({ players: [player({ name: 'Coherent revision', damage: 654 })] })));
  page.event({ fight_updated: fight() }, '14'); page.setStore(saved('14')); await advance();
  expect(main().getByRole('heading', { name: 'Coherent revision You' })).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledTimes(3);
});

it.each([
  ['missing_stats', /missing.*stat|stat.*missing|assum/i],
  ['missing_identity', /identity|unverified/i],
  ['missing_catalog', /catalog|metadata/i],
  ['missing_projectile', /projectile/i],
  ['ambiguous_hit', /ambiguous|uncertain.*hit/i],
  ['resource_limit', /resource|limit/i],
  ['midstream', /midstream|already.*progress|capture.*start/i],
  ['decode_failure', /decode|decod/i],
  ['stream_gap', /gap|missing.*traffic/i],
  ['clock_regression', /clock|time.*back/i],
] as const)('durable_coverage_issues_are_explained_without_invented_counters (%s)', async (issue, copy) => {
  mountRuns({ hash: '#/fights/fight-1', queries: { getFight: async () => ready(fight({ run_id: null,
    complete_capture: false, issues: [issue as CoverageIssue],
  })) } }); await flush();
  expect(main().getAllByText(copy).length).toBeGreaterThan(0);
  expect(main().getAllByText(/partial|incomplete/i).length).toBeGreaterThan(0);
  expect(main().queryByText(/0 omitted|0 missing hits|all hits reconstructed/i)).toBeNull();
  expect(main().getByRole('link', { name: /Invented Archer/ })).toBeTruthy();
});

it.each(['superseded', 'busy'] as const)('player_context_unwraps_control_before_breakdown (%s)', async status => {
  const page = mountRuns({ hash: '#/fights/fight-1/players/7', queries: { getFight: async () => ({ status }) } }); await flush();
  await advance(2000);
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  expect(page.queries.getFight).toHaveBeenCalledTimes(2);
  page.queries.getFight.mockResolvedValue(ready(fight({ run_id: null })));
  if (status === 'busy') await click(/Retry/i);
  else { page.event({ fight_updated: fight({ run_id: null }) }, '13'); page.setStore(saved('13')); await advance(); }
  expect(main().getByRole('heading', { name: 'Player', level: 1 })).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledTimes(1);
  expect(region('Damage by item').getByText('321', { exact: true })).toBeTruthy();
});

it('shows one item row with its estimated damage share in the fight summary', async () => {
  setDebugMode(false);
  mountRuns({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getFight: async () => ready(fight({ players: [player({ damage: 100, sources: [
      source({ damage: 68 }), source({ damage: 32, estimated: true }),
    ] })] })),
  } }); await flush();
  const sources = region('Your damage sources');
  expect(sources.getAllByText('100', { exact: true })).toHaveLength(1);
  expect(sources.getByText('~32% est.')).toBeTruthy();
  expect(sources.queryByText('68', { exact: true })).toBeNull();
});

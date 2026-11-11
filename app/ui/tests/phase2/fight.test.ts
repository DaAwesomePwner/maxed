import { setDebugMode } from '../../src/lib/debug/debug';
import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunFightStripRow } from '../../src/lib/bindings/RunFightStripRow';
import DamageChart from '../../src/lib/home/v2/DamageChart.svelte';
import { advance, click, deferred, fight, flush, follow, history, main, mountPhase2, player, primary, ready, region, saved, source } from './fixtures';

beforeEach(() => { setDebugMode(true); vi.useFakeTimers(); });
afterEach(() => { setDebugMode(false); cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('phase2_fight_identity_series_and_sources', async () => {
  const selected = player({ object_id: -7, sources: [
    source({ item_type: 1000, damage: 111, enchant: { state: 'recorded_empty', slots: 1, applied: 0, ids: [] } }),
    source({ item_type: 1000, damage: 222, enchant: { state: 'recorded', slots: 1, applied: 1, ids: [42] } }),
    source({ item_type: null, kind: 'unknown', damage: 0, biggest_hit: null, estimated: true, reasons: ['<img src=x onerror=alert(1)>'] }),
  ] });
  const detail = fight({ run_id: null, players: [selected], buckets: [
    { from: '3000000', until: '4000000', total: 200, local: 0 },
    { from: '4000000', until: '6000000', total: 600, local: null },
    { from: '6000000', until: '7000000', total: 500, local: 100 },
  ], player_series: [{ player_id: selected.player_id, object_id: -7, is_local: true, points: [10, 40, 30] }] });
  const page = mountPhase2({ hash: '#/fights/fight-1', queries: {
    getFight: async () => ready(detail), getPlayerBreakdown: async () => ready(selected),
  } }); await flush();
  expect(main().getByRole('img', { name: 'Fight DPS' })).toBeTruthy();
  const row = region('Players').getByRole('link', { name: /Invented Archer/ });
  expect(row.getAttribute('aria-label')).toBe('4 Invented Archer');
  expect(within(row.closest('tr')!).getByLabelText('Share').textContent).toMatch(/^23\s*%$/);
  expect(within(row.closest('tr')!).getByLabelText('Active DPS').textContent).toBe('17');
  await follow('#/fights/fight-1/players/-7');
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledWith('fight-1', -7);
  const sources = main().getByRole('region', { name: 'Damage by item' });
  // All saved rows remain separate even with identical item IDs.
  expect(sources.textContent).toContain('111'); expect(sources.textContent).toContain('222');
  expect(sources.textContent).toContain('42'); expect(sources.textContent).toContain('<img src=x onerror=alert(1)>');
  expect(sources.querySelector('img[src="x"]')).toBeNull();
  expect(page.queries.getRun).not.toHaveBeenCalled();
});

it.each(['omitted', 'null', 'provisional'] as const)('phase2_fight_kill_is_not_end_reason (%s)', async mode => {
  const value = fight({ kill: mode === 'provisional' ? { at: '7000000', evidence: 'hp_zero_removed', provisional: true } : null });
  if (mode === 'omitted') { delete value.kill; delete value.player_series; delete value.target_kind; }
  mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: { getFight: async () => ready(value) } }); await flush();
  const evidence = main().getByRole('region', { name: /kill evidence/i });
  if (mode === 'provisional') {
    expect(evidence.textContent).toMatch(/provisional/i); expect(evidence.textContent).toMatch(/HP|zero|removed/i);
  } else {
    expect(evidence.textContent).toMatch(/no saved|unavailable|not saved/i);
    expect(evidence.textContent).not.toMatch(/confirmed kill|defeated/i);
  }
  expect(main().queryByRole('progressbar')).toBeNull();
});

it.each(['mismatch', 'missing', 'ambiguous'] as const)('fight identity guard prevents guessed detail (%s)', async mode => {
  const rows = mode === 'missing' ? [] : [player(), ...(mode === 'ambiguous' ? [player({ player_id: 'other', name: 'Other incarnation' })] : [])];
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: {
    getFight: async () => ready(fight({ run_id: mode === 'mismatch' ? 'wrong-run' : 'run-1', players: rows })),
  } }); await flush();
  expect(main().getAllByText(/unavailable|not found|not saved|ambiguous|reused object|does not belong/i).length).toBeGreaterThan(0);
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  expect(main().queryByRole('link', { name: /Invented Archer/ })).toBeNull();
});

it('saved chart preserves duration, null gaps, exact player identity and the 120 interval cap', async () => {
  const buckets = [
    { from: '9007199254740993000', until: '9007199254741993000', total: 100, local: 0 },
    { from: '9007199254741993000', until: '9007199254743993000', total: 600, local: null },
    { from: '9007199254743993000', until: '9007199254744993000', total: 100, local: 10 },
  ];
  const series = { player_id: 'chosen', object_id: -7, is_local: true, points: [20, 80, 30] };
  const view = render(DamageChart, { buckets, playerSeries: [series], selectedPlayerId: 'chosen' }); await flush();
  await fireEvent.click(view.getByText(/Intervals/)); await flush();
  await fireEvent.change(view.getByLabelText('Interval'), { target: { value: '1' } }); await flush();
  expect(view.getByText(/Party 300 DPS.*Local \u2014 DPS.*Player 40 DPS/)).toBeTruthy();
  expect(view.container.querySelector('path.local')?.getAttribute('d')?.match(/M/g)).toHaveLength(2);
  for (const playerSeries of [null, [], [series, series], [{ ...series, points: [1] }], [{ ...series, player_id: 'wrong' }]]) {
    await view.rerender({ buckets, playerSeries, selectedPlayerId: 'chosen' }); await flush();
    expect(view.container.querySelector('path.player')?.getAttribute('d')).toBe('');
    expect(view.getAllByText(/series unavailable/i).length).toBeGreaterThan(0);
  }
  await view.rerender({ buckets: Array.from({ length: 250 }, (_, i) => ({
    from: String(i * 1000000), until: String((i + 1) * 1000000), total: i, local: null,
  })), playerSeries: null, selectedPlayerId: null }); await flush();
  expect(view.getByLabelText('Interval').querySelectorAll('option')).toHaveLength(120);
  expect(view.getByText(/Sampled to 120 intervals; omitted intervals remain gaps/)).toBeTruthy();
});

it('phase2_player_coherent_revision_and_full_table_focus', async () => {
  const players = Array.from({ length: 12 }, (_, i) => player({ object_id: i, player_id: `player-${i}`,
    name: `Invented player ${i}`, rank: i + 21, share: 0.0125, dps: 321, is_local: i === 7 }));
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getFight: async () => ready(fight({ players })), getPlayerBreakdown: async (_, objectId) => ready(players[objectId]),
  } }); await flush();
  const links = region('Players').getAllByRole('link');
  expect(links).toHaveLength(12);
  expect(region('Players').queryByRole('button', { name: 'Next Players' })).toBeNull();
  for (const link of links) {
    const objectId = Number(link.getAttribute('href')!.split('/').at(-1));
    expect(link.getAttribute('aria-label')).toBe(`${objectId + 21} Invented player ${objectId}`);
    expect(within(link.closest('tr')!).getByLabelText('Share').textContent).toMatch(/^1\.3\s*%$/);
    expect(within(link.closest('tr')!).getByLabelText('Active DPS').textContent).toBe('321');
  }
  const selected = links.at(-1)!; selected.focus();
  const href = selected.getAttribute('href')!; await follow(href, region('Players')); await click('Back', history());
  expect(document.activeElement?.getAttribute('href')).toBe(href);
  await follow(href, region('Players'));
  const oldText = main().getByRole('region', { name: 'Damage by item' }).textContent;
  page.queries.getPlayerBreakdown.mockResolvedValue(ready(player({ player_id: 'wrong-revision', object_id: 11, sources: [source({ damage: 999999 })] })));
  page.event({ fight_ended: fight({ players }) }, '13'); page.setStore(saved('13', 'live-phase2')); await advance();
  expect(main().getByRole('region', { name: 'Damage by item' }).textContent).toBe(oldText);
  expect(main().queryByText(/999[ ,]?999/)).toBeNull();
  expect(main().getAllByText(/stale|changed|coherent|mismatch|revision/i).length).toBeGreaterThan(0);
});

it('chart selection belongs to this fight and is independent of player navigation', async () => {
  const value = fight({ players: [player(), player({ player_id: 'second', object_id: 8, name: 'Second invented player', is_local: false })],
    buckets: [{ from: '3000000', until: '4000000', total: 100, local: 10 }],
    player_series: [{ player_id: 'second', object_id: 8, is_local: false, points: [73] }],
  });
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: { getFight: async () => ready(value) } }); await flush();
  const toggle = main().getByRole('button', { name: 'Second invented player' });
  expect(toggle.getAttribute('aria-pressed')).toBe('true');
  await fireEvent.click(toggle); await flush();
  expect(toggle.getAttribute('aria-pressed')).toBe('false');
  expect(page.browser.adapter.hash).toBe('#/runs/run-1/fights/fight-1');
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  const row = region('Players').getByRole('link', { name: /Second invented player/ });
  expect(within(row.closest('tr')!).getByLabelText('Share').textContent).toMatch(/23\s*%/);
});

it('chart toggles survive Back and Forward independently of row focus', async () => {
  const players = [player({ player_id: 'self', is_local: true }), player({ player_id: 'ally', object_id: 8, name: 'DemoAlly', is_local: false })];
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getFight: async () => ready(fight({ players, player_series: [{ player_id: 'ally', object_id: 8, is_local: false, points: [10, 20] }] })),
  } }); await flush();
  await click('Party', region('Damage over time'));
  expect(region('Damage over time').getByRole('button', { name: 'Party' }).getAttribute('aria-pressed')).toBe('false');
  await advance(250);
  expect(page.browser.adapter.state).toMatchObject({ fightPosition: { plotted: ['self'] } });
  await follow('#/logs', primary()); await click('Back', history());
  expect(region('Damage over time').getByRole('button', { name: 'Party' }).getAttribute('aria-pressed')).toBe('false');
  await click('Forward', history()); await click('Back', history());
  expect(region('Damage over time').getByRole('button', { name: 'Party' }).getAttribute('aria-pressed')).toBe('false');
});

const bossStrip = ['before', 'fight-1', 'after'].map((fight_id, index) => ({ fight_id,
  target_type: null, boss_type: 99, target_name: `Synthetic boss ${index + 1}`, started: '1000000', ended: '2000000',
  killed: true, player_count: 7, damage: index === 2 ? null : 100, rank: index === 2 ? null : 1, share: null }));
it('shows neighboring boss links, the current fight, and unknown strip metrics', async () => {
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getFight: async id => ready(fight({ id, boss_type: 99 })), getRunFightStrip: async () => ready(bossStrip),
  } }); await flush();
  expect(page.queries.getRunFightStrip).toHaveBeenCalledWith('run-1', null);
  expect(main().getByText(/^Boss · fight 2 of 3 · /)).toBeTruthy();
  expect(main().getByRole('link', { name: 'Previous boss: Synthetic boss 1' }).classList.contains('ctl')).toBe(true);
  expect(main().getByRole('link', { name: 'Previous boss: Synthetic boss 1' }).classList.contains('ghost')).toBe(true);
  expect(main().getByRole('link', { name: 'Previous boss: Synthetic boss 1' }).getAttribute('href')).toBe('#/runs/run-1/fights/before');
  expect(main().getByRole('link', { name: 'Next boss: Synthetic boss 3' }).getAttribute('href')).toBe('#/runs/run-1/fights/after');
  expect(region('Bosses this run').getByRole('link', { name: 'Synthetic boss 2' }).getAttribute('aria-current')).toBe('page');
  expect(region('Bosses this run').getByRole('row', { name: /Synthetic boss 3/ }).textContent).toContain('——');
  expect(page.queries.getRun).not.toHaveBeenCalled();
  await follow('#/runs/run-1/fights/after', within(main().getByRole('navigation', { name: 'Adjacent boss fights' })));
  const next = main().getByText('Next boss ›');
  expect(next.getAttribute('aria-disabled')).toBe('true'); expect(next.hasAttribute('href')).toBe(false);
  await follow('#/runs/run-1/fights/before', region('Bosses this run'));
  expect(main().getByText('‹ Previous boss').getAttribute('aria-disabled')).toBe('true');
});
it.each(['busy', 'error'] as const)('strip %s keeps the fight visible with unavailable run context', async mode => {
  mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getRunFightStrip: async () => { if (mode === 'error') throw new Error('Synthetic read error'); return { status: 'busy' }; },
  } }); await flush();
  expect(main().getByRole('table', { name: 'Fight Players' })).toBeTruthy();
  expect(region('Bosses this run').getByText('—')).toBeTruthy();
  expect(main().queryByText(/fight \d+ of/)).toBeNull();
});
it.each([
  ['Killed', { kill: { at: '7000000', evidence: 'hp_zero_removed' as const, provisional: false } }],
  ['Killed', { kill: null, ended: '7000000', end_reason: 'killed' as const }],
  ['Ended', { kill: null, ended: '7000000', end_reason: 'map_changed' as const }],
  ['In progress', { kill: null, ended: null, end_reason: null }],
] as const)('shows the %s chip from saved evidence', async (label, overrides) => {
  const page = mountPhase2({ hash: '#/fights/fight-1', queries: { getFight: async () => ready(fight({ ...overrides, run_id: null })) } });
  await flush();
  expect(main().getByText(label, { selector: '.chip' })).toBeTruthy();
  expect(page.queries.getRunFightStrip).not.toHaveBeenCalled();
});
it('reused object IDs remain unlinked with debug evidence', async () => {
  mountPhase2({ hash: '#/fights/fight-1', queries: { getFight: async () => ready(fight({ run_id: null,
    players: [player(), player({ player_id: 'reused', name: 'DemoReused' })],
  })) } }); await flush();
  expect(region('Players').queryAllByRole('link')).toHaveLength(0);
  expect(region('Players').getAllByText(/navigation is ambiguous/)).toHaveLength(2);
  expect(region('Your damage sources').queryByRole('link')).toBeNull();
});

it('loads the strip only after valid fight context resolves', async () => {
  const pending = deferred<HistoryReply<FightView | null>>();
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getFight: () => pending.promise, getRunFightStrip: async () => ready(bossStrip),
  } }); await flush();
  expect(page.queries.getFight).toHaveBeenCalledTimes(1);
  expect(page.queries.getRunFightStrip).not.toHaveBeenCalled();
  pending.resolve(ready(fight())); await flush();
  expect(page.queries.getRunFightStrip).toHaveBeenCalledTimes(1);
  expect(region('Bosses this run').getByRole('link', { name: 'Synthetic boss 2' })).toBeTruthy();
});
it.each(['fight', 'strip'] as const)('retires the pending %s stage when leaving the route', async stage => {
  const context = deferred<HistoryReply<FightView | null>>();
  const strip = deferred<HistoryReply<RunFightStripRow[]>>();
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getFight: () => context.promise, getRunFightStrip: () => strip.promise,
  } }); await flush();
  if (stage === 'strip') { context.resolve(ready(fight())); await flush(); }
  await follow('#/logs', primary());
  context.resolve(ready(fight())); strip.resolve(ready(bossStrip)); await flush();
  expect(page.queries.getRunFightStrip).toHaveBeenCalledTimes(stage === 'strip' ? 1 : 0);
  expect(page.browser.adapter.hash).toBe('#/logs');
  expect(main().queryByRole('region', { name: 'Bosses this run' })).toBeNull();
});
it.each(['busy', 'superseded', 'missing', 'mismatch'] as const)('skips the strip for %s fight context', async mode => {
  const page = mountPhase2({ hash: '#/runs/run-1/fights/fight-1', queries: {
    getFight: async () => mode === 'busy' || mode === 'superseded' ? { status: mode }
      : ready(mode === 'missing' ? null : fight({ run_id: 'another-run' })),
  } }); await flush();
  expect(page.queries.getRunFightStrip).not.toHaveBeenCalled();
});


it.each([null, '7000000'])('provisional kill evidence alone does not confirm a kill (ended=%s)', async ended => {
  mountPhase2({ hash: '#/fights/fight-1', queries: { getFight: async () => ready(fight({ run_id: null,
    ended, end_reason: ended === null ? null : 'map_changed', complete_capture: true, issues: [],
    kill: { at: '7000000', evidence: 'hp_zero_removed', provisional: true },
  })) } }); await flush();
  expect(main().queryByText('Killed', { selector: '.chip' })).toBeNull();
  expect(main().getByText(ended === null ? 'In progress' : 'Ended', { selector: '.chip' })).toBeTruthy();
  expect(main().getByText('partial', { selector: '.tag .badge' })).toBeTruthy();
});

it('uses numeric-hour formatting for the fight facts', async () => {
  const at = String(BigInt(new Date(2026, 9, 2, 7, 35, 0).getTime()) * 1000n);
  mountPhase2({ hash: '#/fights/fight-1', queries: { getFight: async () => ready(fight({ run_id: null, started: at, last_hit: at })) } });
  await flush();
  const expected = new Date(Number(BigInt(at) / 1000n)).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit', second: '2-digit' });
  const facts = main().getByRole('region', { name: 'Fight facts' });
  for (const label of ['Started', 'Last hit']) {
    expect(within(facts).getByText(label).nextElementSibling?.textContent).toBe(expected);
  }
});

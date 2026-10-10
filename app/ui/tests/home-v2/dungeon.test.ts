import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import DamageChart from '../../src/lib/home/v2/DamageChart.svelte';
import type { RunTotals } from '../../src/lib/bindings/RunTotals';
import type { DamageChartProps } from '../../src/lib/home/v2/contracts';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { catalog, fight, flushHomeRender, homeSnapshot, mountV2 } from './fixtures';

afterEach(() => vi.restoreAllMocks());

it('dungeon_tabs_zero_history_and_series_alignment', async () => {
  const snapshot = homeSnapshot();
  snapshot.fights = [fight('current', { target_type: 301 }), fight('previous', { target_type: 302, ended: '4500000', player_series: null })];
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'Current sentinel'), catalog(302, 'Previous sentinel')] });
  await flushHomeRender();
  const main = within(screen.getByRole('main'));
  expect(main.getByRole('tablist', { name: 'Encounters' })).toBeTruthy();
  const selected = main.getByRole('tab', { selected: true });
  expect(selected.textContent).toContain('Current sentinel');
  await fireEvent.click(main.getByRole('tab', { name: /Previous sentinel/ }));
  expect(main.getByRole('tab', { selected: true }).textContent).toContain('Previous sentinel');
  // A newer live fight cannot silently replace an explicit stable-ID selection.
  page.publish({ ...snapshot, fights: [fight('newest', { target_type: 303, last_hit: '7000000' }), ...snapshot.fights] });
  await flushHomeRender();
  expect(main.getByRole('tab', { selected: true }).textContent).toContain('Previous sentinel');
  await fireEvent.click(main.getByRole('tab', { name: /Whole run/i }));
  const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
  await fireEvent.click(screen.getByRole('link', { name: 'Settings' })); await flushHomeRender();
  await fireEvent.click(nav.getByRole('link', { name: 'Home' })); await flushHomeRender();
  for (const read of page.history) expect(read).not.toHaveBeenCalled();
  page.unmount();

  // Different durations and nulls make an index/duration mix-up observable.
  const buckets = [{ from: '0', until: '2000000', total: 200, local: 40 },
    { from: '2000000', until: '2500000', total: 150, local: null }];
  const input: DamageChartProps = { buckets, selectedPlayerId: 'local', playerSeries: [
    { player_id: 'local', object_id: 1, is_local: true, points: [40, 0] },
    { player_id: 'other', object_id: 2, is_local: false, points: [100, 25] },
  ] };
  const chart = render(DamageChart, input);
  chart.container.querySelector('details')!.open = true;
  expect(chart.container.textContent?.replace(/\s+/g, ' ')).toContain('2s · Party 100 DPS · Local 20 DPS · Player 20 DPS');
  await fireEvent.change(screen.getByLabelText('Interval'), { target: { value: '1' } });
  expect(chart.container.textContent?.replace(/\s+/g, ' ')).toContain('0.5s · Party 300 DPS · Local — DPS · Player 0 DPS');
  await chart.rerender({ buckets, selectedPlayerId: 'local', playerSeries: null });
  expect(chart.container.textContent).not.toMatch(/player.*unavailable|series.*unavailable|series.*omitted/i);
  expect(chart.container.textContent?.replace(/\s+/g, ' ')).toContain('Party 300 DPS · Local — DPS · Player — DPS');
  expect(chart.container.querySelector('path.player')?.getAttribute('d')).toBe('');
  await chart.rerender({ buckets, selectedPlayerId: 'local', playerSeries: [
    { player_id: 'local', object_id: 1, is_local: true, points: [777] },
  ] });
  expect(chart.container.textContent).not.toContain('777');
  expect(chart.container.textContent).not.toMatch(/unavailable|align|length/i);
});

it('whole_run_uses_frozen_totals_and_full_set_rank', async () => {
  const snapshot = homeSnapshot();
  const players = Array.from({ length: 8 }, (_, index) => ({ player_id: index === 7 ? snapshot.character!.id : `p-${index}`,
    name: index === 7 ? 'Synthetic self' : `Synthetic player ${index}`, class_id: 100, is_local: index === 7,
    damage: 800 - index * 10, hits: 9, deaths: 0, dps: 187.5, share: 0.031, rank: index === 7 ? 80 : index + 1 }));
  const totals: RunTotals = { fights: 42, kills: 5, first_hit: '1000000', last_hit: '9000000', total_damage: 98765,
    unattributed_damage: 123, players, local: players[7], fame_delta: 0, partial: true };
  snapshot.run_totals = totals;
  const page = render(DungeonHome, { snapshot, stale: false });
  await fireEvent.click(screen.getByRole('tab', { name: /Whole run/i }));
  expect(page.container.textContent).toContain('98,765');
  const rows = page.container.querySelectorAll('tbody .damage-row');
  expect(rows).toHaveLength(8);
  expect(Array.from(rows, row => row.textContent).at(-1)).toContain('Synthetic self');
  const self = page.container.querySelector('tbody .self')!;
  expect(self.textContent).toMatch(/80/);
  expect(self.textContent).toMatch(/3\.1%/);
  expect(self.textContent).toMatch(/187[.,]5|188/);
  expect(page.container.querySelector('.fight-detail .scope')?.textContent).toContain('42 fights · 5 kills');
  expect(page.container.textContent).not.toMatch(/partial/i);
  expect(screen.queryByRole('meter', { name: 'Target HP' })).toBeNull();
  expect(screen.queryByRole('img', { name: /Fight DPS/ })).toBeNull();
  expect(page.container.textContent).not.toMatch(/biggest hit|best hit/i);
  await page.rerender({ snapshot: { ...snapshot, run_totals: null }, stale: false });
  expect(page.container.textContent).toContain('No Totals');
  expect(page.container.textContent).not.toContain('98,765');
});

it('dungeon_observed_hp_events_and_null_fame', async () => {
  const snapshot = homeSnapshot();
  snapshot.fights = [fight('current', { total_damage: 999999, target_hp: 240, max_hp: 1000 })];
  snapshot.run_totals = null;
  const page = mountV2(snapshot);
  await flushHomeRender();
  const departure = { id: 'left-once', session_id: snapshot.session!.id, run_id: snapshot.run!.id, at: '6000000',
    kind: { type: 'player_left' as const, object_id: 99, player_name: 'Synthetic leaver' }, provisional: true };
  page.send({ instance_event: departure });
  page.send({ instance_event: departure });
  page.send({ instance_event: { ...departure, id: 'foreign-left', run_id: 'other-run' } });
  page.send({ instance_event: { ...departure, id: 'local-death', kind: { type: 'player_death', player_name: 'Synthetic self', is_local: true } } });
  page.publish({ ...snapshot, run: { ...snapshot.run!, died_at: '6000000' } });
  await flushHomeRender();
  const main = screen.getByRole('main');
  expect(main.textContent).toMatch(/240\s*\/\s*1,?000|24%/);
  const metric = (key: string) => main.querySelector(`[data-kpi="${key}"] dd .value`)!;
  expect(metric('left').textContent?.trim()).toBe('1');
  expect(metric('deaths').textContent?.trim()).toBe('1');
  expect(metric('fame').textContent?.trim()).toBe('—');
  expect(main.textContent).not.toMatch(/in.view|daily ordinal|Pace/);
  page.publish({ ...snapshot, fights: [fight('current', { target_hp: null, max_hp: null })] });
  await flushHomeRender();
  expect(main.querySelector('.boss-hp')?.textContent).toContain('\u2014');
  expect(main.querySelector('.boss-hp')?.textContent).not.toMatch(/unavailable|observed/i);
});

it('dungeon_loot_window_preserves_unknown_contents', async () => {
  const snapshot = homeSnapshot();
  snapshot.drops = Array.from({ length: 7 }, (_, index) => ({ ...snapshot.drops[0], id: `bag-${index}`,
    bag_type: 700 + index, at: String(3000000 + index), bag_rank: index, contents_known: false, items: [], contents: [] }));
  snapshot.drops.push({ ...snapshot.drops[0], id: 'foreign', run_id: 'other-run', bag_type: 999 });
  const visuals = { ...emptyHomeVisuals(), phase: 'ready' as const, objects: [
    ...snapshot.drops.map(drop => catalog(drop.bag_type, drop.id === 'foreign' ? 'Foreign bag' : `Synthetic bag ${drop.id}`)),
  ] };
  const page = render(DungeonHome, { snapshot, stale: false, visuals });
  expect(page.container.textContent).not.toContain('Foreign bag');
  expect(page.container.querySelector('.area-loot')?.textContent).not.toMatch(/contents.*unknown|contents.*unseen|contents.*unavailable|not observed/i);
  const loot = screen.getByRole('region', { name: 'LOOT THIS RUN' });
  expect(loot.querySelectorAll('[data-drop-id]').length).toBeLessThanOrEqual(7);
  expect(loot.textContent).toContain('+6 more');
  expect(loot.querySelectorAll('[data-overflow-bag]')).toHaveLength(6);
});

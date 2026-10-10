import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import FightSeriesChart from '../../src/lib/runs/FightSeriesChart.svelte';
import { defaultFightPlotted, seriesForChart } from '../../src/lib/runs/fight-screen';
import { fight, player } from './fixtures';

afterEach(cleanup);
const sample = (count = 7) => {
  const players = Array.from({ length: count }, (_, index) => player({ player_id: `p-${index}`, object_id: index,
    name: index === 0 ? 'TestMage' : `DemoAlly${index}`, is_local: index === 0, rank: index + 1, damage: 1000 - index }));
  return fight({ players: [...players].reverse(), buckets: [{ from: '0', until: '1000000', total: 5000, local: 1000 }],
    player_series: players.map(row => ({ player_id: row.player_id, object_id: row.object_id, is_local: row.is_local, points: [row.damage] })) });
};
it('orders chips by self, Party and saved rank, and plots self plus the top two other damage dealers', () => {
  const value = sample();
  value.players.find(row => row.player_id === 'p-4')!.damage = 5000;
  expect(defaultFightPlotted(value)).toEqual(['p-0', 'party', 'p-4', 'p-1']);
  expect(seriesForChart(value, ['party', ...value.players.map(row => row.player_id)]).map(row => row.label))
    .toEqual(['TestMage', 'Party', ...Array.from({ length: 6 }, (_, index) => `DemoAlly${index + 1}`)]);
});
it.each([7, 85])('collapses %s players to four series chips and reveals the rest with a reversible disclosure', async count => {
  const value = sample(count), onToggle = vi.fn();
  const view = render(FightSeriesChart, { fight: value, plotted: defaultFightPlotted(value), onToggle });
  const chips = () => view.getAllByRole('button').filter(button => button.hasAttribute('aria-pressed'));
  expect(chips().map(button => button.textContent?.trim())).toEqual(['TestMage', 'Party', 'DemoAlly1', 'DemoAlly2']);
  expect(chips().every(button => button.getAttribute('aria-pressed') === 'true')).toBe(true);
  const more = view.getByRole('button', { name: `+${count - 3}` });
  expect(more.getAttribute('aria-expanded')).toBe('false');
  expect(view.queryByRole('button', { name: 'DemoAlly3' })).toBeNull();
  more.focus();
  await userEvent.setup().keyboard('{Enter}');
  expect(more.getAttribute('aria-expanded')).toBe('true');
  expect(chips()).toHaveLength(count + 1);
  const hidden = view.getByRole('button', { name: 'DemoAlly3' });
  expect(hidden.getAttribute('aria-pressed')).toBe('false');
  await fireEvent.click(hidden);
  expect(onToggle).toHaveBeenCalledWith('p-3');
  await view.rerender({ fight: value, plotted: ['p-3'], onToggle });
  expect(view.getByRole('button', { name: 'DemoAlly3' }).getAttribute('aria-pressed')).toBe('true');
  expect(view.getByRole('button', { name: 'TestMage' }).getAttribute('aria-pressed')).toBe('false');
  hidden.focus(); await fireEvent.keyDown(window, { key: 'Escape' });
  expect(more.getAttribute('aria-expanded')).toBe('false');
  expect(view.queryByRole('button', { name: 'DemoAlly3' })).toBeNull();
  expect(document.activeElement).toBe(more);
  await fireEvent.click(more); await fireEvent.click(more);
  expect(more.getAttribute('aria-expanded')).toBe('false');
});
it('does not invent unavailable player lines or a local identity', () => {
  const value = sample();
  value.player_series = null;
  expect(defaultFightPlotted(value)).toEqual(['p-0', 'party']);
  value.players.forEach(row => { row.is_local = false; });
  expect(defaultFightPlotted(value)).toEqual(['party']);
});

it('exposes per-bucket plotted DPS and unknown gaps without debug mode', async () => {
  const value = sample();
  value.buckets = [
    { from: '0', until: '2000000', total: 1000, local: 400 },
    { from: '2000000', until: '3000000', total: 300, local: null },
  ];
  value.player_series = [{ player_id: 'p-1', object_id: 1, is_local: false, points: [246, 0] }];
  const props = { fight: value, plotted: ['party', 'p-0', 'p-1'], onToggle: vi.fn() };
  const view = render(FightSeriesChart, props);
  const table = () => within(view.getByRole('table', { name: 'Fight DPS by interval' }));
  expect(table().getByRole('columnheader', { name: 'DemoAlly1 DPS' })).toBeTruthy();
  const cells = (interval: RegExp) => within(table().getByRole('row', { name: interval }))
    .getAllByRole('cell').map(cell => cell.textContent);
  expect(cells(/1: 0:00 – 0:02/)).toEqual(['500', '123', '200']);
  expect(cells(/2: 0:02 – 0:03/)).toEqual(['300', '0', '—']);
  await view.rerender({ ...props, plotted: ['p-1'] });
  expect(table().queryByRole('columnheader', { name: 'Party DPS' })).toBeNull();
  expect(cells(/1: 0:00 – 0:02/)).toEqual(['123']);
});

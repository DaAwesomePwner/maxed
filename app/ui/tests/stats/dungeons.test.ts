import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import Stats from '../../src/routes/Stats.svelte';
import DungeonBreakdown from '../../src/lib/home/DungeonBreakdown.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import demoSnapshot from '../fixtures/t0-live';
import { statsQueries } from '../shell/fixtures/stats';

afterEach(cleanup);

it('shows lifetime dungeon records', async () => {
  const client = createClient(createFixtureTransport(demoSnapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, statsQueries));
  render(Stats, { route: { kind: 'stats', view: 'dungeons' }, client });
  const table = await screen.findByRole('table');
  const first = within(table).getAllByRole('row')[1];
  expect(within(first).getByRole('rowheader').textContent).toBe('Lost Halls');
  expect(within(first).getAllByRole('cell').map(c => c.textContent)).toEqual(
    expect.arrayContaining(['12', '3h 10m', '9', '75%', '2', '14:30', '11:00']));
  const second = within(table).getAllByRole('row')[2];
  expect(within(second).getAllByRole('cell').map(c => c.textContent)).toEqual(expect.arrayContaining(['—']));
});
it.each([[null, false], ['#/stats/dungeons', true]])('Home links to all-time records only with Stats on', (allTimeHref, shown) => {
  render(DungeonBreakdown, { rows: [], allTimeHref });
  const link = screen.queryByRole('link', { name: 'All time ›' });
  expect(link !== null).toBe(shown);
  if (shown) expect(link?.getAttribute('href')).toBe(allTimeHref);
});

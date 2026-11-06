import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import Stats from '../../src/routes/Stats.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import demoSnapshot from '../fixtures/t0-live';
import { statsQueries } from '../shell/fixtures/stats';

afterEach(cleanup);
const client = () => createClient(createFixtureTransport(demoSnapshot, undefined, undefined, undefined, undefined,
  undefined, undefined, {}, undefined, undefined, statsQueries));

it('ranks maps by fame per hour with dashes and missing baselines', async () => {
  render(Stats, { route: { kind: 'stats', view: 'maps' }, client: client() });
  await fireEvent.change(await screen.findByRole('spinbutton', { name: 'Minimum runs' }), { target: { value: '1' } });
  const table = await screen.findByRole('table', { name: 'Dungeon rankings' });
  const rows = within(table).getAllByRole('row').slice(1).map(r => within(r).getAllByRole('cell').map(c => c.textContent));
  expect(within(table).getAllByRole('rowheader').map(h => h.textContent)).toEqual(['Lost Halls', 'Pirate Cave']);
  expect(rows[0]).toEqual(['2', '38m', '≥ 900', '1,421', '1', '1']);
  expect(rows[1][3]).toBe('—');
  expect(screen.getByText(/Realm and hub visits are recorded from/)).toBeTruthy();
});
it('sorts by a column and records the sort in list state', async () => {
  const onStateChange = vi.fn();
  render(Stats, { route: { kind: 'stats', view: 'maps' }, client: client(), onStateChange });
  await fireEvent.change(await screen.findByRole('spinbutton', { name: 'Minimum runs' }), { target: { value: '1' } });
  await fireEvent.click(await screen.findByRole('button', { name: 'Visits' }));
  expect(onStateChange).toHaveBeenLastCalledWith({ statsPosition: expect.objectContaining({ mapSort: { key: 'visits', descending: true } }) });
  expect(within(screen.getByRole('table', { name: 'Dungeon rankings' })).getAllByRole('rowheader')[0].textContent).toBe('Lost Halls');
});
it('passes the character filter to the read', async () => {
  const c = client(); const read = vi.spyOn(c, 'getStatsMaps');
  render(Stats, { route: { kind: 'stats', view: 'maps' }, client: c });
  // A select's accessible name includes its selected option, so match the label prefix.
  await fireEvent.change(await screen.findByRole('combobox', { name: /^Character/ }), { target: { value: 'preview-alpha' } });
  // Non-Runs history reads are spaced at least one second apart.
  await vi.waitFor(() => expect(read).toHaveBeenLastCalledWith({ range: null, character_id: 'preview-alpha' }), { timeout: 3000 });
});
it('shows an export failure inline', async () => {
  render(Stats, { route: { kind: 'stats', view: 'maps' }, client: client() });
  await fireEvent.click(await screen.findByRole('button', { name: 'Export CSV' }));
  expect((await screen.findByRole('alert')).textContent).toBe('Preview Only');
});

it('defaults to five runs, keeps hubs separate and persists changes to the minimum', async () => {
  const onStateChange = vi.fn();
  render(Stats, { route: { kind: 'stats', view: 'maps' }, client: client(), onStateChange });
  const minimum = await screen.findByRole('spinbutton', { name: 'Minimum runs' });
  expect((minimum as HTMLInputElement).value).toBe('5');
  expect(screen.queryByRole('table', { name: 'Dungeon rankings' })).toBeNull();
  expect(within(screen.getByRole('table', { name: 'Other map statistics' })).getByText('Hubs')).toBeTruthy();
  await fireEvent.change(minimum, { target: { value: '2' } });
  const table = await screen.findByRole('table', { name: 'Dungeon rankings' });
  expect(within(table).getAllByRole('rowheader').map(row => row.textContent)).toEqual(['Lost Halls']);
  expect(onStateChange).toHaveBeenLastCalledWith({ statsPosition: expect.objectContaining({ minMapRuns: 2 }) });
});

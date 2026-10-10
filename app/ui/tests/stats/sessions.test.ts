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

it('lists sessions newest first with the session in progress marked', async () => {
  render(Stats, { route: { kind: 'stats', view: 'sessions' }, client: client() });
  const table = await screen.findByRole('table');
  const rows = within(table).getAllByRole('row').slice(1);
  expect(rows).toHaveLength(2);
  expect(within(rows[0]).getByText('In progress')).toBeTruthy();
  expect(within(rows[1]).getByText('≥ 1,200')).toBeTruthy();
  const link = within(rows[1]).getByRole('link');
  expect(link.getAttribute('href')).toBe('#/stats/sessions/preview-session-1');
});
it('asks for empty sessions when the toggle is on and resets paging', async () => {
  const c = client(); const read = vi.spyOn(c, 'listStatsSessions'); const onStateChange = vi.fn();
  render(Stats, { route: { kind: 'stats', view: 'sessions' }, client: c, onStateChange });
  await fireEvent.click(await screen.findByRole('checkbox', { name: 'Show empty sessions' }));
  await vi.waitFor(() => expect(read).toHaveBeenLastCalledWith(expect.objectContaining({ include_empty: true, cursor: null })),
    { timeout: 3000 });
  expect(onStateChange).toHaveBeenLastCalledWith({ statsPosition: expect.objectContaining({
    includeEmpty: true, sessions: { cursor: null, previous: [] } }) });
});
it('shows a session with its maps, unrecorded fame and runs', async () => {
  render(Stats, { route: { kind: 'stats', view: 'sessions', sessionId: 'preview-session-1' }, client: client() });
  const unrecorded = await screen.findByRole('rowheader', { name: 'Unrecorded' });
  expect(unrecorded.closest('tr')!.textContent).toContain('120');
  expect(screen.getAllByRole('link').some(a => a.getAttribute('href') === '#/runs/preview-run')).toBe(true);
  expect(screen.getByRole('button', { name: 'Export runs with party' })).toBeTruthy();
  // Ended sessions have no Home link; Home shows only the current session.
  expect(screen.queryByRole('link', { name: 'Open in Home' })).toBeNull();
});
it('links the session in progress to Home', async () => {
  render(Stats, { route: { kind: 'stats', view: 'sessions', sessionId: 'preview-session-2' }, client: client() });
  expect((await screen.findByRole('link', { name: 'Open in Home' })).getAttribute('href')).toBe('#/');
});
it('reports a missing session', async () => {
  render(Stats, { route: { kind: 'stats', view: 'sessions', sessionId: 'absent' }, client: client() });
  expect(await screen.findByRole('heading', { name: 'Session not found' })).toBeTruthy();
});

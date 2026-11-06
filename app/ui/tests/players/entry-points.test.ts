import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import RealmHome from '../../src/lib/home/RealmHome.svelte';
import { routeHref } from '../../src/lib/navigation/routes';
import { homeSnapshot } from '../home/fixtures';
import { realmSnapshot } from '../home-v2/fixtures';
import { fight, flush, follow, main, mountRuns, player, ready, run, story } from '../runs/fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('dungeon meter links to its fight or whole run as the tab changes', async () => {
  const snapshot = homeSnapshot();
  const row = snapshot.fights[0].players[0];
  snapshot.run_totals = { fights: 1, kills: 0, players: [row], total_damage: row.damage,
    unattributed_damage: 0, partial: false };
  render(DungeonHome, { snapshot, stale: false });
  const meter = within(screen.getByRole('table', { name: 'Player Damage' }));
  expect(meter.getByRole('link').getAttribute('href')).toBe(routeHref({ kind: 'player',
    fightId: snapshot.fights[0].id, objectId: row.object_id, runId: snapshot.run!.id }));
  await fireEvent.click(screen.getByRole('tab', { name: 'Whole run' }));
  expect(meter.getByRole('link').getAttribute('href')).toBe(routeHref({ kind: 'run-player',
    runId: snapshot.run!.id, playerId: row.player_id }));
});

it('realm meter links a unique player without run context', () => {
  const snapshot = realmSnapshot();
  snapshot.fights = [fight({ run_id: null, session_id: snapshot.session!.id, ended: null,
    started: snapshot.at, last_hit: snapshot.at, local_last_hit: snapshot.at })];
  render(RealmHome, { snapshot });
  expect(within(screen.getByRole('table', { name: 'Player Damage' })).getByRole('link').getAttribute('href'))
    .toBe('#/fights/fight-1/players/7');
});

it('run story whole-run rows open the run player page and preserve Runs navigation', async () => {
  const row = player({ player_id: 'test:player:3:42:0' });
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({
    totals: { fights: 1, kills: 0, players: [row], total_damage: row.damage, unattributed_damage: 0, partial: false },
  })), getRunPlayer: async (runId, playerId) => ready({ run: run({ id: runId }), scope_id: runId, player_id: playerId,
    player: row, party: { total_damage: row.damage, average_dps: row.dps, top_name: row.name, top_dps: row.dps, players_with_damage: 1 },
    sources: row.sources, fights: [], profile: null, gear_log: [], timeline: null, deaths: [], roster: [row], partial: false,
  }) } });
  await flush();
  await follow('#/runs/run-1/players/test%3Aplayer%3A3%3A42%3A0', within(main().getByRole('table', { name: 'Player Damage' })));
  expect(page.browser.adapter.hash).toBe('#/runs/run-1/players/test%3Aplayer%3A3%3A42%3A0');
  expect(within(main().getByRole('navigation', { name: 'Breadcrumb' })).getAllByRole('listitem').map(item => item.textContent?.replace('/', '').trim()))
    .toEqual(['Runs', 'Run', row.name]);
  expect(page.queries.getRunPlayer).toHaveBeenCalledWith('run-1', row.player_id);
  expect(main().getByRole('table', { name: 'Damage by item' })).toBeTruthy();
  expect(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Runs' }).getAttribute('aria-current')).toBe('page');
  expect(main().getByRole('heading', { level: 1, name: 'Player' })).toBeTruthy();
});

it('run story boss rows open the fight player with run context', async () => {
  mountRuns({ hash: '#/runs/run-1' });
  await flush();
  await fireEvent.click(main().getAllByRole('tab')[1]);
  await flush();
  expect(within(main().getByRole('table', { name: 'Player Damage' })).getByRole('link').getAttribute('href'))
    .toBe('#/runs/run-1/fights/fight-1/players/7');
});

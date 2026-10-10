import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import FightPlayers from '../../src/lib/runs/FightPlayers.svelte';
import { limitRows, renderCount, RENDER_STEP } from '../../src/lib/runs/render-limit';
import { fight, fightSummary, flush, main, mountRuns, player, ready, story } from './fixtures';

afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

const players = (n: number, local: number) => Array.from({ length: n }, (_, i) => player({ object_id: i + 1,
  player_id: `player-${i}`, name: `Invented ${i}`, is_local: i === local, rank: i + 1, damage: n - i }));

it('caps rows per step and keeps pinned rows', () => {
  const rows = Array.from({ length: 450 }, (_, i) => i);
  expect(limitRows(rows, RENDER_STEP)).toHaveLength(RENDER_STEP);
  expect(limitRows(rows, RENDER_STEP, row => row === 420).at(-1)).toBe(420);
  expect(limitRows(rows.slice(0, 10), RENDER_STEP)).toHaveLength(10);
  expect(renderCount({ key: 'a', count: 400 }, 'a')).toBe(400);
  expect(renderCount({ key: 'a', count: 400 }, 'b')).toBe(RENDER_STEP);
});

it('mounts a huge saved fight one step at a time, keeping the local player visible', async () => {
  render(FightPlayers, { fight: fight({ players: players(450, 420) }) });
  const table = () => within(screen.getByRole('table', { name: 'Fight Players' }));
  // 200 rows, the pinned local row, and the Other damage row.
  expect(table().getAllByRole('row')).toHaveLength(1 + RENDER_STEP + 1 + 1);
  expect(table().getByText('Invented 420')).toBeTruthy();
  expect(screen.getByText('249 more players not shown')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
  expect(table().getAllByRole('row')).toHaveLength(1 + 400 + 1 + 1);
  expect(screen.getByText('49 more players not shown')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
  expect(table().getAllByRole('row')).toHaveLength(1 + 450 + 1);
  expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull();
});

it('leaves realistic fights uncapped with no Show more control', () => {
  render(FightPlayers, { fight: fight({ players: players(85, 3) }) });
  expect(within(screen.getByRole('table', { name: 'Fight Players' })).getAllByRole('row')).toHaveLength(1 + 85 + 1);
  expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull();
});

it('caps the saved run other-targets table and the boss damage table', async () => {
  vi.useFakeTimers();
  const others = Array.from({ length: 250 }, (_, i) => fightSummary(i + 2));
  const value = story({ boss_fights: [fight({ target_kind: 'boss', players: players(300, 250) })],
    fights: [fightSummary(1, { target_kind: 'boss' }), ...others] });
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  await fireEvent.click(main().getByRole('tab', { name: /250 other targets/ })); await flush();
  const targets = () => within(main().getByRole('table', { name: 'Other targets' })).getAllByRole('row');
  expect(targets()).toHaveLength(1 + RENDER_STEP);
  expect(main().getByText('50 more targets not shown')).toBeTruthy();
  await fireEvent.click(main().getByRole('button', { name: 'Show more' })); await flush();
  expect(targets()).toHaveLength(1 + 250);
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush();
  const damage = within(main().getByRole('table', { name: 'Player Damage' }));
  expect(damage.getAllByRole('row')).toHaveLength(1 + RENDER_STEP + 1 + 1);
  expect(main().getByText('99 more players not shown')).toBeTruthy();
});

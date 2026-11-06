import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import type { RunPlayerView } from '../../src/lib/bindings/RunPlayerView';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createRunHistory, historyKey, type HistoryInput, type HistoryState } from '../../src/lib/runs/history';
import { homeSnapshot, saved } from '../home/fixtures';
import { fight, player, ready, run, runPage, story } from '../runs/fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

it('invokes player commands with decimal string and nullable time arguments', async () => {
  const transport = createFixtureTransport(homeSnapshot());
  const invoke = vi.spyOn(transport, 'invoke').mockResolvedValue(ready(null));
  const client = createClient(transport);
  await client.getRunPlayer('run-1', 'test:player:3:42:0');
  expect(invoke).toHaveBeenLastCalledWith('get_run_player', { runId: 'run-1', playerId: 'test:player:3:42:0' });
  await client.getPlayerContext('scope-1', 'player-1', '9007199254740993', null);
  expect(invoke).toHaveBeenLastCalledWith('get_player_context', {
    scopeId: 'scope-1', playerId: 'player-1', from: '9007199254740993', until: null,
  });
  await client.getPlayerContext('scope-1', 'player-1', null, '9007199254740994');
  expect(invoke).toHaveBeenLastCalledWith('get_player_context', {
    scopeId: 'scope-1', playerId: 'player-1', from: null, until: '9007199254740994',
  });
});

it('provides explicit preview defaults for the player queries', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  await expect(client.getRunPlayer('run-1', 'player-1')).rejects.toThrow('Preview Only');
  await expect(client.getPlayerContext('scope-1', 'player-1', null, null)).resolves.toEqual(ready(null));
});

it('reads run players through history and keys both ids', async () => {
  const view: RunPlayerView = { run: run(), scope_id: 'run-1', player_id: 'player-1', player: player(),
    party: { total_damage: 321, players_with_damage: 1 }, sources: [], fights: [], gear_log: [], deaths: [], roster: [], partial: false };
  const context = { gear_log: [], deaths: [] };
  const getRunPlayer = vi.fn(async () => ready(view));
  const getPlayerContext = vi.fn(async () => ready(context));
  const client = createClient(createFixtureTransport(homeSnapshot(), undefined, undefined, {
    listRuns: async () => ready(runPage()), getRun: async () => ready(story()),
    getFight: async () => ready(fight()), getPlayerBreakdown: async () => ready(player()),
    getRunPlayer, getPlayerContext,
  }));
  const states: HistoryState[] = [];
  const controller = createRunHistory(client, state => states.push(state));
  const input: HistoryInput = { query: { kind: 'run-player', runId: 'run-1', playerId: 'player-1' },
    scopeKey: 'live', streamId: homeSnapshot().stream_id, required: '12', store: saved('12') };
  try {
    controller.update(input);
    await vi.advanceTimersByTimeAsync(0);
    expect(getRunPlayer).toHaveBeenCalledWith('run-1', 'player-1');
    expect(states.at(-1)).toMatchObject({ phase: 'ready', value: { kind: 'run-player', player: view } });
    expect(historyKey(input)).not.toBe(historyKey({ ...input, query: { kind: 'run-player', runId: 'other', playerId: 'player-1' } }));
    expect(historyKey(input)).not.toBe(historyKey({ ...input, query: { kind: 'run-player', runId: 'run-1', playerId: 'other' } }));
    await expect(client.getPlayerContext('scope-1', 'player-1', '1', '2')).resolves.toEqual(ready(context));
    expect(getPlayerContext).toHaveBeenCalledWith('scope-1', 'player-1', '1', '2');
  } finally { controller.dispose(); }
});

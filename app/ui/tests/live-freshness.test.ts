import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { subscribeLive, type LiveState } from '../src/lib/bridge/live';
import { flush, homeSnapshot, homeSummary, saved } from './home/fixtures';

let dispose = () => {};
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});
afterEach(() => { dispose(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('does not mark ordinary snapshots or session heartbeats stale, but still gates run-ended events', async () => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const transport = createFixtureTransport(snapshot);
  const client = createClient(transport);
  let durable = 12;
  client.queryStore = vi.fn(async () => saved(String(durable), snapshot.stream_id));
  const states: LiveState[] = [];
  const received = vi.fn();
  dispose = subscribeLive(client, { snapshot: received, state: state => states.push(state) }).dispose;
  await flush();
  expect(received).toHaveBeenCalledWith(snapshot);
  expect(states.at(-1)).toMatchObject({ phase: 'live', historyStale: false });
  states.length = 0;
  for (let sequence = 13; sequence <= 36; sequence++) {
    durable = sequence - 1;
    transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: String(sequence), at: snapshot.at,
      event: { session_updated: { ...snapshot.session!, last_traffic: String(sequence * 250000) } } });
    transport.publishSnapshot({ ...snapshot, sequence: String(sequence) });
    await vi.advanceTimersByTimeAsync(250); await flush();
  }
  expect(received).toHaveBeenCalledTimes(25);
  expect(states.length).toBeGreaterThan(0);
  expect(states.every(state => !state.historyStale && !state.historyLagging)).toBe(true);
  transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '37', at: snapshot.at,
    event: { run_ended: homeSummary().recent_runs[0] } });
  await flush();
  expect(states.at(-1)?.historyStale).toBe(true);
  durable = 37;
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(states.at(-1)).toMatchObject({ phase: 'live', historyStale: false, historyLagging: false });
});

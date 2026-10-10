import { cleanup } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { homeSnapshot, saved } from '../home/fixtures';
import { mountHome, flushHomeRender } from '../home/shell-fixture';

afterEach(() => { cleanup(); vi.useRealTimers(); });

it.each(['run_updated', 'run_ended'] as const)('Shell refreshes PBs for a %s completion followed by a character update', async kind => {
  vi.useFakeTimers();
  const dungeon = homeSnapshot();
  const source = homeSnapshot({ area_kind: 'nexus', run: null });
  const app = mountHome(source);
  const bests = vi.spyOn(app.client, 'queryDungeonBests').mockResolvedValue({ status: 'ready', value: [] });
  let durable = source.sequence;
  app.client.queryStore = vi.fn(async () => saved(durable, source.stream_id));
  await flushHomeRender(); await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(1);
  const seq = BigInt(source.sequence);
  const ended = { ...dungeon.run!, ended: source.at, completed_at: null };
  app.transport.publishEngineEvent({ stream_id: source.stream_id, sequence: String(seq + 1n), at: source.at,
    event: { run_ended: ended } });
  durable = String(seq + 1n);
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(2);
  const completed = { ...ended, completed_at: source.at };
  const events = [
    { stream_id: source.stream_id, sequence: String(seq + 2n), at: source.at,
      event: kind === 'run_updated' ? { run_updated: completed } : { run_ended: completed } },
    { stream_id: source.stream_id, sequence: String(seq + 3n), at: source.at,
      event: { character_updated: source.character! } },
  ];
  const batch = { stream_id: source.stream_id, first_sequence: events[0].sequence,
    last_sequence: events[1].sequence, events };
  app.transport.publishEngineEvent(batch);
  await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(2); // Wait for the writer, even within a batch.
  durable = String(seq + 3n);
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(3);
  app.transport.publishEngineEvent(batch);
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(3);
});

it.each(['stream change', 'gap replay', 'snapshot overtaking'] as const)('PB completion refreshes exactly once after %s', async mode => {
  vi.useFakeTimers();
  const source = homeSnapshot();
  const app = mountHome(source);
  const bests = vi.spyOn(app.client, 'queryDungeonBests').mockResolvedValue({ status: 'ready', value: [] });
  let durable = saved(source.sequence, source.stream_id);
  app.client.queryStore = vi.fn(async () => durable);
  await flushHomeRender(); await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(1);
  const stream = mode === 'stream change' ? 'new-stream' : source.stream_id;
  const first = mode === 'stream change' ? 11n : BigInt(source.sequence) + 2n;
  const completed = { ...source.run!, ended: source.at, completed_at: source.at };
  const baseline = { ...source, stream_id: stream, sequence: String(first - 1n), run: null };
  let resolve!: (snapshot: typeof source) => void;
  app.client.querySnapshot = vi.fn(() => new Promise<typeof source>(done => { resolve = done; }));
  const events = [
    { stream_id: stream, sequence: String(first), at: source.at, event: { run_updated: completed } },
    { stream_id: stream, sequence: String(first + 1n), at: source.at, event: { character_updated: source.character! } },
  ];
  const batch = { stream_id: stream, first_sequence: events[0].sequence, last_sequence: events[1].sequence, events };
  if (mode === 'snapshot overtaking') {
    app.transport.publishSnapshot({ ...baseline, sequence: String(first + 1n), run: completed });
    await flushHomeRender();
  }
  app.transport.publishEngineEvent(batch);
  // Baseline updates and replay callbacks run in one task, before Home's effect.
  resolve(mode === 'gap replay' ? { ...baseline, sequence: String(first + 1n), run: completed } : baseline);
  await flushHomeRender();
  durable = saved(String(first + 1n), stream);
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(2);
  app.transport.publishEngineEvent(batch);
  app.transport.publishSnapshot({ ...baseline, sequence: String(first + 2n), run: completed });
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(bests).toHaveBeenCalledTimes(2);
});

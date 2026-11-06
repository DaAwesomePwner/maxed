import { expect, it, vi } from 'vitest';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import { createHomeLiveWindow } from '../../src/lib/home/v2/live';
import { RECOVERY_GRACE_MS } from '../../src/lib/home/mode';
import { homeSnapshot, realmSnapshot } from './fixtures';

function harness() {
  const source = homeSnapshot({ fights: [], drops: [] });
  const receive = vi.fn(), onWindow = vi.fn(), onEvent = vi.fn();
  const live = createHomeLiveWindow(receive, { onWindow, onEvent });
  live.snapshot(source);
  let sequence = BigInt(source.sequence);
  const send = (event: EngineEvent, at = source.at, stream_id = source.stream_id) => {
    const envelope: EventEnvelope = { stream_id, sequence: String(++sequence), at, event };
    live.event(envelope); return envelope;
  };
  receive.mockClear(); onWindow.mockClear(); onEvent.mockClear();
  return { source, live, receive, onWindow, onEvent, send };
}

it('the_first_snapshot_has_no_pending_projection_to_checkpoint', () => {
  const receive = vi.fn(), onWindow = vi.fn();
  const live = createHomeLiveWindow(receive, { onWindow, onEvent() {} });
  live.snapshot(homeSnapshot());
  expect(onWindow).toHaveBeenCalledOnce();
  expect(onWindow.mock.lastCall![0]).toBe(receive.mock.lastCall![0]);
  live.dispose();
});

it.each(['state', 'stale-snapshot', 'pre-entry-snapshot', 'capture'] as const)(
  'no_op_%s_leaves_pending_rows_until_batch_end', kind => {
    const h = harness(), drop = homeSnapshot().drops[0];
    const state = { phase: 'live' as const, message: '', historyStale: false, historyFailed: false, historyLagging: false };
    h.live.state(state);
    if (kind === 'pre-entry-snapshot') {
      h.send({ map_entered: { at: h.source.at, kind: 'dungeon', name: h.source.run!.dungeon } });
      h.send({ run_started: h.source.run! });
    }
    if (kind === 'capture') h.live.capture(h.source.capture);
    h.onWindow.mockClear(); h.receive.mockClear();
    h.live.batch(() => {
      h.send({ bag_dropped: drop });
      if (kind === 'state') h.live.state({ ...state, historyStale: true });
      if (kind === 'stale-snapshot') h.live.snapshot({ ...h.source, sequence: String(BigInt(h.source.sequence) - 1n) });
      if (kind === 'pre-entry-snapshot') h.live.snapshot(h.source);
      if (kind === 'capture') h.live.capture(h.source.capture);
      expect(h.onWindow).not.toHaveBeenCalled();
      expect(h.receive).not.toHaveBeenCalled();
    });
    expect(h.onWindow).toHaveBeenCalledOnce(); expect(h.receive).toHaveBeenCalledOnce();
    expect(h.onWindow.mock.lastCall![0].drops).toEqual([drop]);
    h.live.dispose();
  });

it.each(['Recovering', 'WaitingForGame'] as const)('unchanged_%s_capture_does_not_checkpoint_pending_selection', state => {
  vi.useFakeTimers();
  const h = harness(), capture = { ...h.source.capture, state };
  try {
    h.live.capture(capture); h.onWindow.mockClear(); h.receive.mockClear();
    h.live.batch(() => {
      h.live.select('synthetic-selection');
      h.live.capture(capture);
      expect(h.onWindow).not.toHaveBeenCalled();
    });
    expect(h.onWindow).toHaveBeenCalledOnce(); expect(h.receive).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(1);
  } finally { h.live.dispose(); vi.useRealTimers(); }
});

it('reuses_the_window_and_drop_array_for_sequence_only_updates_without_suppressing_events', () => {
  const h = harness(), drop = homeSnapshot().drops[0];
  h.send({ bag_dropped: drop });
  const before = h.receive.mock.lastCall![0];
  h.onWindow.mockClear(); h.onEvent.mockClear(); h.receive.mockClear();
  const envelopes: EventEnvelope[] = [];
  h.live.batch(() => {
    for (let i = 0; i < 8; i++) envelopes.push(h.send({ drop_updated: drop }));
  });
  expect(h.onEvent.mock.calls.map(([event]) => event)).toEqual(envelopes);
  expect(h.onWindow).toHaveBeenCalledOnce(); expect(h.receive).toHaveBeenCalledOnce();
  expect(h.receive.mock.lastCall![0]).toBe(before);
  expect(h.receive.mock.lastCall![0].drops).toBe(before.drops);
  h.send({ drop_updated: { ...drop, bag_rank: 255 } });
  expect(h.receive.mock.lastCall![0]).not.toBe(before);
  expect(h.receive.mock.lastCall![0].drops).not.toBe(before.drops);
  h.live.dispose();
});

it('orders_pre_boundary_event_and_post_boundary_callbacks_then_publishes_the_cached_final_window', () => {
  const h = harness(), drop = homeSnapshot().drops[0];
  h.live.batch(() => {
    h.send({ bag_dropped: drop });
    expect(h.onWindow).not.toHaveBeenCalled();
    h.send({ map_entered: { at: h.source.at, kind: 'nexus', name: 'Nexus' } });
    expect(h.receive).not.toHaveBeenCalled();
    expect(h.onWindow).toHaveBeenCalledTimes(2);
    expect(h.onWindow.mock.calls[0][0].runId).toBe(h.source.run!.id);
    expect(h.onWindow.mock.calls[0][0].drops).toEqual([drop]);
    expect(h.onWindow.mock.calls[1][0].runId).toBeNull();
    expect(h.onEvent.mock.invocationCallOrder[0]).toBeLessThan(h.onWindow.mock.invocationCallOrder[0]);
    expect(h.onWindow.mock.invocationCallOrder[0]).toBeLessThan(h.onEvent.mock.invocationCallOrder[1]);
    expect(h.onEvent.mock.invocationCallOrder[1]).toBeLessThan(h.onWindow.mock.invocationCallOrder[1]);
  });
  expect(h.onWindow).toHaveBeenCalledTimes(3);
  expect(h.onWindow.mock.calls[2][0]).toBe(h.onWindow.mock.calls[1][0]);
  expect(h.receive).toHaveBeenCalledOnce();
  expect(h.receive.mock.lastCall![0]).toBe(h.onWindow.mock.calls[1][0]);
  h.live.dispose();
});

it('flag_off_does_not_publish_for_an_unchanged_capture_event', () => {
  const receive = vi.fn(), source = homeSnapshot(), live = createHomeLiveWindow(receive);
  live.snapshot(source); receive.mockClear();
  live.event({ stream_id: source.stream_id, sequence: '20', at: source.at,
    event: { capture_state_changed: source.capture } });
  expect(receive).not.toHaveBeenCalled();
  live.dispose();
});

it('capture_events_never_reach_the_ordered_event_observer', () => {
  const h = harness();
  h.send({ capture_state_changed: h.source.capture });
  h.send({ capture_state_changed: { ...h.source.capture, state: 'Stopped' } });
  expect(h.onEvent).not.toHaveBeenCalled();
  expect(h.receive.mock.lastCall![0].drops).toEqual([]);
  h.live.dispose();
});

it('a_throwing_pre_boundary_observer_defers_publication_and_restores_event_state', () => {
  const h = harness(), drop = homeSnapshot().drops[0];
  let publishedDuringCheckpoint = -1;
  h.onWindow.mockImplementationOnce(() => {
    h.send({ bag_dropped: drop });
    publishedDuringCheckpoint = h.receive.mock.calls.length;
    throw new Error('synthetic checkpoint failure');
  });
  expect(() => h.send({ map_entered: { at: h.source.at, kind: 'nexus', name: 'Nexus' } }))
    .toThrow('synthetic checkpoint failure');
  expect(publishedDuringCheckpoint).toBe(0);
  expect(h.receive).toHaveBeenCalledOnce();
  expect(h.receive.mock.lastCall![0].drops).toEqual([drop]);
  h.onWindow.mockClear(); h.receive.mockClear();
  h.live.batch(() => {
    h.send({ drop_updated: { ...drop, bag_rank: 255 } });
    h.live.state({ phase: 'resyncing', message: 'synthetic gap', historyStale: true, historyFailed: false, historyLagging: false });
  });
  expect(h.onWindow.mock.calls[0][0].drops[0].bag_rank).toBe(255);
  expect(h.onWindow.mock.lastCall![0].drops).toEqual([]);
  expect(h.receive).toHaveBeenCalledOnce();
  h.live.dispose();
});

it.each(['snapshot', 'state', 'capture', 'recovery', 'dispose'] as const)(
  'non_event_%s_observes_pending_rows_once_on_entry_and_never_partway_through', kind => {
    vi.useFakeTimers();
    const h = harness(), drop = homeSnapshot().drops[0];
    try {
      if (kind === 'recovery') {
        h.send({ bag_dropped: drop });
        h.live.capture({ ...h.source.capture, state: 'Recovering' });
      }
      h.onWindow.mockClear();
      h.live.batch(() => {
        if (kind === 'recovery') h.live.select('synthetic-selection');
        else h.send({ bag_dropped: drop });
        if (kind === 'snapshot') h.live.snapshot({ ...h.source, sequence: '20',
          character: { ...h.source.character!, id: 'synthetic-next-character' }, capture: { ...h.source.capture, state: 'Stopped' } });
        if (kind === 'state') h.live.state({ phase: 'resyncing', message: 'synthetic gap', historyStale: true, historyFailed: false, historyLagging: false });
        if (kind === 'capture') h.live.capture({ ...h.source.capture, state: 'Stopped' });
        if (kind === 'recovery') vi.advanceTimersByTime(RECOVERY_GRACE_MS);
        if (kind === 'dispose') h.live.dispose();
        expect(h.onWindow).toHaveBeenCalledOnce();
        expect(h.onWindow.mock.calls[0][0].drops).toEqual([drop]);
      });
    } finally { h.live.dispose(); vi.useRealTimers(); }
  });

it.each(['snapshot', 'state', 'capture', 'recovery', 'dispose'] as const)(
  'non_event_%s_does_not_checkpoint_an_already_observed_window', kind => {
    vi.useFakeTimers();
    const h = harness();
    try {
      if (kind === 'recovery') h.live.capture({ ...h.source.capture, state: 'Recovering' });
      h.onWindow.mockClear();
      if (kind === 'snapshot') h.live.snapshot({ ...h.source, sequence: '20', area_kind: 'nexus', run: null });
      if (kind === 'state') h.live.state({ phase: 'resyncing', message: 'synthetic gap', historyStale: true, historyFailed: false, historyLagging: false });
      if (kind === 'capture') h.live.capture({ ...h.source.capture, state: 'Stopped' });
      if (kind === 'recovery') vi.advanceTimersByTime(RECOVERY_GRACE_MS);
      if (kind === 'dispose') h.live.dispose();
      expect(h.onWindow).toHaveBeenCalledOnce();
      expect(h.onWindow.mock.lastCall![0].runId).toBeNull();
    } finally { h.live.dispose(); vi.useRealTimers(); }
  });

it.each(['map', 'session', 'character', 'source', 'capture', 'state'] as const)(
  'checkpoints_pending_rows_before_%s_retires_the_scope', kind => {
    const h = harness(), drop = homeSnapshot().drops[0];
    h.live.batch(() => {
      h.send({ bag_dropped: drop });
      if (kind === 'map') h.send({ map_entered: { at: h.source.at, kind: 'nexus', name: 'Nexus' } });
      if (kind === 'session') h.send({ session_updated: { ...h.source.session!, id: 'synthetic-next-session' } });
      if (kind === 'character') h.send({ character_updated: { ...h.source.character!, id: 'synthetic-next-character' } });
      if (kind === 'source') h.send({ session_updated: h.source.session! }, h.source.at, 'synthetic-next-stream');
      if (kind === 'capture') h.send({ capture_state_changed: { ...h.source.capture, state: 'Stopped' } });
      if (kind === 'state') h.live.state({ phase: 'resyncing', message: 'synthetic gap',
        historyStale: true, historyFailed: false, historyLagging: false });
    });
    expect(h.onWindow.mock.calls[0][0].drops).toEqual([drop]);
    expect(h.onWindow.mock.lastCall![0].drops).toEqual([]);
    expect(h.receive).toHaveBeenCalledOnce();
    expect(h.onEvent).toHaveBeenCalledTimes(kind === 'state' || kind === 'capture' ? 1 : 2);
    h.live.dispose();
  });

it('unchanged_realm_snapshots_reuse_metadata_but_clock_fame_and_selection_invalidate', () => {
  const source = realmSnapshot(), receive = vi.fn();
  const live = createHomeLiveWindow(receive);
  live.snapshot(source);
  live.event({ stream_id: source.stream_id, sequence: '20', at: source.at, event: {
    map_entered: { at: source.at, kind: 'realm', name: 'Synthetic Realm', realm_instance_key: 'synthetic-realm' } } });
  const snapshot = { ...source, sequence: '21', instance: { ...source.instance!, visit_fame_delta: 0 } };
  live.snapshot(snapshot);
  const before = receive.mock.lastCall![0];
  live.snapshot(snapshot); expect(receive.mock.lastCall![0]).toBe(before);
  live.select('synthetic-selection'); expect(receive.mock.lastCall![0]).not.toBe(before);
  const selected = receive.mock.lastCall![0];
  live.select('synthetic-selection'); expect(receive.mock.lastCall![0]).toBe(selected);
  live.snapshot({ ...snapshot, at: String(BigInt(source.at) + 1n) });
  expect(receive.mock.lastCall![0]).not.toBe(selected);
  const clock = receive.mock.lastCall![0];
  live.snapshot({ ...snapshot, instance: { ...snapshot.instance, visit_fame_delta: 2 } });
  expect(receive.mock.lastCall![0].realmPeriods.at(-1).fame).toBe(2);
  expect(clock.realmPeriods.at(-1).fame).toBe(0);
  live.dispose();
});

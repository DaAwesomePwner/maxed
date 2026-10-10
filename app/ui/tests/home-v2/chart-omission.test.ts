import { afterEach, expect, it } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { fight, realmSnapshot } from './fixtures';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { scopedFights } from '../../src/lib/shell/rail';
import FightChart from '../../src/lib/home/v3/FightChart.svelte';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { FightView } from '../../src/lib/bindings/FightView';

const disposals: (() => void)[] = [];
afterEach(() => { for (const dispose of disposals.splice(0)) dispose(); });
function setup() {
  const source = realmSnapshot(); source.sequence = '0'; source.at = '100';
  let window = emptyHomeLiveWindow(), sequence = 0;
  const live = createHomeLiveWindow(value => { window = value; });
  disposals.push(live.dispose);
  live.snapshot(source);
  const send = (event: EngineEvent, omitted = false, at = '200') => live.event({
    stream_id: source.stream_id, sequence: String(++sequence), at, event, fight_chart_omitted: omitted,
  });
  const map = (kind: 'realm' | 'nexus', at: string) => send({ map_entered: {
    kind, name: kind, at, realm_instance_key: kind === 'realm' ? 'realm-a' : null,
  } }, false, at);
  map('realm', '100');
  const row = (id = 'boss', extra: Partial<FightView> = {}) => fight(id, {
    session_id: source.session!.id, run_id: null, started: '110', ended: null,
    last_hit: '150', local_last_hit: null, kill: null,
    buckets: [{ from: '110', until: '150', total: 100, local: 40 }],
    player_series: [{ player_id: 'synthetic', object_id: 1, is_local: false, points: [100] }], ...extra,
  });
  const snapshot = (rows: FightView[]) => {
    const next = { ...source, at: '250', sequence: String(++sequence), fights: rows };
    live.snapshot(next); return next;
  };
  const update = (value: FightView) => ({ ...value, buckets: [], player_series: null });
  return { live, source, send, map, row, snapshot, update, get window() { return window; } };
}

it('keeps an omitted chart pair by reference, reuses repeated source projections, and accepts empty unmarked updates', () => {
  const h = setup(), original = h.row();
  h.send({ fight_started: original });
  const update = h.update({ ...original, total_damage: 200 });
  h.send({ fight_updated: update }, true);
  const first = h.window.fights;
  expect(first[0].total_damage).toBe(200);
  expect(first[0].buckets).toBe(original.buckets);
  expect(first[0].player_series).toBe(original.player_series);
  h.send({ fight_updated: update }, true);
  expect(h.window.fights).toBe(first);
  expect(h.window.fights[0]).toBe(first[0]);
  expect(update.buckets).toEqual([]);
  h.send({ fight_updated: update });
  expect(h.window.fights[0].buckets).toEqual([]);
  expect(h.window.fights[0].player_series).toBeNull();
});

it('refreshes live charts from snapshots and invalidates the merge cache when the chart pair changes', async () => {
  const h = setup(), original = h.row();
  h.send({ fight_started: original });
  const update = h.update(original);
  h.send({ fight_updated: update }, true);
  const chart = render(FightChart, { buckets: h.window.fights[0].buckets });
  expect(screen.getByRole('img', { name: 'Party and your DPS over the fight' })).toBeTruthy();
  const before = chart.container.querySelector('.peak')!.textContent;
  const fresh = h.row('boss', { buckets: [{ from: '110', until: '150', total: 200, local: 80 }], player_series: null });
  const snapshot = h.snapshot([fresh]);
  h.send({ fight_updated: update }, true);
  expect(h.window.fights[0].buckets).toBe(fresh.buckets);
  expect(h.window.fights[0].player_series).toBeNull();
  expect(scopedFights(snapshot, h.window)[0].buckets).toBe(fresh.buckets);
  await chart.rerender({ buckets: h.window.fights[0].buckets });
  expect(chart.container.querySelector('.peak')!.textContent).not.toBe(before);
  h.snapshot([h.update(fresh)]);
  expect(h.window.fights[0].buckets).toEqual([]);
  expect(h.window.fights[0].player_series).toBeNull();
});

it('accepts an omitted first observation without inventing a chart, then seeds it from a snapshot', () => {
  const h = setup(), original = h.row();
  h.send({ fight_updated: h.update(original) }, true);
  expect(h.window.fights[0].buckets).toEqual([]);
  h.snapshot([original]);
  expect(h.window.fights[0].buckets).toBe(original.buckets);
});

it('preserves final charts without a final snapshot, through departure batches and same-instance return', () => {
  const h = setup(), original = h.row();
  h.send({ fight_started: original });
  h.live.select(original.id);
  const final = h.row('boss', { ended: '250', buckets: [{ from: '110', until: '250', total: 300, local: 100 }] });
  h.live.batch(() => {
    h.send({ fight_updated: h.update(original) }, true);
    h.send({ fight_ended: final }, false, '250');
    h.map('nexus', '300');
  });
  expect(h.window.fights).toEqual([]);
  h.map('realm', '400');
  expect(h.window.fights[0].buckets).toBe(final.buckets);
  expect(h.window.fights[0].player_series).toBe(final.player_series);
  const next = h.snapshot([]);
  expect(scopedFights(next, h.window)[0].buckets).toBe(final.buckets);
  const correction = { ...final, buckets: [{ from: '110', until: '250', total: 350, local: 100 }] };
  h.send({ fight_updated: correction });
  expect(h.window.fights[0].buckets).toBe(correction.buckets);
});

it('keeps selected charts within the 64-fight cap and does not resurrect evicted charts', () => {
  const h = setup(), selected = h.row('selected'), evicted = h.row('evicted');
  h.send({ fight_started: selected }); h.live.select(selected.id);
  h.send({ fight_started: evicted });
  for (let i = 0; i < 70; i++) h.send({ fight_ended: h.row(`other-${i}`, { started: String(120 + i), ended: '200' }) });
  expect(h.window.fights).toHaveLength(64);
  expect(h.window.fights.find(row => row.id === evicted.id)).toBeUndefined();
  h.send({ fight_updated: h.update(selected) }, true);
  expect(h.window.fights.find(row => row.id === selected.id)!.buckets).toBe(selected.buckets);
  h.live.select(evicted.id);
  h.send({ fight_updated: h.update(evicted) }, true);
  expect(h.window.fights.find(row => row.id === evicted.id)!.buckets).toEqual([]);
});

it.each(['target_generation', 'target_id', 'started'] as const)('does not reuse charts across changed %s identity', field => {
  const h = setup(), original = h.row();
  h.send({ fight_started: original });
  const changed = h.update({ ...original, [field]: field === 'started' ? '120' : Number(original[field]) + 1 });
  h.send({ fight_updated: changed }, true);
  expect(h.window.fights[0].buckets).toEqual([]);
  expect(h.window.fights[0].player_series).toBeNull();
});

it.each(['stream', 'session', 'resync'] as const)('does not reuse a cached merge after %s retirement', retirement => {
  const h = setup(), original = h.row(), update = h.update(original);
  h.send({ fight_started: original }); h.send({ fight_updated: update }, true);
  if (retirement === 'stream') h.source.stream_id = 'new-stream';
  else if (retirement === 'session') h.source.session = { ...h.source.session!, id: 'new-session' };
  else h.live.state({ phase: 'resyncing', historyStale: true, historyFailed: false, historyLagging: false, message: '' });
  h.snapshot([]); h.map('realm', '100');
  h.send({ fight_updated: retirement === 'session' ? { ...update, session_id: h.source.session!.id } : update }, true);
  expect(h.window.fights[0].buckets).toEqual([]);
  expect(h.window.fights[0].player_series).toBeNull();
});

it('keeps sequence acceptance and absent-open snapshot removal unchanged', () => {
  const h = setup(), original = h.row();
  h.send({ fight_started: original });
  h.snapshot([original]);
  const before = h.window.fights;
  h.live.event({ stream_id: h.source.stream_id, sequence: '1', at: '200', fight_chart_omitted: true,
    event: { fight_updated: h.update({ ...original, total_damage: 999 }) } });
  expect(h.window.fights).toBe(before);
  h.snapshot([]);
  expect(h.window.fights).toEqual([]);
  h.send({ fight_updated: h.update(original) }, true);
  expect(h.window.fights[0].buckets).toEqual([]);
});

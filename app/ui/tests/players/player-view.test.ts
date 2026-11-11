import { expect, it } from 'vitest';
import type { PlayerTimeline } from '../../src/lib/bindings/PlayerTimeline';
import type { RunPlayerView } from '../../src/lib/bindings/RunPlayerView';
import type { GearChange } from '../../src/lib/bindings/GearChange';
import { fight, player, run, source } from '../runs/fixtures';
import { activeMs, idleGaps, sliceTimeline, timelineFromFightSeries, presenceEnd, gearAt, swapsIn,
  vsAverage, vsTop, runPlayerKpis, fightPlayerKpis, activityItems } from '../../src/lib/players/player-view';

const us = (ms: number) => String(ms * 1000);

it('extends stale presence to the latest timeline bucket only while live', () => {
  const t = timeline([1000, 1000, 1000], [10, 0, 10]);
  expect(presenceEnd('1000000', t, true)).toBe(3000);
  expect(presenceEnd('1000000', t, false)).toBe(1000);
  expect(presenceEnd('4000000', t, true)).toBe(4000);
  expect(presenceEnd(null, t, true)).toBeNull();
  const profile = { scope_id: 'run-1', player_id: 'player-1', object_id: 7, is_local: true,
    maxed_stats: [], stats: [], equipment: [], first_seen: '0', last_seen: '1000000' };
  expect(runPlayerKpis(view({ profile, timeline: t, run: run({ ended: null }) }))[7]).toMatchObject({ value: '66.7%', sub: '0:02 of 0:03 dealing dmg' });
  expect(runPlayerKpis(view({ profile, timeline: t }))[7].sub).toBe('0:02 of 0:01 dealing dmg');
});
function timeline(widths: number[], points: number[], totals = widths.map(() => 100)): PlayerTimeline {
  let at = 0;
  return { buckets: widths.map((width, i) => {
    const from = at; at += width;
    return { from: us(from), until: us(at), party_total: totals[i], party_active: 2 };
  }), points, compacted: false, disabled: false };
}
function view(overrides: Partial<RunPlayerView> = {}): RunPlayerView {
  return { run: run(), scope_id: 'run-1', player_id: 'player-1', player: player({ damage: 1000, hits: 4, dps: 100, rank: 2, share: 0.25 }),
    party: { total_damage: 4000, average_dps: 50, top_name: 'Invented Mage', top_dps: 200, players_with_damage: 3 },
    sources: [source({ biggest_hit: 400 })], fights: [], profile: null, gear_log: [], timeline: null,
    deaths: [], roster: [], partial: false, ...overrides };
}

it('finds maximal 30-second idle gaps inside presence, excluding party-idle buckets and unobserved holes', () => {
  const t = timeline([29_000, 1000, 31_000, 5000, 40_000], [0, 1, 0, 0, 0], [100, 100, 100, 0, 100]);
  expect(idleGaps(t, { from: 0, until: 106_000 })).toEqual([{ from: 30_000, until: 61_000 }, { from: 66_000, until: 106_000 }]);
  expect(idleGaps(t, { from: 32_000, until: 90_000 })).toEqual([]);
  expect(idleGaps(t, { from: 0, until: 106_000 }, 29_000)[0]).toEqual({ from: 0, until: 29_000 });
  const holes = { ...timeline([20_000, 20_000], [0, 0]), buckets: [
    { from: '0', until: us(20_000), party_total: 100, party_active: 2 },
    { from: us(25_000), until: us(45_000), party_total: 100, party_active: 2 },
  ] };
  expect(idleGaps(holes, { from: 0, until: 45_000 })).toEqual([]);
});

it('sums positive bucket widths before and after compaction', () => {
  expect(activeMs(timeline([1000, 1000, 1000], [10, 0, 10]))).toBe(2000);
  expect(activeMs({ ...timeline([2000, 1000], [10, 10]), compacted: true })).toBe(3000);
});

it('slices intersecting whole buckets without redistributing observed damage or mutating inputs', () => {
  const t = timeline([1000, 1000, 1000], [10, 20, 30]);
  expect(sliceTimeline(t, 1000, 2000)).toEqual({ ...t, buckets: [t.buckets[1]], points: [20] });
  expect(sliceTimeline(t, 500, 1500).points).toEqual([10, 20]);
  expect(sliceTimeline(t, 3000, 4000).buckets).toEqual([]);
  expect(sliceTimeline(t, 2000, 1000).buckets).toEqual([]);
  expect(t.points).toEqual([10, 20, 30]);
});

it('builds a fight timeline from matching series and per-bucket active series rows', () => {
  const f = fight({ buckets: [{ from: '0', until: '1000000', total: 40, local: 10 }], player_series: [
    { player_id: 'player-1', object_id: 7, is_local: true, points: [10] },
    { player_id: 'player-2', object_id: 8, is_local: false, points: [30] },
    { player_id: 'player-3', object_id: 9, is_local: false, points: [0] },
  ] });
  expect(timelineFromFightSeries(f, 'player-1')).toEqual({ buckets: [{ from: '0', until: '1000000', party_total: 40, party_active: 2 }], points: [10], compacted: false, disabled: false });
  expect(timelineFromFightSeries(f, 'absent')).toBeNull();
  expect(timelineFromFightSeries({ ...f, player_series: null }, 'player-1')).toBeNull();
  expect(timelineFromFightSeries({ ...f, player_series: [...f.player_series!, f.player_series![0]] }, 'player-1')).toBeNull();
});

it('finds gear as of a time and selects swaps in a half-open window, even with unsorted logs', () => {
  const log: GearChange[] = [{ at: '2000000', equipment: [] }, { at: '1000000', equipment: [
    { slot: 0, item_type: 1000, enchant: source().enchant },
  ] }];
  expect(gearAt(log, 999)).toBeNull();
  expect(gearAt(log, 1000)).toEqual(log[1].equipment);
  expect(gearAt(log, 2000)).toEqual([]);
  expect(swapsIn(log, 1000, 2000)).toEqual([log[1]]);
  expect(swapsIn(log, 2000, 3000)).toEqual([log[0]]);
});

it('falls back to the scope timeline when fight series omit damaging players', () => {
  const players = Array.from({ length: 8 }, (_, i) => player({ player_id: `player-${i}`, damage: 10 }));
  const f = fight({ players, buckets: [{ from: '0', until: '1000000', total: 80, local: 10 }],
    player_series: players.slice(0, 6).map(p => ({ player_id: p.player_id, object_id: p.object_id, is_local: p.is_local, points: [10] })) });
  expect(timelineFromFightSeries(f, 'player-0')).toBeNull();
});

it('clamps compacted active ratios and names deaths outside boss fights', () => {
  const profile = { scope_id: 'run-1', player_id: 'player-1', object_id: 7, is_local: true,
    maxed_stats: [], stats: [], equipment: [], first_seen: '500000', last_seen: '1500000' };
  const cells = runPlayerKpis(view({ profile, timeline: { ...timeline([2000], [10]), compacted: true },
    deaths: [{ at: '1000000', fight_id: 'boss-1' }, { at: '1500000', fight_id: null }],
    fights: [{ fight_id: 'boss-1', dps_basis: 'player_active_window_v1', started: '0', target_name: 'Invented Sentinel', killed: true, player_count: 2 }] }));
  expect(cells[7]).toMatchObject({ value: 'approx. 100%', sub: '0:02 of 0:01 dealing dmg' });
  expect(cells[6].sub).toBe('Invented Sentinel, outside boss fights');
});

it('uses supplied fight presence for Active and preserves unknown presence', () => {
  const p = player(), f = fight({ players: [p] }), t = timeline([1000, 1000], [10, 0]);
  expect(fightPlayerKpis(f, p, [], t, 2000)[7]).toMatchObject({ value: '50%', sub: '0:01 of 0:02 dealing dmg' });
  expect(fightPlayerKpis(f, p, [], t, null)[7].value).toBe('—');
});

it('preserves null, zero denominators and invalid rates when comparing DPS', () => {
  for (const compare of [vsTop, vsAverage]) {
    expect(compare(100, 50)).toBe(2);
    expect(compare(0, 50)).toBe(0);
    for (const [a, b] of [[null, 1], [1, null], [1, 0], [-1, 1], [NaN, 1], [1, Infinity]]) expect(compare(a, b)).toBeNull();
    expect(compare(Number.MAX_VALUE, Number.MIN_VALUE)).toBeNull();
  }
});

it('builds eight run KPIs in spec order with party, source and death context', () => {
  const cells = runPlayerKpis(view({ deaths: [{ at: '3000000', fight_id: 'boss-1' }], fights: [{
    fight_id: 'boss-1', dps_basis: 'player_active_window_v1', started: '1000000', target_name: 'Invented Sentinel', killed: true, player_count: 3,
  }] }));
  expect(cells.map(c => c.label)).toEqual(['Damage', 'DPS', 'vs Top', 'Rank', 'Hits', 'Best hit', 'Deaths', 'Active']);
  expect(cells.slice(0, 6).map(c => c.value)).toEqual(['1,000', '100', '50%', '#2', '4', '400']);
  expect(cells[0].sub).toBe('of 4,000 party');
  expect(cells[1].sub).toBe('+100% vs avg');
  expect(cells[2].sub).toBe('Invented Mage 200');
  expect(cells[3]).toMatchObject({ of: '3', sub: '25% share' });
  expect(cells[4].sub).toBe('avg 250');
  expect(cells[6].sub).toBe('Invented Sentinel');
});

it('leaves damage cells unknown for a roster-only player and labels compacted activity approximate', () => {
  expect(runPlayerKpis(view({ player: null })).slice(0, 6).map(c => c.value)).toEqual(Array(6).fill('—'));
  const t = { ...timeline([1000, 1000], [10, 0]), compacted: true };
  const profile = { scope_id: 'run-1', player_id: 'player-1', object_id: 7, is_local: true, maxed_stats: [], stats: [], equipment: [], first_seen: '0', last_seen: '2000000' };
  expect(runPlayerKpis(view({ timeline: t, profile }))[7]).toMatchObject({ value: 'approx. 50%', sub: '0:01 of 0:02 dealing dmg' });
  expect(runPlayerKpis(view({ timeline: { ...t, disabled: true }, profile }))[7].value).toBe('—');
});

it('builds fight KPIs with the mean DPS of damaging players and unknown active denominator without presence', () => {
  const p = player({ dps: 100, damage: 1000 });
  const f = fight({ players: [p, player({ player_id: 'second', dps: 200 }), player({ player_id: 'idle', damage: 0, dps: 0 })] });
  const cells = fightPlayerKpis(f, p, [{ at: '3000000', fight_id: f.id }], timeline([1000], [10]));
  expect(cells).toHaveLength(8);
  expect(cells[1].sub).toBe('−33.3% vs avg');
  expect(cells[2].value).toBe('50%');
  expect(cells[6].value).toBe('1');
  expect(cells[7].value).toBe('—');
  const incomplete = fightPlayerKpis({ ...f, players: [p, player({ dps: null })] }, p, [], null);
  expect(incomplete[1].sub).toBe('—');
  expect(incomplete[2].value).toBe('—');
});

it('orders activity chronologically with stable ties and ignores absent events', () => {
  const items = activityItems({ firstSeen: 1000, firstHit: 2000, lastHit: 8000, departedAt: 9000,
    deaths: [{ at: '7000000' }, { at: '5000000' }], gaps: [{ from: 3000, until: 6000 }] });
  expect(items.map(i => i.kind)).toEqual(['entered', 'first-hit', 'idle', 'death', 'death', 'last-hit', 'departed']);
  expect(items[2]).toMatchObject({ at: 3000, until: 6000, label: 'Idle 0:03 (no damage)' });
  expect(activityItems({ firstSeen: null, firstHit: null, lastHit: null, departedAt: null, deaths: [], gaps: [] })).toEqual([]);
});

it.each([
  { label: 'retained death list', deaths: [{ at: '1000000' }, { at: '2000000' }], row: player({ deaths: 7 }), profile: null, expected: '2' },
  { label: 'legacy matched deaths', deaths: [], row: player({ deaths: 3 }), profile: null, expected: '3' },
  { label: 'known zero in player row', deaths: [], row: player({ deaths: 0 }), profile: null, expected: '0' },
  { label: 'unknown without row or profile', deaths: [], row: null, profile: null, expected: '—' },
  { label: 'observed profile without damage', deaths: [], row: null, profile: { scope_id: 'run-1', player_id: 'player-1', object_id: 7,
    is_local: true, maxed_stats: [], stats: [], equipment: [], first_seen: '0', last_seen: '2000000' }, expected: '0' },
])('uses the deaths ruling in both scopes: $label', ({ deaths, row, profile, expected }) => {
  expect(runPlayerKpis(view({ player: row, profile, deaths }))[6].value).toBe(expected);
  expect(fightPlayerKpis(fight(), row, deaths, null, null, profile)[6].value).toBe(expected);
});

it.each([-1, NaN, Infinity])('keeps invalid saved deaths unknown in both scopes: %s', deaths => {
  const row = player({ deaths });
  expect(runPlayerKpis(view({ player: row }))[6]).toMatchObject({ value: '\u2014', tone: null });
  expect(fightPlayerKpis(fight(), row, [], null)[6]).toMatchObject({ value: '\u2014', tone: null });
});

it('labels fight KPIs by their saved basis and run KPIs by the whole-run denominator', () => {
  const p = player();
  for (const basis of ['player_active_window_v1', 'whole_target_v0'] as const) {
    const cell = fightPlayerKpis(fight({ dps_basis: basis }), p, [], null)[1];
    expect(cell.label).toBe(basis === 'player_active_window_v1' ? 'Active DPS' : 'DPS');
    expect(cell.tooltip).toContain(basis === 'player_active_window_v1' ? 'that player' : 'whole fight');
    expect(cell.value).toBe('17');
  }
  expect(runPlayerKpis(view())[1]).toMatchObject({ label: 'DPS', tooltip: expect.stringContaining('whole run') });
});

it('tones the Damage KPI gold only for the local player, profile first', () => {
  const profile = (is_local: boolean) => ({ scope_id: 'run-1', player_id: 'player-1', object_id: 7, is_local,
    maxed_stats: [], stats: [], equipment: [], first_seen: '0', last_seen: '2000000' });
  const damage = (cells: { key: string; tone?: string | null }[]) => cells.find(cell => cell.key === 'damage')?.tone ?? null;
  for (const [row, prof, expected] of [
    [player({ is_local: true }), null, 'fame'],
    [player({ is_local: false }), null, null],
    [player({ is_local: false }), profile(true), 'fame'],
    [player({ is_local: true }), profile(false), null],
    [null, null, null],
  ] as const) {
    expect(damage(runPlayerKpis(view({ player: row, profile: prof })))).toBe(expected);
    expect(damage(fightPlayerKpis(fight(), row, [], null, null, prof))).toBe(expected);
  }
});

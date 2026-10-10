import { expect, it } from 'vitest';
import { runCrumbs, runHeaderLine, runKpis, runMarkers, otherTargets } from '../../src/lib/runs/run-screen';
import { fightSummary, run, story } from './fixtures';

it('compares an observed clear with its PB baseline and qualifies the average', () => {
  const value = story({ run: run({ started: '1000000', completed_at: '581000000' }), comparison: null });
  expect(runKpis(value, { dungeon: value.run.dungeon, best_clear_micros: 552000000 })[0])
    .toMatchObject({ key: 'clear', value: '9:40', sub: 'PB 9:12 · +0:28', subTone: 'bad' });
  expect(runKpis(value, { dungeon: value.run.dungeon, best_clear_micros: 580000000 })[0].subTone).toBe('good');
  value.comparison = { previous_clears: 3, mean_micros: 600000000, change_fraction: -.2 };
  expect(runKpis(value, null)[0].sub).toContain('20% faster than avg');
  value.comparison.previous_clears = 2;
  expect(runKpis(value, null)[0].sub).not.toContain('avg');
});
it('keeps unavailable clear, totals, identity and fame unknown', () => {
  const value = story({ run: run({ outcome: 'died', completed_at: null, party_size: 6 }), totals: null });
  const cells = runKpis(value, null);
  for (const key of ['clear', 'damage', 'share', 'fame']) expect(cells.find(cell => cell.key === key)?.value).toBe('—');
  expect(cells.find(cell => cell.key === 'party')?.value).toBe('~6');
});
it('excludes every boss tier and unidentified targets from other targets', () => {
  const value = story({ fights: [fightSummary(1), fightSummary(2, { target_kind: 'quest' }),
    fightSummary(3, { boss_type: 2 }), fightSummary(4, { target_type: null })] });
  expect(otherTargets(value).map(row => row.id)).toEqual(['fight-1']);
});
it('omits unknown header fields and links breadcrumbs to Runs', () => {
  const value = story({ run: run({ context: null, party_size: null }) });
  const line = runHeaderLine(value, null);
  expect(line).toContain('Entry Knight');
  expect(line).not.toMatch(/unknown|undefined|null|class|realm|party/i);
  expect(runCrumbs(value)[0]).toEqual({ label: 'Runs', href: '#/runs' });
});
it('retains departure notifications as left markers', () => {
  const value = story({ events: [{ id: 'left', session_id: 's', at: '4000000', provisional: false,
    kind: { type: 'player_left', object_id: 1, player_name: 'DemoAlly' } }] });
  expect(runMarkers(value)).toContainEqual(expect.objectContaining({ event: 'left', at: '4000000' }));
});
it('keeps missing fame and local identity unknown even when totals exist', () => {
  const value = story({ totals: { total_damage: 123, unattributed_damage: 0, fights: 1, kills: 0, players: [], local: null, fame_delta: null, partial: true } });
  expect(runKpis(value, null).find(cell => cell.key === 'damage')?.value).toBe('123');
  for (const key of ['share', 'fame']) expect(runKpis(value, null).find(cell => cell.key === key)?.value).toBe('—');
});
it('does not compare a counter-estimated clear to the average', () => {
  const value = story({ run: run({ evidence: [{ at: '9000000', evidence: { completion_counter: { before: 1, after: 2, delay_micros: 1000000 } } }] }) });
  expect(runKpis(value, null)[0].sub).not.toContain('avg');
});

it.each([
  [null, ['before', 'exact', 'after']],
  ['9000000', ['before']],
] as const)('saved story counts only departures before completion (%s)', (completed_at, ids) => {
  const events = ['8000000', '9000000', '10000000'].map((at, i) => ({ id: ['before', 'exact', 'after'][i],
    session_id: 's', at, provisional: false, kind: { type: 'player_left' as const, object_id: i, player_name: `Ally ${i}` } }));
  const value = story({ run: run({ completed_at, complete_capture: true, issues: [] }), events });
  expect(runKpis(value, null).find(cell => cell.key === 'party')?.sub).toBe(`${ids.length} left · 0 deaths`);
  expect(runMarkers(value).filter(marker => marker.event === 'left').map(marker => marker.id)).toEqual(ids.map(id => `event:${id}`));
});

it.each(['absent', 'null', 'incomplete', 'issues', 'limited', 'partial totals', 'complete'] as const)(
  'keeps unavailable event counts distinct from observed zero (%s)', state => {
    const value = story({ run: run({ complete_capture: state !== 'incomplete', issues: state === 'issues' ? ['stream_gap'] : [] }),
      events: state === 'null' ? null : [], events_limited: state === 'limited' });
    if (state === 'absent') delete value.events;
    if (state === 'partial totals') value.totals = { total_damage: 0, unattributed_damage: 0, fights: 0, kills: 0,
      players: [], local: null, fame_delta: null, partial: true };
    expect(runKpis(value, null).find(cell => cell.key === 'party')?.sub).toBe(state === 'complete' ? '0 left · 0 deaths' : '— left · — deaths');
  });

it('retains observed counts during partial capture and deaths at or after completion', () => {
  const events = [
    { id: 'left', session_id: 's', at: '8000000', provisional: false,
      kind: { type: 'player_left' as const, object_id: 1, player_name: 'Ally' } },
    ...['9000000', '10000000'].map((at, i) => ({ id: `death-${i}`, session_id: 's', at, provisional: false,
      kind: { type: 'player_death' as const, player_name: `Ally ${i}`, is_local: false } })),
  ];
  const value = story({ run: run({ complete_capture: false }), events });
  expect(runKpis(value, null).find(cell => cell.key === 'party')?.sub).toBe('1 left · 2 deaths');
  expect(runMarkers(value).filter(marker => marker.event === 'death').map(marker => marker.at)).toEqual(['9000000', '10000000']);
  value.events_limited = true;
  expect(runKpis(value, null).find(cell => cell.key === 'party')?.sub).toBe('~1 left · ~2 deaths');
  value.events = [events[0]];
  expect(runKpis(value, null).find(cell => cell.key === 'party')?.sub).toBe('~1 left · — deaths');
});
it('shows an observed clear while the cleared run is still open', () => {
  const value = story({ run: run({ started: '1000000', completed_at: '581000000', ended: null, outcome: 'cleared' }) });
  expect(runKpis(value, { dungeon: value.run.dungeon, best_clear_micros: 552000000 })[0])
    .toMatchObject({ value: '9:40', sub: 'PB 9:12 · +0:28 · 20% faster than avg' });
  for (const overrides of [{ outcome: 'died' as const }, { completed_at: '1000000' }, { completed_at: null }]) {
    expect(runKpis(story({ run: run({ started: '1000000', completed_at: '581000000', ended: null, ...overrides }) }), null)[0].value).toBe('—');
  }
});

import { expect, it } from 'vitest';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { RunFightSummary } from '../../src/lib/bindings/RunFightSummary';
import type { RunRecapData, VisitRecapData } from '../../src/lib/home/v4/recap-reads';
import { indefiniteArticle, recapVisualTypes, runRecapView, visitRecapView } from '../../src/lib/home/v4/recap-view';
import { homeSnapshot } from '../home/fixtures';

const us = (seconds: number) => String(seconds * 1_000_000);
function detail(id = 'boss-1', seconds = 60): FightView {
  return { ...homeSnapshot().fights[0], id, target_type: 300, target_kind: 'boss', started: us(1),
    ended: us(100), first_hit: us(1), last_hit: us(11),
    kill: { at: us(1 + seconds), evidence: 'hp_zero_removed', provisional: false },
    players: [{ ...homeSnapshot().fights[0].players[0], is_local: true, dps: 200, rank: 2, share: .25, biggest_hit: 1200 }],
    buckets: [{ from: us(1), until: us(2), total: 300, local: 200, others_damage: 100, others_presence: 1 }] };
}
function summary(id = 'boss-1', overrides: Partial<RunFightSummary> = {}): RunFightSummary {
  return { id, target_type: 300, boss_type: 300, target_kind: 'boss', target_name: 'Invented Guardian', ordinal: 1,
    started: us(1), ended: us(100), first_hit: us(1), last_hit: us(11), total_damage: 8000, local_damage: 2000,
    local_dps: 200, local_rank: 2, party: { avg_dps: 100, players: 3 }, player_count: 4,
    partial: false, estimated: false, ...overrides };
}
function runData(): RunRecapData {
  const local = detail().players[0];
  return { kind: 'run', recordsUnavailable: false, story: { run: { ...homeSnapshot().run!, id: 'synthetic-run', dungeon: 'Invented Sanctum',
    started: us(1), completed_at: us(121), ended: us(181), outcome: 'cleared' }, character: null,
    fights: [summary()], boss_fights: [detail()], boss_fights_limited: false, drops: [], timeline: [], comparison: null,
    totals: { fights: 1, kills: 1, total_damage: 8000, unattributed_damage: 0, local, players: [local], fame_delta: 12, partial: false } },
    extras: { run_id: 'synthetic-run', clear_number: 38, in_a_row: 3, chain: null, pb_clear: null,
      dry_streak_end: null, best_dps: [], milestones: [] },
    profile: { dungeon: 'Invented Sanctum', clears: 38, usual_run_dps: 100, run_dps_samples: 4,
      usual_clear_micros: usNumber(150), clear_samples: 4, bosses: [{ target_type: 300, target_name: 'Invented Guardian',
        seen_in: 4, median_offset_micros: 0, median_kill_micros: usNumber(65), kill_samples: 3, best_dps: 150 }] },
    baselines: { rows: [{ target_type: 300, fights: 4, avg_dps: 100, best_dps: 150,
      kill_samples: 3, median_kill_micros: usNumber(70) }], truncated: false }, streak: null };
}
const usNumber = (seconds: number) => seconds * 1_000_000;

it('missing_boss_detail_kill_is_unknown', () => {
  const data = runData(); data.story.boss_fights = []; data.story.boss_fights_limited = true;
  expect(runRecapView(data).fights[0].kill).toBe('—');
});

it('fame_negative_is_unknown_and_death_is_singular', () => {
  const data = runData(); data.story.totals!.fame_delta = -5; data.story.totals!.players[0].deaths = 1;
  expect(runRecapView(data).tiles[4]).toMatchObject({ value: '—', detail: '1 death' });
  data.story.totals!.players[0].deaths = 2;
  expect(runRecapView(data).tiles[4].detail).toBe('2 deaths');
  const visit = visitData(); visit.visit.fame_delta = -5;
  expect(visitRecapView(visit).tiles[4].value).toBe('—');
});

it('recap_visual_types_include_all_saved_evidence_once', () => {
  const view = runRecapView(runData()), drop = homeSnapshot().drops[0];
  view.fights.push({ ...view.fights[0], targetType: null });
  view.drops = [{ ...drop, bag_type: 500, contents: [{ ...drop.contents[0], item_type: 501 }], items: [502], contents_known: true },
    { ...drop, bag_type: 500, contents: [], items: [999], contents_known: false }];
  view.best = { drop: { ...drop, bag_type: 503, contents: [], items: [504], contents_known: true } };
  view.chain = { label: 'Synthetic chain', totals: { time: '1:00', bags: 1, whites: 0 }, steps: [
    { visit_id: 'synthetic-step', kind: 'realm', name: 'Synthetic realm', run_id: null, started: '0', ended: '60000000',
      local_dps: null, party_dps: null, bags: 1, whites: 0, item_types: [505, 501] },
  ] };
  view.chips = [{ kind: 'new-items', text: 'New in collection', itemTypes: [506, 502] }];
  view.death = { killed_by: null, killed_by_type: 507, fame: null, hp_trace: [] };
  view.character = { name: null, class_id: 508, maxed: null };
  const before = structuredClone(view), types = recapVisualTypes(view);
  expect([...types].sort((a, b) => a - b)).toEqual([300, 500, 501, 502, 503, 504, 505, 506, 507, 508]);
  expect(view).toEqual(before);
  view.death.killed_by_type = null; view.character.class_id = null;
  expect(recapVisualTypes(view)).not.toContain(null);
});

it('recap_character_comes_from_the_saved_story', () => {
  const data = runData();
  expect(runRecapView(data).character).toBeNull();
  data.story.character = { ...homeSnapshot().character!, name: 'Synthetic Hero', class_id: 508, maxed: 8 };
  data.story.character.stat_panel = { breakdown_provisional: true, conditions: [], lines:
    [0, 3, 20, 21, 22, 26, 27, 28].map(stat_id => ({ stat_id, base: 75, cap: 75, exalt: 0, at_cap: true,
      split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } })) };
  expect(runRecapView(data).character).toEqual({ name: 'Synthetic Hero', class_id: 508, maxed: 8 });
  const before = structuredClone(data.story.character);
  data.story.character.stat_panel.lines[0].split_evidence = undefined;
  expect(runRecapView(data).character?.maxed).toBeNull();
  data.story.character = structuredClone(before);
  data.story.character.stat_panel!.lines[0].split_evidence!.cap_eligible = false;
  expect(runRecapView(data).character?.maxed).toBeNull();
  data.story.character.stat_panel = null;
  expect(runRecapView(data).character?.maxed).toBeNull();
  data.story.character.stat_panel = { breakdown_provisional: true, conditions: [], lines: [] };
  expect(runRecapView(data).character?.maxed).toBeNull();
});
function visitData(): VisitRecapData {
  return { kind: 'visit', baselines: null, visit: { visit_id: 'synthetic-visit', session_id: 'synthetic-session',
    character_id: 'synthetic-player', title: 'Invented Realm', periods: [{ visit_id: 'entry-1', started: '0', ended: us(100) }],
    fights: [summary()], boss_fights: [detail()], boss_fights_limited: false, announcements: [], announcements_limited: false,
    drops: [], drops_limited: false, fame_delta: 5, best_dps: [], chain: null } };
}
function announcement(id: string, at: string, object_type = 300) {
  return { id, session_id: 'synthetic-session', at, provisional: true,
    kind: { type: 'realm_event_killed' as const, object_type, boss_key: 'synthetic-boss', killer: null } };
}

it('outcome_labels_and_clear_time_only_when_cleared', () => {
  const data = runData();
  for (const [outcome, label] of [['cleared', 'Cleared'], ['died', 'Died'], ['left', 'Left early'],
    ['unknown', 'Outcome not established'], [null, 'Outcome not established']] as const) {
    data.story.run.outcome = outcome;
    const view = runRecapView(data);
    expect(view.outcomeLabel).toBe(label); expect(view.clearTime).toBe(outcome === 'cleared' ? '2:00' : null);
  }
  data.story.run.outcome = 'died'; data.story.run.died_at = us(61);
  data.story.run.death = { killed_by: 'Invented Guardian', killed_by_type: 300, fame: 12, hp_trace: [] };
  expect(runRecapView(data)).toMatchObject({ died: { at: us(61), elapsed: '1:00' }, death: data.story.run.death });
});

it('meta_lists_clear_number_in_a_row_and_end_time', () => {
  const data = runData();
  data.story.run.ended = String(Date.parse('2026-10-13T23:50:00Z') * 1000);
  expect(runRecapView(data, 'UTC').meta).toEqual(['Clear #38', '3rd Invented Sanctum in a row tonight', 'ended 23:50']);
  expect(runRecapView(data, 'America/Chicago').meta.at(-1)).toBe('ended 18:50');
  for (const [n, ordinal] of [[2, '2nd'], [4, '4th'], [11, '11th'], [12, '12th'], [13, '13th'], [21, '21st']] as const) {
    data.extras!.in_a_row = n;
    expect(runRecapView(data).meta[1]).toBe(`${ordinal} Invented Sanctum in a row tonight`);
  }
});

it('vs_party_is_duration_weighted_over_boss_fights', () => {
  const data = runData();
  data.story.fights.push(summary('event', { target_kind: 'event_boss', local_dps: 100, last_hit: us(31),
    party: { avg_dps: 200, players: 2 } }), summary('ordinary', { target_kind: 'ordinary', local_dps: 99999 }),
  summary('missing-local', { local_dps: null }), summary('missing-party', { party: null }),
  summary('zero-span', { last_hit: us(1) }), summary('reverse-span', { last_hit: us(0) }));
  expect(runRecapView(data).tiles[1]).toMatchObject({ label: 'vs party avg', value: '−29%', detail: 'party avg 175', tone: 'down' });
  data.story.fights = [summary('missing', { party: null })];
  expect(runRecapView(data).tiles[1]).toMatchObject({ value: '—', tone: null });
});

it('usual_dps_learning_and_dash_states', () => {
  const data = runData();
  expect(runRecapView(data).tiles.map(tile => tile.key)).toEqual(['dps', 'party', 'rank', 'best-hit', 'fame']);
  expect(runRecapView(data).tiles.map(tile => tile.comparison)).toEqual(['usual', 'party', null, null, null]);
  expect(runRecapView(data).tiles[0]).toMatchObject({ value: '200', detail: '▲ 100% vs your usual', tone: 'up' });
  data.profile!.usual_run_dps = 400;
  expect(runRecapView(data).tiles[0]).toMatchObject({ detail: '▼ 50% vs your usual', tone: 'down' });
  data.profile!.usual_run_dps = null; data.profile!.run_dps_samples = 2;
  expect(runRecapView(data).tiles[0].detail).toBe('learning (2 runs)');
  data.profile = null;
  expect(runRecapView(data).tiles[0].detail).toBe('vs your usual —');
});

it('zero_percent_is_neutral', () => {
  const data = runData(); data.profile!.usual_run_dps = 200; data.story.fights[0].party!.avg_dps = 200;
  expect(runRecapView(data).tiles[0]).toMatchObject({ detail: '0% vs your usual', tone: 'neutral' });
  expect(runRecapView(data).tiles[1]).toMatchObject({ value: '0%', tone: 'neutral' });
});

it('kill_text_faster_slower_usual_and_dash', () => {
  const data = runData();
  expect(runRecapView(data).fights[0].kill).toBe('1:00 · 5 s faster than usual');
  data.story.boss_fights = [detail('boss-1', 70)];
  expect(runRecapView(data).fights[0].kill).toBe('1:10 · 5 s slower than usual');
  data.story.boss_fights = [detail('boss-1', 65.5)];
  expect(runRecapView(data).fights[0].kill).toBe('1:05 · usual 1:05');
  data.profile!.bosses[0].kill_samples = 2;
  expect(runRecapView(data).fights[0].kill).toBe('1:05 · 4 s faster than usual');
  data.baselines!.rows[0].kill_samples = 2;
  expect(runRecapView(data).fights[0].kill).toBe('1:05');
  data.story.boss_fights![0].kill = null;
  expect(runRecapView(data).fights[0].kill).toBe('—');
  data.baselines!.rows[0].kill_samples = 3;
  expect(runRecapView(data).fights[0].kill).toBe('usual 1:10');
});

it('limited_boss_details_mark_best_hit_partial', () => {
  const data = runData(); data.story.boss_fights_limited = true;
  expect(runRecapView(data).tiles[3]).toMatchObject({ label: 'Best hit', value: '1,200', detail: 'Invented Guardian', partial: true });
  expect(runRecapView(data).fightsPartial).toBe(true);
  data.story.boss_fights![0].players.push({ ...detail().players[0], biggest_hit: 9999 });
  expect(runRecapView(data).tiles[3].value).toBe('—');
});

it('legacy_buckets_omit_sparkline', () => {
  const data = runData();
  expect(runRecapView(data).fights[0].buckets).toEqual(data.story.boss_fights![0].buckets);
  delete data.story.boss_fights![0].buckets[0].others_presence;
  expect(runRecapView(data).fights[0].buckets).toBeNull();
  data.story.boss_fights = [];
  expect(runRecapView(data).fights[0].buckets).toBeNull();
});

it('unverified_self_shows_dashes', () => {
  const data = runData(); data.story.totals!.local!.is_local = false;
  expect(runRecapView(data).tiles.slice(0, 4).map(tile => tile.value)).toEqual(['—', '—', '—', '—']);
  expect(runRecapView(data).tiles[4].value).toBe('+12');
  data.story.totals!.local = null; data.story.totals!.players[0].is_local = true;
  expect(runRecapView(data).tiles[0].value).toBe('200');
  data.story.totals!.players.push({ ...data.story.totals!.players[0], player_id: 'ambiguous' });
  expect(runRecapView(data).tiles[0].value).toBe('—');
  data.story.totals = null;
  expect(runRecapView(data).tiles.map(tile => tile.value)).toEqual(['—', '—', '—', '—', '—']);
  expect(runRecapView(data).tiles[4].detail).toBeNull();
});

it('chips_order_and_text', () => {
  const data = runData();
  data.extras!.dry_streak_end = { runs: 8, had_previous_white: true };
  data.extras!.best_dps = [{ fight_id: 'boss-1', boss_type: 300, dps: 200, previous_best_dps: 100 }];
  data.extras!.pb_clear = { clear_micros: usNumber(120), previous_best_micros: usNumber(150) };
  data.extras!.milestones = [{ kind: 'new_items', item_types: [1000, 1001], more: 2 }, { kind: 'first_white' },
    { kind: 'clear_count', count: 100 }, { kind: 'first_clear' }];
  expect(runRecapView(data).chips).toEqual([
    { kind: 'streak', text: 'Ended an 8-run dry streak' }, { kind: 'best-dps', text: 'Best DPS on Invented Guardian' },
    { kind: 'pb', text: 'PB clear 2:00 · 0:30 faster' }, { kind: 'milestone', text: 'First Invented Sanctum clear' },
    { kind: 'milestone', text: '100th Invented Sanctum clear' }, { kind: 'milestone', text: 'First Invented Sanctum white' },
    { kind: 'new-items', text: 'New in collection · +2 more', itemTypes: [1000, 1001] },
  ]);
  expect(runRecapView(data)).toMatchObject({ pb: true, streakEnded: 8, fights: [{ best: true }] });
});

it('streak_note_with_and_without_previous_white', () => {
  const data = runData(); data.extras!.dry_streak_end = { runs: 23, had_previous_white: true };
  expect(runRecapView(data).streakNote).toBe('Ended a 23-run dry streak');
  data.extras!.dry_streak_end.had_previous_white = false;
  expect(runRecapView(data).streakNote).toBe('No Invented Sanctum white in 23 runs before this');
  expect(runRecapView(data).chips).toEqual([]);
});

it('indefinite_article_cases', () => {
  for (const n of [8, 11, 18, 80, 11000, 18000, 800]) expect(indefiniteArticle(n)).toBe('an');
  for (const n of [0, 1, 23, 100, 110, 180]) expect(indefiniteArticle(n)).toBe('a');
});

it('party_line_top_three_and_more', () => {
  const data = runData(), local = data.story.totals!.local!;
  data.story.totals!.players = [
    { ...local, rank: null, name: 'Synthetic D', damage: 100, is_local: false },
    { ...local, rank: 2, name: 'Synthetic B' },
    { ...local, rank: 1, name: null, share: .5, is_local: false },
    { ...local, rank: null, name: 'Synthetic C', damage: 200, is_local: false },
  ];
  const party = runRecapView(data).party;
  expect(party.top).toEqual([{ name: 'Unknown', share: '50%' }, { name: 'Synthetic B', share: '25%' }, { name: 'Synthetic C', share: '25%' }]);
  expect(party.more).toBe(1);
  expect(party.rows.map(row => row.name)).toEqual([null, 'Synthetic B', 'Synthetic C', 'Synthetic D']);
  expect(party.rows[1]).toMatchObject({ rank: 2, isLocal: true, classId: local.class_id, share: .25 });
  expect(runRecapView(runData()).party.more).toBe(0);
});

it('visit_tiles_count_announced_separately', () => {
  const data = visitData(); data.visit.announcements = [announcement('extra', us(80), 301)];
  const view = visitRecapView(data);
  expect(view.tiles.map(tile => tile.key)).toEqual(['dps', 'party', 'events', 'best-rank', 'fame']);
  expect(view.tiles.map(tile => tile.comparison)).toEqual([null, 'party', null, null, null]);
  expect(view.tiles.map(tile => [tile.label, tile.value])).toEqual([
    ['Your DPS', '200'], ['vs party avg', '+100%'], ['Events killed', '1'], ['Best rank', '#2 of 4'], ['Fame this visit', '+5'],
  ]);
  expect(view.tiles[2].detail).toBe('+1 announced');
  data.visit.announcements = []; expect(visitRecapView(data).tiles[2].detail).toBeNull();
});

it('visit_best_rank_ignores_ordinary_and_unclassified_fights', () => {
  const data = visitData();
  const ignored = [summary('mob', { target_kind: 'ordinary', local_rank: 1, player_count: 1 }),
    summary('unknown-kind', { target_kind: null, local_rank: 1 }),
    summary('unknown-type', { target_type: null, local_rank: 1 })];
  data.visit.fights.push(...ignored);
  expect(visitRecapView(data).tiles.find(tile => tile.key === 'best-rank')!.value).toBe('#2 of 4');
  data.visit.fights = ignored;
  expect(visitRecapView(data).tiles.find(tile => tile.key === 'best-rank')!.value).toBe('—');
  data.visit.fights.push(summary('quest', { target_kind: 'quest', local_rank: 3 }));
  expect(visitRecapView(data).tiles.find(tile => tile.key === 'best-rank')!.value).toBe('#3 of 4');
});

it('visit_records_mark_only_the_matching_kill_and_add_highlights', () => {
  const data = visitData();
  data.visit.best_dps = [{ fight_id: 'boss-1', boss_type: 300, dps: 200, previous_best_dps: 100 }];
  data.visit.announcements = [announcement('extra', us(80), 301)];
  const view = visitRecapView(data);
  expect(view.events.find(row => row.key === 'fight:boss-1')!.best).toBe(true);
  expect(view.events.find(row => row.state === 'announced')!.best).toBe(false);
  expect(view.chips).toEqual([{ kind: 'best-dps', text: 'Best DPS on Invented Guardian' }]);
  data.visit.best_dps = [];
  expect(visitRecapView(data).events.every(row => !row.best)).toBe(true);
  expect(visitRecapView(data).chips).toEqual([]);
});

it('visit_loot_limit_is_preserved', () => {
  const data = visitData();
  expect(visitRecapView(data).dropsPartial).toBe(false);
  data.visit.drops_limited = true;
  expect(visitRecapView(data).dropsPartial).toBe(true);
});

it('visit_events_reuse_realm_matching', () => {
  const data = visitData();
  data.visit.periods = [{ visit_id: 'a', started: '0', ended: us(65) }, { visit_id: 'b', started: us(66), ended: us(100) }];
  data.visit.announcements = [announcement('merged', us(62)), announcement('different-period', us(67)),
    announcement('away', us(65)), announcement('end-exclusive', us(100))];
  data.visit.drops = [{ ...homeSnapshot().drops[0], id: 'synthetic-bag', at: us(63), source_enemy: 300, source_estimated: false }];
  const events = visitRecapView(data).events;
  expect(events.map(row => row.key)).toEqual(['announcement:different-period', 'fight:boss-1']);
  expect(events[1]).toMatchObject({ state: 'killed', dps: 200, rank: 2, bags: [{ dropId: 'synthetic-bag' }] });
  expect(events[0]).toMatchObject({ state: 'announced', dps: null });
});

it('visit_event_rows_join_details_and_summaries_by_fight_key', () => {
  const data = visitData(); data.baselines = runData().baselines;
  data.visit.boss_fights.push(detail('second', 80), detail('missing-summary', 90));
  data.visit.fights.unshift(summary('second', { player_count: 7, local_dps: 50 }));
  data.visit.announcements = [announcement('extra', us(95), 301)];
  const before = structuredClone(data), events = visitRecapView(data).events;
  expect(events.find(row => row.key === 'fight:boss-1')).toMatchObject({ players: 4, vsParty: 1,
    kill: '1:00 · 10 s faster than usual', buckets: data.visit.boss_fights[0].buckets });
  expect(events.find(row => row.key === 'fight:second')).toMatchObject({ players: 7, vsParty: -.5,
    kill: '1:20 · 10 s slower than usual', buckets: data.visit.boss_fights[1].buckets });
  expect(events.find(row => row.key === 'fight:missing-summary')).toMatchObject({ players: null, vsParty: null,
    kill: '1:30 · 20 s slower than usual', buckets: data.visit.boss_fights[2].buckets });
  expect(events.find(row => row.key === 'announcement:extra')).toMatchObject({ state: 'announced', dps: null,
    rank: null, players: null, share: null, kill: null, vsParty: null, buckets: null });
  expect(data).toEqual(before);
});

it('visit_event_kill_usual_requires_three_samples_and_party_line_requires_recorded_buckets', () => {
  const data = visitData(); data.baselines = runData().baselines;
  data.baselines!.rows[0].kill_samples = 2;
  delete data.visit.boss_fights[0].buckets[0].others_presence;
  expect(visitRecapView(data).events[0]).toMatchObject({ kill: '1:00', buckets: null });
  data.baselines = null; data.visit.fights[0].party = null;
  expect(visitRecapView(data).events[0]).toMatchObject({ kill: '1:00', vsParty: null });
});

it('visit_events_are_newest_first_without_mutating_saved_evidence', () => {
  const data = visitData();
  data.visit.boss_fights.push(detail('latest', 90), detail('middle', 70));
  data.visit.announcements = [announcement('extra', us(80), 301)];
  const before = structuredClone(data);
  expect(visitRecapView(data).events.map(row => row.key)).toEqual([
    'fight:latest', 'announcement:extra', 'fight:middle', 'fight:boss-1',
  ]);
  expect(data).toEqual(before);
});

it('chain_label_is_inferred', () => {
  const data = runData(); data.extras!.chain = { steps: [
    { visit_id: 'a', kind: 'realm', name: 'Invented Realm', run_id: null, started: us(1), ended: us(61),
      local_dps: null, party_dps: null, bags: 2, whites: 1, item_types: [] },
    { visit_id: 'b', kind: 'castle', name: 'Invented Castle', run_id: 'synthetic-run', started: us(91), ended: us(181),
      local_dps: 200, party_dps: 100, bags: 3, whites: 0, item_types: [] },
  ] };
  expect(runRecapView(data).chain).toMatchObject({ label: 'Invented Realm → Invented Castle · inferred',
    totals: { time: '3:00', bags: 5, whites: 1 } });
  const visit = visitData(); visit.visit.chain = data.extras!.chain;
  expect(visitRecapView(visit).chain).toEqual(runRecapView(data).chain);
  data.extras!.chain.steps[1].ended = null;
  expect(runRecapView(data).chain!.totals.time).toBe('—');
});

it('extras_unavailable_keeps_only_end_time_and_saved_streak', () => {
  const data = runData(); data.extras = null;
  data.streak = { dungeon: 'Invented Sanctum', runs_ended: 10, runs_since_last_white: 8, avg_runs_per_white: null, last_white_at: null };
  expect(runRecapView(data, 'UTC')).toMatchObject({ chips: [], pb: false, chain: null, streakNote: null,
    meta: ['ended 00:03'], streakEnded: null, streak: data.streak });
});

it('visit_weights_preserve_microsecond_spans_and_rank_ties_choose_earliest', () => {
  const data = visitData(), start = 9007199254740992n;
  data.visit.fights = [summary('later', { started: us(2), first_hit: String(start), last_hit: String(start + 3n),
    local_dps: 100, party: { avg_dps: 200, players: 5 }, player_count: 6 }),
  summary('earlier', { first_hit: String(start), last_hit: String(start + 1n) }),
  summary('no-party', { local_dps: 500, first_hit: String(start), last_hit: String(start + 1n), party: null, local_rank: null })];
  const tiles = visitRecapView(data).tiles;
  expect(tiles[0].value).toBe('200'); expect(tiles[1].value).toBe('−29%'); expect(tiles[3].value).toBe('#2 of 4');
});

it('visit_title_time_and_unknown_metrics', () => {
  const data = visitData(); data.visit.periods.push({ visit_id: 'b', started: us(200), ended: us(220) });
  expect(visitRecapView(data, 'UTC', 'Live Realm')).toMatchObject({ title: 'Live Realm', timeHere: '2:00' });
  expect(visitRecapView(data, 'UTC', '—').title).toBe('Invented Realm');
  data.visit.periods[1].ended = null; data.visit.fights = []; data.visit.fame_delta = null;
  const view = visitRecapView(data);
  expect(view.timeHere).toBe('—'); expect([view.tiles[0].value, view.tiles[1].value, view.tiles[3].value, view.tiles[4].value]).toEqual(['—', '—', '—', '—']);
});

it('fight_rows_order_summary_metrics_and_best_loot_are_preserved_without_mutation', () => {
  const data = runData(); data.story.fights = [summary('later', { started: us(10), local_dps: 50, local_rank: 3 }),
    summary(), summary('ordinary', { target_kind: 'ordinary' })];
  data.story.drops = [{ ...homeSnapshot().drops[0], id: 'brown', bag_type: 0x500, bag_rank: 1 },
    { ...homeSnapshot().drops[0], id: 'white', bag_type: 0x506, bag_rank: 7 }];
  const before = structuredClone(data), view = runRecapView(data);
  expect(view.fights.map(row => row.fightId)).toEqual(['boss-1', 'later']);
  expect(view.fights[1]).toMatchObject({ dps: 50, rank: 3, players: 4, vsParty: -.5 });
  expect(view.drops).toBe(data.story.drops); expect(view.best?.drop.id).toBe('white');
  expect(data).toEqual(before);
});

it('unknown_damage_share_has_no_share_detail', () => {
  const data = runData(); data.story.totals!.local!.share = null;
  data.story.totals!.players = [data.story.totals!.local!];
  expect(runRecapView(data).tiles.find(tile => tile.key === 'rank')?.detail).toBeNull();
  expect(runRecapView(runData()).tiles.find(tile => tile.key === 'rank')?.detail).toBe('25% of damage');
});

it('sub_second_pb_gain_reads_under_one_second', () => {
  const data = runData();
  data.extras = { ...data.extras!, pb_clear: { clear_micros: usNumber(60), previous_best_micros: usNumber(60) + 400_000 } };
  expect(runRecapView(data).chips.find(chip => chip.kind === 'pb')?.text).toBe('PB clear 1:00 · under 1 s faster');
  data.extras.pb_clear = { clear_micros: usNumber(60), previous_best_micros: usNumber(65) };
  expect(runRecapView(data).chips.find(chip => chip.kind === 'pb')?.text).toBe('PB clear 1:00 · 0:05 faster');
});

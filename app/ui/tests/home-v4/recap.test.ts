import { get } from 'svelte/store';
import { expect, it } from 'vitest';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import { initialHomeMode } from '../../src/lib/home/mode';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { createRecapTracker, homeScreen, storedVisitId } from '../../src/lib/home/v4/recap';
import { homeSnapshot } from '../home/fixtures';

const live = () => {
  const snapshot = homeSnapshot();
  const local = { ...snapshot.fights[0].players[0], dps: 4321, rank: 2 };
  snapshot.run_totals = { fights: 2, kills: 1, total_damage: 9000, unattributed_damage: 0,
    players: [local, { ...local, player_id: 'synthetic-other', is_local: false, rank: 1 }], local, partial: false };
  return snapshot;
};
const windowFor = (snapshot: LiveSnapshot) => ({ ...emptyHomeLiveWindow(), streamId: snapshot.stream_id,
  visitKey: 'synthetic-visit', visitStarted: '1000000', runId: snapshot.run?.id ?? null,
  fights: snapshot.fights, drops: snapshot.drops, partial: false, reasons: [] });
const ended = (snapshot: LiveSnapshot) => ({ ...snapshot, at: '9000000', sequence: '13',
  run: { ...snapshot.run!, ended: '8000000', outcome: 'cleared' as const } });
const hub = (area_kind: 'nexus' | 'other_hub' | 'unknown' = 'nexus') =>
  homeSnapshot({ at: '10000000', area_kind, map: 'Synthetic hub', run: null, run_totals: null, fights: [], drops: [] });
function runRecap() {
  const tracker = createRecapTracker(), snapshot = live(), window = windowFor(snapshot);
  tracker.observe(snapshot, window);
  tracker.observe(ended(snapshot), window);
  return tracker;
}

it('watermark_is_the_finishing_sequence', () => {
  expect(get(runRecap())?.watermark).toBe('13');
  const tracker = createRecapTracker(), snapshot = live();
  tracker.observe(snapshot, windowFor(snapshot));
  tracker.onEvent({ stream_id: snapshot.stream_id, sequence: '14', at: '8000000', event: { run_ended: ended(snapshot).run } });
  tracker.observe({ ...hub(), sequence: '99' }, emptyHomeLiveWindow());
  expect(get(tracker)?.watermark).toBe('14');
});

it('visit_records_entered_sequence', () => {
  const tracker = createRecapTracker(), snapshot = live();
  tracker.onEvent({ stream_id: snapshot.stream_id, sequence: '20', at: '1000000',
    event: { map_entered: { at: '1000000', name: 'Synthetic realm', kind: 'realm' } } });
  tracker.onEvent({ stream_id: snapshot.stream_id, sequence: '21', at: '8000000',
    event: { map_entered: { at: '8000000', name: 'Synthetic Nexus', kind: 'nexus' } } });
  const recap = get(tracker)!;
  expect(recap).toMatchObject({ watermark: '21', source: { enteredSequence: '20' } });
  if (recap.source.kind !== 'visit') throw new Error('Expected visit');
  expect(storedVisitId(recap.source)).toBe(`${snapshot.stream_id}:visit:20`);
  expect(storedVisitId({ ...recap.source, enteredSequence: null })).toBeNull();
  const fallback = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', run: null });
  fallback.observe(realm, windowFor(realm)); fallback.observe(hub(), emptyHomeLiveWindow());
  expect(get(fallback)?.source).toMatchObject({ enteredSequence: null });
});

it('realm_reentry_keeps_its_observed_sequence_when_the_window_restores_an_older_visit_key', () => {
  const tracker = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', run: null });
  tracker.onEvent({ stream_id: realm.stream_id, sequence: '20', at: '6000000',
    event: { map_entered: { at: '6000000', name: 'Synthetic realm', kind: 'realm' } } });
  const window = { ...windowFor(realm), visitKey: `${realm.stream_id}:4`, observedAt: '6000000',
    realmPeriods: [{ started: '1000000', ended: '2000000', fame: null }, { started: '6000000', ended: null, fame: null }] };
  tracker.observe(null, window);
  tracker.observe({ ...realm, sequence: '20', at: '6000000' }, window);
  tracker.onEvent({ stream_id: realm.stream_id, sequence: '21', at: '8000000',
    event: { map_entered: { at: '8000000', name: 'Synthetic Nexus', kind: 'nexus' } } });
  const source = get(tracker)!.source;
  expect(source).toMatchObject({ visitKey: `${realm.stream_id}:4`, enteredSequence: '20' });
  if (source.kind !== 'visit') throw new Error('Expected visit');
  expect(storedVisitId(source)).toBe(`${realm.stream_id}:visit:20`);
});

it('run_end_enters_recap', () => {
  const tracker = runRecap();
  expect(get(tracker)).toMatchObject({ source: { kind: 'run', streamId: live().stream_id, runId: live().run!.id },
    summary: { kind: 'run', title: 'Synthetic Sanctum', outcome: 'cleared', startedAt: '1000000', endedAt: '8000000',
      localDps: 4321, rank: 2, players: 2, runId: live().run!.id, best: { drop: { id: live().drops[0].id } } } });
});

it.each(['nexus', 'other_hub', 'unknown'] as const)('area_exit_to_%s_uses_the_last_live_window', area => {
  const tracker = createRecapTracker(), snapshot = live();
  const drop = { ...snapshot.drops[0], id: 'synthetic-window-bag', bag_rank: 255 };
  tracker.observe(snapshot, { ...windowFor(snapshot), drops: [drop] });
  tracker.observe(hub(area), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ title: snapshot.run!.dungeon, localDps: 4321, rank: 2,
    outcome: null, endedAt: '10000000', best: { drop: { id: drop.id } } });
});

it('recap_holds_through_nexus_vault_and_unknown', () => {
  const tracker = runRecap(), recap = get(tracker);
  for (const area of ['nexus', 'other_hub', 'unknown'] as const) {
    tracker.observe(hub(area), emptyHomeLiveWindow());
    expect(get(tracker)).toBe(recap);
  }
});

it.each([['dungeon', false], ['realm', false], ['dungeon', true], ['realm', true]] as const)(
  'next_area_hides_banner_and_closes_recap_but_keeps_it_available: %s open=%s', (area_kind, open) => {
  const tracker = runRecap(), previous = get(tracker);
  if (open) tracker.presentation.open();
  expect(get(tracker.presentation)).toEqual({ banner: !open, open });
  const next = homeSnapshot({ area_kind, run: area_kind === 'realm' ? null : { ...live().run!, id: 'synthetic-next' } });
  tracker.observe(next, windowFor(next));
  expect(get(tracker)).toBe(previous);
  expect(get(tracker.presentation)).toEqual({ banner: false, open: false });
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary.kind).toBe(area_kind === 'realm' ? 'visit' : 'run');
});

it('realm_exit_into_castle_remembers_visit_without_banner', () => {
  const tracker = createRecapTracker();
  const realm = homeSnapshot({ area_kind: 'realm', map: 'Synthetic realm', run: null });
  tracker.observe(realm, windowFor(realm));
  for (const title of ["Oryx's Castle", "Oryx's Chamber", "Oryx's Sanctuary"]) {
    const next = homeSnapshot({ run: { ...live().run!, id: title, dungeon: title } });
    tracker.observe(next, windowFor(next));
    expect(get(tracker)).toMatchObject({
      source: { kind: 'visit', streamId: realm.stream_id, visitKey: 'synthetic-visit' },
      summary: { kind: 'visit', title: 'Synthetic realm', endedAt: next.at },
    });
    expect(get(tracker.presentation).banner).toBe(false);
  }
});

it('realm_exit_to_nexus_enters_visit_recap', () => {
  const tracker = createRecapTracker();
  const realm = homeSnapshot({ area_kind: 'realm', map: 'Synthetic realm', run: null, run_totals: null });
  tracker.observe(realm, windowFor(realm));
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)).toMatchObject({ source: { kind: 'visit', streamId: realm.stream_id, visitKey: 'synthetic-visit' },
    summary: { kind: 'visit', title: 'Synthetic realm', outcome: null, startedAt: '1000000', endedAt: '10000000',
      localDps: null, rank: null, runId: null } });
});

it('back_keeps_recap', () => {
  const tracker = runRecap();
  tracker.presentation.open();
  tracker.dismiss();
  expect(get(tracker.presentation).open).toBe(false);
  tracker.observe(ended(live()), windowFor(live()));
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary.title).toBe('Synthetic Sanctum');
});

it('capture_drop_keeps_recap', () => {
  const tracker = runRecap(), recap = get(tracker);
  tracker.observe(null, emptyHomeLiveWindow());
  for (const state of ['Recovering', 'WaitingForGame', 'Stopped', 'Failed'] as const) {
    tracker.observe({ ...live(), capture: { ...live().capture, state } }, emptyHomeLiveWindow());
    expect(get(tracker)).toBe(recap);
  }
});

it('capture_loss_does_not_end_the_remembered_run', () => {
  const tracker = createRecapTracker(), snapshot = live();
  tracker.observe(snapshot, windowFor(snapshot));
  tracker.observe({ ...hub(), capture: { ...snapshot.capture, state: 'Recovering' } }, emptyHomeLiveWindow());
  expect(get(tracker)).toBeNull();
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary.localDps).toBe(4321);
});

it('new_stream_clears_recap', () => {
  const tracker = runRecap();
  const next = { ...hub(), stream_id: 'synthetic-new-stream', capture: { ...hub().capture, state: 'Recovering' as const } };
  tracker.observe(next, emptyHomeLiveWindow());
  expect(get(tracker)).toBeNull();
  expect(get(tracker.presentation)).toEqual({ banner: false, open: false });
  tracker.observe({ ...next, capture: hub().capture }, emptyHomeLiveWindow());
  expect(get(tracker)).toBeNull();
});

it('home_screen_prefers_live_over_recap', () => {
  const recap = get(runRecap());
  for (const mode of ['dungeon', 'realm'] as const) expect(homeScreen({ ...initialHomeMode(), mode }, recap)).toBe(mode);
  expect(homeScreen(initialHomeMode(), recap)).toBe('nexus');
  for (const mode of ['nexus', 'dungeon', 'realm'] as const) {
    expect(homeScreen({ ...initialHomeMode(), mode }, recap, true)).toBe('recap');
    expect(homeScreen({ ...initialHomeMode(), mode }, recap, false)).toBe(mode);
  }
  expect(homeScreen(initialHomeMode(), null)).toBe('nexus');
});

it('missing_identity_and_run_totals_stay_unknown', () => {
  const tracker = createRecapTracker(), snapshot = live();
  snapshot.character = null; snapshot.run_totals = null;
  tracker.observe(snapshot, windowFor(snapshot));
  tracker.observe(ended(snapshot), windowFor(snapshot));
  expect(get(tracker)?.summary).toMatchObject({ localDps: null, rank: null, players: null });
});

it('terminal_totals_override_the_last_live_totals', () => {
  const tracker = createRecapTracker(), snapshot = live(), final = ended(snapshot);
  final.run_totals = { ...snapshot.run_totals!, local: { ...snapshot.run_totals!.local!, dps: 5555, rank: 1 } };
  tracker.observe(snapshot, windowFor(snapshot));
  tracker.observe(final, windowFor(final));
  expect(get(tracker)?.summary).toMatchObject({ localDps: 5555, rank: 1, endedAt: '8000000' });
});

it('new_area_window_cannot_overwrite_the_last_realm_window', () => {
  const tracker = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', map: 'Synthetic realm', run: null });
  const window = windowFor(realm);
  window.drops = realm.drops.map(drop => ({ ...drop, run_id: null }));
  tracker.observe(realm, window);
  tracker.observe(realm, { ...window, visitKey: 'synthetic-hub', visitStarted: '9000000', drops: [] });
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)).toMatchObject({ source: { visitKey: 'synthetic-visit' },
    summary: { startedAt: '1000000', best: { drop: { id: window.drops[0].id } } } });
});

it('unknown_realm_entry_does_not_invent_a_started_time', () => {
  const tracker = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', map: null, run: null });
  tracker.observe(realm, emptyHomeLiveWindow());
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ title: '—', startedAt: '', endedAt: '10000000' });
});

it('batched_realm_entry_accepts_its_window_after_the_snapshot', () => {
  const tracker = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', map: 'Synthetic realm', run: null });
  const oldHub = { ...windowFor(realm), visitKey: 'synthetic-hub', visitStarted: '1000000', observedAt: '4000000' };
  const entry = { ...windowFor(realm), visitStarted: realm.at, observedAt: realm.at };
  // connectShell's snapshot callback can run before its batched window emission.
  tracker.observe(realm, oldHub);
  tracker.observe(realm, entry);
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)).toMatchObject({ source: { visitKey: 'synthetic-visit' }, summary: { startedAt: realm.at } });
});

it('partial_windows_do_not_claim_final_fights_or_best_loot', () => {
  const tracker = createRecapTracker(), snapshot = live();
  tracker.observe(snapshot, { ...windowFor(snapshot), partial: true });
  tracker.observe(ended(snapshot), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ fights: null, best: null, outcome: 'cleared' });
});

it('hidden_snapshot_only_totals_are_unknown_but_ordered_window_fights_and_loot_remain', () => {
  const tracker = createRecapTracker(), snapshot = live(), window = windowFor(snapshot);
  tracker.observe(snapshot, window);
  tracker.setVisible(false);
  tracker.observe(snapshot, window);
  tracker.setVisible(true);
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ localDps: null, rank: null, players: null,
    fights: snapshot.fights.map(fight => ({ id: fight.id })), best: { drop: { id: snapshot.drops[0].id } } });
});

it('restored_terminal_snapshot_can_establish_totals_after_a_hidden_boundary', () => {
  const tracker = createRecapTracker(), snapshot = live(), window = windowFor(snapshot);
  tracker.observe(snapshot, window);
  tracker.setVisible(false); tracker.setVisible(true);
  tracker.observe(ended(snapshot), window);
  expect(get(tracker)?.summary).toMatchObject({ localDps: 4321, rank: 2, players: 2 });
});

it('a_fresh_same_area_snapshot_replaces_stale_totals_before_the_area_exit', () => {
  const tracker = createRecapTracker(), snapshot = live(), window = windowFor(snapshot);
  tracker.observe(snapshot, window);
  tracker.setVisible(false); tracker.setVisible(true);
  tracker.observe({ ...snapshot, sequence: '13', at: '7000000' }, window);
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ localDps: 4321, rank: 2, players: 2 });
});

it('terminal_snapshot_without_totals_does_not_fall_back_to_pre_hide_totals', () => {
  const tracker = createRecapTracker(), snapshot = live(), window = windowFor(snapshot);
  tracker.observe(snapshot, window);
  tracker.setVisible(false); tracker.setVisible(true);
  tracker.observe({ ...ended(snapshot), run_totals: null }, window);
  expect(get(tracker)?.summary).toMatchObject({ localDps: null, rank: null, players: null, outcome: 'cleared' });
});

it('run_identity_fields_survive_a_hide_in_the_same_area', () => {
  const tracker = createRecapTracker(), snapshot = live();
  tracker.observe(snapshot, windowFor(snapshot));
  tracker.setVisible(false); tracker.setVisible(true);
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ title: snapshot.run!.dungeon, startedAt: snapshot.run!.started, outcome: null,
    runId: snapshot.run!.id, fights: snapshot.fights.map(fight => ({ id: fight.id })) });
});

it('realm_identity_fields_survive_a_hide_in_the_same_area', () => {
  const tracker = createRecapTracker(), snapshot = homeSnapshot({ area_kind: 'realm', map: 'Synthetic realm', run: null });
  const window = windowFor(snapshot);
  tracker.observe(snapshot, window);
  tracker.setVisible(false); tracker.setVisible(true);
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ title: 'Synthetic realm', startedAt: window.visitStarted, localDps: null,
    rank: null, players: null, fights: snapshot.fights.map(fight => ({ id: fight.id })) });
});

it('partial_visit_rows_preserve_the_observed_map_entry_time', () => {
  const tracker = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', run: null });
  tracker.observe(realm, { ...windowFor(realm), partial: true });
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ startedAt: '1000000', fights: null, best: null });
});

it('the_first_non_matching_window_ends_a_visit_before_a_delayed_snapshot', () => {
  const tracker = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', run: null });
  tracker.observe(realm, windowFor(realm));
  tracker.setVisible(false);
  tracker.observe(realm, { ...windowFor(realm), visitKey: 'synthetic-nexus', visitStarted: '9000000', fights: [], drops: [] });
  tracker.setVisible(true);
  tracker.observe({ ...hub(), at: '1809000000' }, emptyHomeLiveWindow());
  expect(get(tracker)?.summary.endedAt).toBe('9000000');
});

it('a_restored_snapshot_cannot_override_the_departure_in_its_non_matching_window', () => {
  const tracker = createRecapTracker(), realm = homeSnapshot({ area_kind: 'realm', run: null });
  tracker.observe(realm, windowFor(realm));
  tracker.setVisible(false); tracker.setVisible(true);
  tracker.observe({ ...hub(), at: '1809000000' }, { ...windowFor(realm), visitKey: 'synthetic-nexus',
    visitStarted: '9000000', observedAt: '1809000000', fights: [], drops: [] });
  expect(get(tracker)?.summary.endedAt).toBe('9000000');
});

it('a_new_run_window_cannot_inherit_the_previous_runs_identity_or_totals', () => {
  const tracker = createRecapTracker(), snapshot = live(), window = windowFor(snapshot);
  tracker.observe(snapshot, window);
  tracker.setVisible(false);
  tracker.observe(snapshot, { ...window, runId: 'synthetic-next', visitKey: 'synthetic-next-visit',
    visitStarted: '7000000', observedAt: '7000000', fights: [], drops: [] });
  tracker.setVisible(true);
  tracker.observe(hub(), emptyHomeLiveWindow());
  expect(get(tracker)?.summary).toMatchObject({ title: '—', startedAt: '', localDps: null, rank: null,
    players: null, runId: 'synthetic-next' });
});

it('a_hidden_completed_run_needs_a_terminal_snapshot_to_restore_its_totals', () => {
  const tracker = createRecapTracker(), snapshot = live();
  tracker.observe(snapshot, windowFor(snapshot));
  tracker.setVisible(false);
  const run = { ...snapshot.run!, id: 'synthetic-hidden-run' };
  const window = { ...windowFor(snapshot), runId: run.id };
  tracker.onEvent({ stream_id: snapshot.stream_id, sequence: '13', at: '6000000', event: { run_started: run } });
  tracker.observe(snapshot, window);
  tracker.onEvent({ stream_id: snapshot.stream_id, sequence: '14', at: '8000000',
    event: { run_ended: { ...run, ended: '8000000', outcome: 'cleared' } } });
  tracker.observe(snapshot, window);
  tracker.setVisible(true);
  tracker.observe({ ...snapshot, sequence: '15', at: '9000000', run }, window);
  expect(get(tracker)?.summary).toMatchObject({ outcome: 'cleared', endedAt: '8000000', localDps: null, rank: null, players: null });
  tracker.observe({ ...ended(snapshot), sequence: '16', run: { ...run, ended: '8000000', outcome: 'cleared' } }, window);
  expect(get(tracker)?.summary).toMatchObject({ localDps: 4321, rank: 2, players: 2 });
});

it.each(['run_started', 'dungeon', 'realm'] as const)('ordered_%s_entry_closes_open_recap_without_forgetting_it', entry => {
  const tracker = runRecap(), previous = get(tracker), snapshot = live();
  tracker.presentation.open();
  expect(get(tracker.presentation).open).toBe(true);
  tracker.onEvent({ stream_id: snapshot.stream_id, sequence: '14', at: '10000000', event: entry === 'run_started'
    ? { run_started: { ...snapshot.run!, id: 'synthetic-next', started: '10000000' } }
    : { map_entered: { at: '10000000', name: 'Synthetic next area', kind: entry } } });
  expect(get(tracker.presentation)).toEqual({ banner: false, open: false });
  expect(get(tracker)).toBe(previous);
  tracker.presentation.open();
  expect(get(tracker.presentation).open).toBe(true);
});

it('dungeon_snapshot_without_run_closes_open_recap_without_forgetting_it', () => {
  const tracker = runRecap(), previous = get(tracker);
  tracker.presentation.open();
  tracker.observe(homeSnapshot({ area_kind: 'dungeon', run: null }), emptyHomeLiveWindow());
  expect(get(tracker.presentation)).toEqual({ banner: false, open: false });
  expect(get(tracker)).toBe(previous);
});

it.each(['dungeon', 'realm'] as const)('direct_realm_to_%s_entry_remembers_the_finished_visit_without_a_banner', kind => {
  const tracker = runRecap(), realm = homeSnapshot({ area_kind: 'realm', map: 'Synthetic latest realm', run: null });
  tracker.observe(realm, windowFor(realm));
  tracker.presentation.open();
  tracker.onEvent({ stream_id: realm.stream_id, sequence: '14', at: '10000000',
    event: { map_entered: { at: '10000000', name: 'Synthetic next area', kind } } });
  expect(get(tracker)).toMatchObject({ watermark: '14',
    source: { kind: 'visit', streamId: realm.stream_id, visitKey: 'synthetic-visit' },
    summary: { kind: 'visit', title: 'Synthetic latest realm', endedAt: '10000000' } });
  expect(get(tracker.presentation)).toEqual({ banner: false, open: false });
});

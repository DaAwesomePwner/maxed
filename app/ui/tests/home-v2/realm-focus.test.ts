import { setDebugMode } from '../../src/lib/debug/debug';
import { readFileSync } from 'node:fs';
import { screen } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
afterEach(() => setDebugMode(false));
import type { FightView } from '../../src/lib/bindings/FightView';
import { REALM_FOCUS_IDLE_US, selectBossFight, snapshotFight } from '../../src/lib/shell/rail';
import { fight, realmSnapshot, mountV2, flushHomeRender, catalog } from './fixtures';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';

const context = (at: string) => ({ area: 'realm' as const, at, character: realmSnapshot().character });
const quest = (id: string, overrides: Partial<FightView> = {}) => fight(id, {
  run_id: null, target_kind: 'quest', started: '1000000', last_hit: '1000000', local_last_hit: null,
  is_current_quest: false, ...overrides,
});

it('follows the fixture-derived sequential realm engagement scenario', () => {
  // This fixture starts at a reconnect. Verified UI identity is explicit test
  // context, not a claim that the pcap proves character-select identity.
  const fixture = JSON.parse(readFileSync('../../tests/fixtures/scrubbed/maxed-realm-focus/expected.json', 'utf8'));
  const observations = fixture.boss_tier_targets as { name: string; target_id: number; target_type: number;
    observed_target_kind: FightView['target_kind']; started_us: string; last_hit_us: string; local_damage: number }[];
  const timings = JSON.parse(readFileSync('../../tests/fixtures/scrubbed/maxed-realm-focus/focus-hit-observations.json', 'utf8')) as
    { target_id: number; first_published_local_hit_us: string | null }[];
  const rows = observations.map(row => quest(row.name, { target_id: row.target_id, target_type: row.target_type,
    target_kind: row.observed_target_kind, started: row.started_us, last_hit: row.last_hit_us,
    local_last_hit: timings.find(timing => timing.target_id === row.target_id)!.first_published_local_hit_us,
    players: row.local_damage > 0 ? [{ ...fight('template').players[0], damage: row.local_damage }] : [],
  }));
  const [maze, barnacle, sword, legion] = rows;
  let focus = selectBossFight([{ ...maze, last_hit: maze.started, is_current_quest: true }], null, context(maze.started));
  expect(focus?.id).toBe('Maze Minotaur');
  focus = selectBossFight([maze, { ...barnacle, last_hit: barnacle.local_last_hit! }], focus!.id, context(barnacle.local_last_hit!));
  expect(focus?.id).toBe('Man-eating Barnacle');
  focus = selectBossFight([maze, barnacle, { ...sword, last_hit: legion.local_last_hit!, is_current_quest: true },
    { ...legion, last_hit: legion.local_last_hit! }], focus!.id, context(legion.local_last_hit!));
  expect(focus?.id).toBe('Legion General');
});

it('prefers the current quest unless another target has recent verified local hits, regardless of tier', () => {
  const older = quest('older-boss', { target_kind: 'boss', last_hit: '9000000' });
  const arrow = quest('arrow', { is_current_quest: true, last_hit: '10000000' });
  const engaged = quest('engaged', { local_last_hit: '8000000', last_hit: '8000000' });
  expect(selectBossFight([older, arrow, engaged], older.id, context('10000000'))?.id).toBe(engaged.id);
  expect(selectBossFight([older, arrow, engaged], engaged.id, { ...context('10000000'), character: null })?.id).toBe(arrow.id);
  const unverified = { ...context('10000000').character!, identity_verified: false };
  expect(selectBossFight([arrow, engaged], engaged.id, { ...context('10000000'), character: unverified })?.id).toBe(arrow.id);
  const mismatched = { ...engaged, players: [{ ...engaged.players[0], player_id: 'another-character' }] };
  expect(selectBossFight([arrow, mismatched], engaged.id, context('10000000'))?.id).toBe(arrow.id);
  const duplicate = { ...engaged, players: [engaged.players[0], engaged.players[0]] };
  expect(selectBossFight([arrow, duplicate], engaged.id, context('10000000'))?.id).toBe(arrow.id);
});

it('does not let bystander hits extend local engagement or flicker between active bosses', () => {
  let previous = quest('first', { target_kind: 'event_boss', local_last_hit: '1000000' });
  const second = quest('second', { target_kind: 'boss' });
  for (const last_hit of ['2000000', '3000000', '4000000']) {
    expect(selectBossFight([{ ...previous, local_last_hit: null }, { ...second, last_hit }], previous.id, context(last_hit))?.id).toBe(previous.id);
    expect(selectBossFight([previous, { ...second, last_hit }], previous.id, context(last_hit))?.id).toBe(previous.id);
    previous = { ...previous, last_hit };
  }
  const arrow = { ...second, is_current_quest: true, last_hit: '11000000' };
  previous = { ...previous, last_hit: '11000000' };
  expect(selectBossFight([previous, arrow], previous.id, context('11000000'))?.id).toBe(arrow.id);
});

it('idle and killed fights stop holding focus but stay shown until another boss engages', () => {
  const first = quest('first', { local_last_hit: '1000000' });
  const idleAt = (1_000_000n + REALM_FOCUS_IDLE_US).toString();
  const snapshot = { ...realmSnapshot(), fights: [first], at: idleAt };
  expect(snapshotFight(snapshot, first.id)?.id).toBe(first.id);
  expect(snapshotFight(snapshot, null)).toBeNull();
  const killed = { ...first, kill: { at: '2000000', evidence: 'hp_zero_removed' as const, provisional: false } };
  const next = quest('next', { last_hit: '2000000' });
  expect(selectBossFight([killed, next], first.id, context('2000000'))?.id).toBe(next.id);
  expect(selectBossFight([killed], first.id, context('2000000'))?.id).toBe(killed.id);
  // A retained fight never outranks any active boss-tier target.
  const bystander = quest('bystander', { last_hit: idleAt });
  expect(selectBossFight([first, bystander], first.id, context(idleAt))?.id).toBe(bystander.id);
});

it('labels the retained realm fight as ended or last rather than live', async () => {
  const first = quest('first', { target_type: 301, last_hit: '2000000', is_current_quest: true });
  const snapshot = { ...realmSnapshot(), at: '2000000', fights: [first] };
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'Retained boss')] });
  await flushHomeRender();
  const heading = () => screen.getByRole('main').querySelector('.fight-target')?.closest('section')?.querySelector('h3')?.textContent;
  expect(heading()).toBe('LIVE FIGHT');
  page.publish({ ...snapshot, at: '13000000' });
  await flushHomeRender();
  expect(heading()).toBe('LAST FIGHT');
  page.publish({ ...snapshot, at: '13000000', fights: [{ ...first, kill: { at: '12000000', evidence: 'hp_zero_removed', provisional: false } }] });
  await flushHomeRender();
  expect(heading()).toBe('LAST FIGHT');
});

it('shared Home focus follows arrow-only updates and expires on snapshot clock progress', () => {
  let focus: string | null | undefined;
  const live = createHomeLiveWindow(window => { focus = window.focusedFightId; });
  const first = quest('first', { is_current_quest: true });
  const second = quest('second');
  const snapshot = { ...realmSnapshot(), at: '2000000', fights: [first, second] };
  live.snapshot(snapshot);
  expect(focus).toBe(first.id);
  live.snapshot({ ...snapshot, sequence: '13', fights: [{ ...first, is_current_quest: false }, { ...second, is_current_quest: true }] });
  expect(focus).toBe(second.id);
  live.snapshot({ ...snapshot, sequence: '14', at: '11000000' });
  expect(focus).toBe(second.id); // idle, retained until another boss engages
  live.dispose();
});

it('Home and the character rail switch together when local engagement moves past an open boss', async () => {
  setDebugMode(true);
  const first = quest('first', { target_type: 301, target_kind: 'event_boss', last_hit: '2000000' });
  const next = quest('next', { target_type: 302, is_current_quest: true, last_hit: '3000000', local_last_hit: '3000000' });
  const snapshot = { ...realmSnapshot(), at: '2000000', fights: [first] };
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'Former boss'), catalog(302, 'Engaged quest')] });
  await flushHomeRender();
  const target = () => screen.getByRole('main').querySelector('.fight-target')?.textContent;
  const rail = () => screen.getByRole('complementary', { name: 'Character' }).querySelector('.target')?.textContent;
  expect(target()).toContain('Former boss');
  expect(rail()).toContain('Former boss');
  page.publish({ ...snapshot, at: '3000000', fights: [first, next] });
  await flushHomeRender();
  expect(target()).toContain('Engaged quest');
  expect(rail()).toContain('Engaged quest');
  page.publish({ ...snapshot, at: '13000000', fights: [first, next] });
  await flushHomeRender();
  expect(target()).toContain('Engaged quest');
  expect(rail()).toContain('Engaged quest');
});


it('newer quest-only and local-hit events outrank a stale snapshot, while later snapshots win', () => {
  let window = emptyHomeLiveWindow();
  const live = createHomeLiveWindow(value => { window = value; });
  const first = quest('first', { is_current_quest: true, local_last_hit: null });
  const second = quest('second');
  const snapshot = { ...realmSnapshot(), sequence: '10', at: '2000000', fights: [first, second] };
  live.snapshot(snapshot);
  live.event({ stream_id: snapshot.stream_id, sequence: '11', at: '2000000', event: {
    map_entered: { name: snapshot.map!, kind: 'realm', at: '0' },
  } });
  live.snapshot({ ...snapshot, sequence: '12' });
  live.event({ stream_id: snapshot.stream_id, sequence: '13', at: '3000000', event: { fight_updated: { ...first, is_current_quest: false } } });
  live.event({ stream_id: snapshot.stream_id, sequence: '14', at: '3000000', event: { fight_updated: { ...second, is_current_quest: true } } });
  expect(window.focusedFightId).toBe(second.id);
  expect(snapshotFight(snapshot, second.id, window)?.id).toBe(second.id);
  // Event-time local engagement is newer than snapshot.at and beats its old arrow.
  live.event({ stream_id: snapshot.stream_id, sequence: '15', at: '4000000', event: { fight_updated: { ...first, is_current_quest: false, local_last_hit: '4000000', last_hit: '4000000' } } });
  expect(snapshotFight(snapshot, first.id, window)?.id).toBe(first.id);
  // An independently newer snapshot must not be overwritten by retained events.
  const newer = { ...snapshot, sequence: '16', at: '5000000', fights: [
    { ...first, is_current_quest: false, local_last_hit: null }, { ...second, is_current_quest: true, last_hit: '5000000' },
  ] };
  expect(snapshotFight(newer, first.id, window)?.id).toBe(second.id);
  live.snapshot(newer);
  expect(window.focusedFightId).toBe(second.id);
  live.dispose();
});


it('future hit timestamps cannot advance the observation clock or steal focus', () => {
  const current = quest('current', { last_hit: '2000000', is_current_quest: true });
  const future = quest('future', { last_hit: '999999999', local_last_hit: '999999999' });
  expect(selectBossFight([current, future], current.id, context('3000000'))?.id).toBe(current.id);
});

it('newer bounded event rows refresh focus fields without truncating snapshot players', () => {
  let window = emptyHomeLiveWindow();
  const live = createHomeLiveWindow(value => { window = value; });
  const template = fight('template').players[0];
  const players = Array.from({ length: 8 }, (_, i) => ({ ...template, player_id: i === 0 ? template.player_id : `other-${i}`,
    is_local: i === 0, damage: 100 - i }));
  const boss = quest('boss', { players, last_hit: '2000000' });
  const other = quest('other', { last_hit: '2000000' });
  const snapshot = { ...realmSnapshot(), sequence: '10', at: '2000000', fights: [boss, other] };
  live.snapshot(snapshot);
  live.event({ stream_id: snapshot.stream_id, sequence: '11', at: '2000000', event: { map_entered: { name: snapshot.map!, kind: 'realm', at: '0' } } });
  live.snapshot({ ...snapshot, sequence: '12' });
  live.event({ stream_id: snapshot.stream_id, sequence: '13', at: '3000000', event: { fight_updated: { ...boss, is_current_quest: true, last_hit: '3000000' } } });
  expect(window.fights.find(row => row.id === boss.id)!.players.length).toBeLessThan(8);
  const shown = snapshotFight(snapshot, boss.id, window)!;
  expect(shown.id).toBe(boss.id);
  expect(shown.is_current_quest).toBe(true);
  expect(shown.last_hit).toBe('3000000');
  expect(shown.players).toHaveLength(8);
  live.dispose();
});

it('focuses the later local receipt when realm targets share the same combat tick', () => {
  const earlier = quest('a-earlier', { first_hit: '1000000', last_hit: '1000000', local_last_hit: '1123456' });
  const later = quest('z-later', { first_hit: '1000000', last_hit: '1000000', local_last_hit: '1234567' });
  for (const fights of [[earlier, later], [later, earlier]]) {
    expect(selectBossFight(fights, earlier.id, context('1300000'))?.id).toBe(later.id);
  }
});

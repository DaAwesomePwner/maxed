import { setDebugMode } from '../../src/lib/debug/debug';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
afterEach(() => setDebugMode(false));
import { selectBossFight, snapshotFight } from '../../src/lib/shell/rail';
import { dungeonView } from '../../src/lib/home/dungeon';
import { railView } from '../../src/lib/shell/rail';
import { realmView } from '../../src/lib/home/realm';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import RealmHome from '../../src/lib/home/RealmHome.svelte';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import { fight, homeSnapshot, realmSnapshot, mountV2, flushHomeRender, catalog } from './fixtures';

it('ignores alternating ordinary mobs and unknown targets, including prior focus', () => {
  const boss = fight('boss');
  for (const last_hit of ['6000000', '8000000']) {
    const mobs = [fight('mob-a', { target_kind: 'ordinary', last_hit }),
      fight('mob-b', { target_kind: 'ordinary', last_hit: '7000000' }),
      fight('unknown-kind', { target_kind: null }), fight('unknown-type', { target_type: null })];
    expect(selectBossFight(mobs)).toBeNull();
    expect(selectBossFight(mobs, 'unknown-type')).toBeNull();
    expect(selectBossFight([...mobs, boss], boss.id)?.id).toBe(boss.id);
  }
});

it('keeps an open dungeon boss through alternating boss-tier hits, then keeps its ended result', () => {
  const boss = fight('boss', { target_kind: 'quest' });
  for (const last_hit of ['9000000', '3000000', '10000000']) {
    expect(selectBossFight([fight('other', { target_kind: 'boss', last_hit }), boss], boss.id)?.id).toBe(boss.id);
  }
  const ended = { ...boss, ended: '11000000' };
  expect(selectBossFight([ended, fight('mob', { target_kind: 'ordinary' })], boss.id)).toBe(ended);
  expect(selectBossFight([ended])).toBe(ended);
});

it.each(['quest', 'miniboss'] as const)('switches once after the boss ends when a %s engages', target_kind => {
  const ended = fight('ended', { ended: '7000000' });
  const next = fight('next', { target_kind, started: '8000000' });
  expect(selectBossFight([ended, next], ended.id)).toBe(next);
  expect(selectBossFight([ended, next, fight('newer', { last_hit: '10000000' })], next.id)).toBe(next);
});

it('orders initial candidates by tier, exact hit/start timestamp, then id without mutating input', () => {
  const rows = [fight('quest', { target_kind: 'quest', last_hit: '99999999999999999' }),
    fight('mini', { target_kind: 'miniboss' }), fight('z', { target_kind: 'boss', last_hit: '9007199254740993' }),
    fight('a', { target_kind: 'event_boss', started: '9007199254740993', last_hit: null }),
    fight('older', { target_kind: 'boss', last_hit: '9007199254740992' })];
  const copy = structuredClone(rows);
  expect(selectBossFight(rows)?.id).toBe('a');
  expect(rows).toEqual(copy);
  expect(selectBossFight(rows.slice(0, 2))?.id).toBe('mini');
});

it('selects realm quest fights only within the session and observed visit, retaining ended window rows', () => {
  const snapshot = realmSnapshot();
  const quest = fight('quest', { run_id: null, target_kind: 'quest', started: '6000001', ended: '7000000' });
  const window = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, visitKey: 'visit', visitStarted: '6000000', fights: [quest] };
  snapshot.fights = [fight('ordinary', { run_id: null, target_kind: 'ordinary', started: '6000001' }),
    fight('prior-visit', { run_id: null, started: '5000000' }),
    fight('foreign', { run_id: null, session_id: 'other', started: '6000001' })];
  expect(realmView(snapshot, window).fight?.id).toBe(quest.id);
  expect(snapshotFight(snapshot, quest.id, window)?.id).toBe(quest.id);
  expect(snapshotFight(snapshot, quest.id, { ...window, visitStarted: '8000000', fights: [] })).toBeNull();
  expect(snapshotFight({ ...snapshot, area_kind: 'nexus' }, quest.id, window)).toBeNull();
});

it('counts realm bosses, events, minibosses and quests with kill evidence and preserves announcements', () => {
  const snapshot = realmSnapshot();
  const kills = (['boss', 'event_boss', 'miniboss', 'quest'] as const).map(target_kind => fight(target_kind, {
    run_id: null, target_kind, started: '6000001', ended: '7000000',
    kill: { at: '7000000', evidence: 'hp_zero_removed', provisional: false },
  }));
  const window = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, visitKey: 'visit', visitStarted: '6000000', fights: [
    ...kills, { ...kills[0], id: 'no-kill', kill: null }, { ...kills[0], id: 'ordinary', target_kind: 'ordinary' as const },
    { ...kills[0], id: 'announcement-fight', kill: { ...kills[0].kill!, evidence: 'realm_announcement' as const } },
  ], events: [{ id: 'announcement', session_id: snapshot.session!.id, run_id: null, at: '7000000', provisional: true,
    kind: { type: 'realm_event_killed' as const, boss_key: 'Synthetic boss' } }] };
  expect(realmView(snapshot, window).kills.map(row => row.id).sort()).toEqual([
    'announcement:announcement', 'fight:boss', 'fight:event_boss', 'fight:miniboss', 'fight:quest',
  ]);
});

it.each(['realm', 'dungeon'] as const)('%s displays unattributed damage without ranking it and marks ended fights', async mode => {
  const snapshot = mode === 'realm' ? realmSnapshot() : homeSnapshot();
  const boss = fight('boss', { run_id: snapshot.run?.id ?? null, ended: mode === 'realm' ? '7000000' : null, unattributed_damage: 74000, target_hp: null });
  boss.players[0] = { ...boss.players[0], rank: 7, share: 0.123 };
  snapshot.fights = [boss];
  const page = mode === 'realm' ? render(RealmHome, { snapshot }) : render(DungeonHome, { snapshot, stale: false });
  expect(page.container.querySelector('[data-unattributed]')?.textContent).toContain('Other damage');
  expect(page.container.querySelector('[data-unattributed]')?.textContent).toContain('74,000');
  expect(page.container.querySelector('[data-unattributed]')?.hasAttribute('data-player-id')).toBe(false);
  expect(page.container.querySelector('[data-player-id], .damage-row')?.textContent).toContain('12.3%');
  if (mode === 'realm') expect(page.container.textContent).toContain('Ended');
  expect(screen.queryByRole('meter', { name: 'Target HP' })).toBeNull();
  for (const value of [0, Number.NaN]) {
    await page.rerender({ snapshot: { ...snapshot, fights: [{ ...boss, unattributed_damage: value }] }, stale: false });
    expect(page.container.querySelector('[data-unattributed]')).toBeNull();
  }
  expect(boss.players[0]).toMatchObject({ rank: 7, share: 0.123 });
});

it.each(['realm', 'dungeon'] as const)('%s Home and rail preserve boss focus across snapshots and end transitions', async mode => {
  setDebugMode(true);
  const snapshot = mode === 'realm' ? realmSnapshot() : homeSnapshot();
  snapshot.at = '10000000';
  const first = fight('first', { run_id: snapshot.run?.id ?? null, target_kind: mode === 'dungeon' ? 'boss' : 'quest', target_type: 301 });
  const second = fight('second', { run_id: first.run_id, target_kind: 'boss', target_type: 302, last_hit: '9000000' });
  snapshot.fights = [first];
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'First boss'), catalog(302, 'Second boss')] });
  await flushHomeRender();
  page.publish({ ...snapshot, fights: [first, second] }); await flushHomeRender();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  expect(rail.querySelector('.target')?.textContent).toContain('First boss');
  const main = screen.getByRole('main');
  if (mode === 'dungeon') expect(main.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toContain('First boss');
  else expect(main.querySelector('.fight-target')?.textContent).toContain('First boss');
  const ended = { ...first, ended: '10000000' };
  page.publish({ ...snapshot, fights: [ended] }); await flushHomeRender();
  expect(rail.querySelector('.target')?.textContent).toContain(mode === 'realm' ? 'Ended' : 'Dungeon Total');
  expect(main.textContent).toContain(mode === 'realm' ? 'Ended' : 'No Totals');
  page.publish({ ...snapshot, fights: [ended, second] }); await flushHomeRender();
  expect(rail.querySelector('.target')?.textContent).toContain('Second boss');
});

it('dungeon focus cannot cross runs or sessions', () => {
  const snapshot = homeSnapshot();
  const previous = snapshot.fights[0];
  expect(snapshotFight({ ...snapshot, run: { ...snapshot.run!, id: 'new-run' } }, previous.id)).toBeNull();
  expect(snapshotFight({ ...snapshot, fights: [{ ...previous, session_id: 'foreign' }] }, previous.id)).toBeNull();
});

it.each(['realm', 'dungeon'] as const)('%s with ordinary and unknown fights has no boss panel or encounter tab', mode => {
  const snapshot = mode === 'realm' ? realmSnapshot() : homeSnapshot();
  snapshot.fights = [fight('ordinary', { run_id: snapshot.run?.id ?? null, target_kind: 'ordinary' }),
    fight('unknown', { run_id: snapshot.run?.id ?? null, target_type: null })];
  const page = mode === 'realm' ? render(RealmHome, { snapshot }) : render(DungeonHome, { snapshot, stale: false });
  expect(page.container.textContent).toContain(mode === 'realm' ? 'No Boss' : 'No Totals');
  expect(page.container.querySelector('[data-player-id], .damage-row')).toBeNull();
  expect(page.container.querySelectorAll('[role="tab"]')).toHaveLength(mode === 'dungeon' ? 1 : 0);
});

it('dungeon defaults to supplied run totals, follows an open boss through mob hits, then returns to totals', async () => {
  let snapshot = homeSnapshot();
  const mob = fight('mob', { target_kind: 'ordinary', total_damage: 987654 });
  const boss = fight('boss', { target_kind: 'boss', total_damage: 1234 });
  const local = { ...boss.players[0], damage: 12000, dps: 345, rank: 8, share: 0.12 };
  snapshot.run_totals = { fights: 10, kills: 4, total_damage: 100000, unattributed_damage: 74000,
    partial: false, local, players: [local] };
  snapshot.fights = [mob];
  const page = render(DungeonHome, { snapshot, stale: false });
  const detail = () => page.container.querySelector('.fight-detail')!;
  expect(dungeonView(snapshot)).toMatchObject({ fight: null, wholeRun: true, self: { dps: 345, rank: 8 } });
  expect(railView(snapshot)).toMatchObject({ fight: null, wholeRun: true, target: 'Dungeon Total', dps: 345, rank: 8 });
  expect(detail().getAttribute('data-whole-run')).toBe('true');
  expect(detail().textContent).toContain('100,000');
  expect(detail().textContent).not.toContain('987,654');
  expect(detail().querySelector('[data-unattributed]')?.textContent).toContain('74,000');
  expect(detail().querySelector('[data-player-id], .damage-row')?.textContent).toContain('12%');
  for (const last_hit of ['9000000', '11000000']) {
    snapshot = { ...snapshot, fights: [boss, { ...mob, last_hit }] };
    await page.rerender({ snapshot: structuredClone(snapshot), stale: false });
    expect(detail().getAttribute('data-whole-run')).toBe('false');
    expect(detail().textContent).toContain('1,234');
    expect(dungeonView(snapshot, boss.id).fight?.id).toBe(boss.id);
    expect(railView(snapshot, [], false, boss.id).fight?.id).toBe(boss.id);
  }
  snapshot = { ...snapshot, fights: [{ ...boss, ended: '12000000' }, mob] };
  await page.rerender({ snapshot: structuredClone(snapshot), stale: false });
  expect(detail().getAttribute('data-whole-run')).toBe('true');
  expect(detail().textContent).toContain('100,000');
  expect(detail().textContent).not.toContain('Fight Ended');
  expect(railView(snapshot, [], false, boss.id)).toMatchObject({ fight: null, target: 'Dungeon Total', dps: 345 });
  await fireEvent.click(screen.getByRole('tab', { name: 'Unknown target' }));
  expect(detail().getAttribute('data-whole-run')).toBe('false');
  expect(detail().textContent).toContain('Ended');
  expect(detail().textContent).toContain('1,234');
  await fireEvent.click(screen.getByRole('tab', { name: 'Whole run' }));
  expect(detail().getAttribute('data-whole-run')).toBe('true');
  for (const run_totals of [null, undefined]) {
    snapshot = { ...snapshot, run_totals };
    await page.rerender({ snapshot: structuredClone(snapshot), stale: false });
    expect(detail().textContent).toContain('No Totals');
    expect(detail().querySelector('table')).toBeNull();
    expect(railView(snapshot, [], false, boss.id)).toMatchObject({ fight: null, dps: null, rank: null });
  }
});

it('realm focus survives Home remounts without an observed visit entry', async () => {
  const snapshot = { ...realmSnapshot(), at: '10000000' };
  const first = fight('first', { run_id: null, target_type: 301, target_kind: 'quest', last_hit: '8000000' });
  const second = fight('second', { run_id: null, target_type: 302, target_kind: 'quest', last_hit: '7000000' });
  snapshot.fights = [first, second];
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'First boss'), catalog(302, 'Second boss')] });
  await flushHomeRender();
  const assertFocus = () => {
    expect(screen.getByRole('main').querySelector('.fight-target')?.textContent).toContain('First boss');
    expect(screen.getByRole('complementary', { name: 'Character' }).querySelector('.target')?.textContent).toContain('First boss');
  };
  expect(screen.getByRole('main').textContent).toContain('No Loot');
  expect(screen.getByRole('main').textContent).not.toContain('Visit observations unavailable until the next observed entry');
  setDebugMode(true); await flushHomeRender();
  expect(screen.getByRole('main').textContent).toContain('Visit observations unavailable until the next observed entry');
  assertFocus();
  const originalHome = screen.getByRole('main').querySelector('.realm-home')!;
  const rail = within(screen.getByRole('complementary', { name: 'Character' }));
  await fireEvent.click(rail.getByRole('link', { name: 'Settings' }));
  await flushHomeRender();
  expect(originalHome.isConnected).toBe(false);
  page.publish({ ...snapshot, fights: [first, { ...second, last_hit: '9000000' }] });
  await flushHomeRender();
  const navigation = within(rail.getByRole('navigation', { name: 'Navigation' }));
  await fireEvent.click(navigation.getByRole('link', { name: 'Home' }));
  await flushHomeRender();
  expect(screen.getByRole('main').querySelector('.realm-home')).not.toBe(originalHome);
  assertFocus();
  page.publish({ ...snapshot, fights: [first, { ...second, last_hit: '10000000' }] });
  await flushHomeRender();
  assertFocus();
});

it('an explicitly selected secondary boss stays selected when it ends', async () => {
  const snapshot = homeSnapshot();
  const first = fight('first', { target_type: 301, last_hit: '8000000', total_damage: 1111 });
  const second = fight('second', { target_type: 302, last_hit: '7000000', total_damage: 2222 });
  snapshot.fights = [first, second];
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'Focused boss'), catalog(302, 'Secondary boss')] });
  await flushHomeRender();
  const main = within(screen.getByRole('main'));
  const detail = () => screen.getByRole('main').querySelector('.fight-detail')!;
  await fireEvent.click(main.getByRole('tab', { name: /Secondary boss/ }));
  expect(detail().textContent).toContain('2,222');
  const ended = { ...second, ended: '9000000' };
  page.publish({ ...snapshot, fights: [first, ended] });
  await flushHomeRender();
  expect(main.getByRole('tab', { selected: true }).textContent).toContain('Secondary boss');
  expect(detail().textContent).toContain('Ended');
  expect(detail().textContent).toContain('2,222');
});

it.each([true, false])('a missing secondary selection falls back with another open boss: %s', async hasBoss => {
  const snapshot = homeSnapshot();
  const first = fight('first', { target_type: 301, last_hit: '8000000', total_damage: 1111 });
  const second = fight('second', { target_type: 302, last_hit: '7000000', total_damage: 2222 });
  snapshot.fights = [first, second];
  snapshot.run_totals = { fights: 2, kills: 0, total_damage: 9999, unattributed_damage: 4000,
    players: first.players, local: first.players[0], partial: false };
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'Focused boss'), catalog(302, 'Secondary boss')] });
  await flushHomeRender();
  const main = within(screen.getByRole('main'));
  await fireEvent.click(main.getByRole('tab', { name: /Secondary boss/ }));
  page.publish({ ...snapshot, fights: hasBoss ? [first] : [] });
  await flushHomeRender();
  const detail = screen.getByRole('main').querySelector('.fight-detail')!;
  expect(detail.getAttribute('data-whole-run')).toBe(String(!hasBoss));
  expect(detail.textContent).toContain(hasBoss ? '1,111' : '9,999');
  expect(main.getByRole('tab', { selected: true }).textContent).toContain(hasBoss ? 'Focused boss' : 'Whole run');
  if (hasBoss) {
    page.publish(snapshot); await flushHomeRender();
    expect(main.getByRole('tab', { selected: true }).textContent).toContain('Focused boss');
  }
});

it('realm kill evidence survives an unknown target type without allowing that fight to take focus', () => {
  const snapshot = realmSnapshot();
  const unknown = fight('unknown-event-boss', { run_id: null, target_type: null, target_kind: 'event_boss',
    started: '6000001', ended: '7000000', kill: { at: '7000000', evidence: 'hp_zero_removed', provisional: false } });
  const window = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, visitKey: 'visit', visitStarted: '6000000',
    fights: [unknown, { ...unknown, id: 'unknown-kind', target_kind: null }, { ...unknown, id: 'no-evidence', kill: null }] };
  expect(realmView(snapshot, window).kills).toMatchObject([{ id: 'fight:unknown-event-boss', targetType: null }]);
  expect(realmView(snapshot, window).fight).toBeNull();
});

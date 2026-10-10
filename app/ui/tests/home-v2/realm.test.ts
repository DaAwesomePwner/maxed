import { setDebugMode } from '../../src/lib/debug/debug';
import { screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { catalog, deferred, fight, flushHomeRender, homeSnapshot, mountV2, realmSnapshot } from './fixtures';

afterEach(() => { vi.restoreAllMocks(); setDebugMode(false); });

it('realm_visit_reset_and_unmatched_announcement', async () => {
  const snapshot = realmSnapshot();
  const page = mountV2(snapshot, undefined, { objects: [catalog(300, 'Synthetic world boss')] });
  await flushHomeRender();
  const enter = () => page.send({ map_entered: { at: '6000000', name: 'Synthetic Realm', kind: 'realm' } });
  enter();
  // A same-type nearby fight is deliberately not an identity link.
  page.send({ fight_ended: fight('announcement-fight', { run_id: null, started: '6000001', ended: '7000000',
    kill: { at: '7000000', evidence: 'realm_announcement', provisional: true },
    players: [{ ...fight('seed').players[0], rank: 37, share: 0.876, damage: 54321 }] }) });
  page.send({ instance_event: { id: 'announcement', session_id: snapshot.session!.id, run_id: null,
    at: '7000000', kind: { type: 'realm_event_killed', boss_key: 'Synthetic world boss', object_type: 300 }, provisional: true } });
  await flushHomeRender();
  const main = screen.getByRole('main');
  expect(main.textContent).not.toMatch(/this observed visit.*excludes dungeon hops/i);
  expect(main.textContent).toContain('Synthetic world boss');
  expect(main.textContent).not.toMatch(/contribution unavailable/i);
  expect(main.textContent).not.toMatch(/provisional|announcement/i);
  setDebugMode(true); await flushHomeRender();
  expect(main.textContent).toMatch(/contribution unavailable/i);
  expect(main.textContent).toMatch(/provisional|announcement/i);
  setDebugMode(false); await flushHomeRender();
  expect(main.querySelector('[data-kill-id]')?.textContent).not.toMatch(/rank\s*37|87\.6%|54,321/i);
  expect(main.querySelectorAll('[data-kill-id]')).toHaveLength(1);
  page.send({ map_entered: { at: '8000000', name: 'Synthetic dungeon', kind: 'dungeon' } });
  enter(); await flushHomeRender();
  expect(main.querySelectorAll('[data-kill-id]')).toHaveLength(0);
  expect(main.textContent).not.toContain('Synthetic world boss');
  for (const read of page.history) expect(read).not.toHaveBeenCalled();
});

it('realm_score_is_not_closing_state', async () => {
  const snapshot = realmSnapshot();
  const page = mountV2(snapshot); await flushHomeRender();
  const main = screen.getByRole('main');
  expect(main.textContent).toMatch(/150\s*\/\s*100/);
  expect(main.textContent).toContain('150%');
  const meter = within(main).getByRole('progressbar', { name: 'Realm score' });
  expect(meter.getAttribute('aria-valuenow')).toBe('100');
  expect(main.querySelector('[data-kpi="score"]')?.textContent).toContain('150%');
  expect(main.textContent).not.toMatch(/closing|closed|time.to.close|fame.hour|trip elapsed/i);
  for (const score of [null, { current: 0, max: 100 }, { current: 150, max: 0 }, { current: -1, max: 100 }, { current: Number.NaN, max: 100 }]) {
    page.publish({ ...snapshot, instance: { ...snapshot.instance!, realm_score: score } });
    await flushHomeRender();
    if (score?.current === 0) expect(main.textContent).toMatch(/0\s*\/\s*100/);
    else {
      expect(main.querySelector('[data-kpi="score"]')?.textContent).toContain('\u2014');
      expect(main.querySelector('[data-kpi="score"]')?.textContent).not.toMatch(/unavailable|required/i);
      expect(main.textContent).not.toMatch(/NaN|Infinity|150%/);
    }
  }
});

it('realm_gap_hides_unscoped_loot', async () => {
  const snapshot = realmSnapshot();
  const drop = { ...homeSnapshot().drops[0], id: 'old-bag', run_id: null, bag_type: 701, at: '6000001' };
  snapshot.drops = [drop];
  const page = mountV2(snapshot, undefined, { objects: [catalog(701, 'Prior visit bag')] });
  await flushHomeRender();
  const main = screen.getByRole('main');
  expect(main.textContent).toContain('No Loot');
  expect(main.textContent).not.toMatch(/visit observations unavailable until the next observed entry/i);
  expect(main.textContent).not.toContain('Prior visit bag');
  page.send({ map_entered: { at: '6000000', name: snapshot.map!, kind: 'realm' } });
  page.send({ bag_dropped: drop }); await flushHomeRender();
  expect(main.querySelector('[aria-label="Prior visit bag"]')).toBeTruthy();
  page.send({ drop_updated: { ...drop, contents_known: false, contents: [], items: [] } });
  await flushHomeRender();
  expect(main.querySelectorAll('[data-drop-id="old-bag"]')).toHaveLength(1);
  const pending = deferred<typeof snapshot>();
  page.client.querySnapshot = vi.fn(() => pending.promise);
  page.transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '18', at: '8000000', event: { bag_dropped: drop } });
  await flushHomeRender();
  expect(main.textContent).not.toContain('Prior visit bag');
  pending.resolve({ ...snapshot, sequence: '18' }); await flushHomeRender();
  expect(main.textContent).not.toContain('Prior visit bag');
  expect(main.textContent).toContain('No Loot');
  expect(main.textContent).not.toMatch(/visit observations unavailable until the next observed entry/i);
  for (const read of page.history) expect(read).not.toHaveBeenCalled();
});

it('realm_kill_evidence_not_fight_end', async () => {
  const snapshot = realmSnapshot();
  const page = mountV2(snapshot, undefined, { objects: [catalog(301, 'Ended without kill'), catalog(302, 'Quest only'), catalog(303, 'Proven world boss')] });
  await flushHomeRender();
  page.send({ map_entered: { at: '6000000', name: snapshot.map!, kind: 'realm' } });
  const ended = { run_id: null, started: '6000001', ended: '7000000' };
  page.send({ fight_ended: fight('no-kill', { ...ended, target_type: 301, kill: null }) });
  page.send({ fight_ended: fight('quest', { ...ended, target_type: 302, target_kind: 'quest', kill: null }) });
  page.send({ fight_ended: fight('proven', { ...ended, target_type: 303, kill: { at: '7000000', evidence: 'hp_zero_removed', provisional: false } }) });
  await flushHomeRender();
  const main = screen.getByRole('main');
  const rows = main.querySelectorAll('[data-kill-id]');
  expect(rows).toHaveLength(1);
  expect(rows[0].textContent).toContain('Proven world boss');
  expect(rows[0].textContent).not.toMatch(/Ended without kill|Quest only/);
  expect(rows[0].textContent).not.toMatch(/HP|health/i);
  expect(rows[0].querySelector('.kill-stats b')?.textContent).toBe('Rank #1');
  expect(rows[0].textContent).toContain('#1');
  expect(main.textContent).not.toMatch(/of\s*85|world.boss total|realm.trip count/i);
});

it('realm_kill_and_loot_windows_are_bounded_and_keyboard_reachable', async () => {
  const snapshot = realmSnapshot();
  const page = mountV2(snapshot, undefined, { objects: Array.from({ length: 7 }, (_, index) => catalog(700 + index, `Visit bag ${index}`)) });
  await flushHomeRender();
  page.send({ map_entered: { at: '6000000', name: snapshot.map!, kind: 'realm' } });
  for (let index = 0; index < 7; index++) {
    page.send({ instance_event: { id: `kill-${index}`, session_id: snapshot.session!.id, run_id: null, at: String(6000001 + index),
      kind: { type: 'realm_event_killed', boss_key: `Synthetic boss ${index}` }, provisional: true } });
    page.send({ bag_dropped: { ...homeSnapshot().drops[0], id: `visit-bag-${index}`, run_id: null, at: String(6000001 + index),
      bag_type: 700 + index, items: [], contents: [], contents_known: false } });
  }
  await flushHomeRender();
  const main = screen.getByRole('main');
  expect(main.querySelectorAll('[data-kill-id]')).toHaveLength(2);
  const firstIds = Array.from(main.querySelectorAll('[data-kill-id]'), row => row.getAttribute('data-kill-id'));
  expect(main.querySelectorAll('[data-overflow-bag]')).toHaveLength(6);
  expect(main.querySelectorAll('.best-card[data-drop-id]')).toHaveLength(1);
  const next = within(main).getByRole('button', { name: '+5 more kills ›' });
  next.focus();
  const user = userEvent.setup();
  await user.keyboard('{Enter}'); await flushHomeRender();
  const allIds = Array.from(main.querySelectorAll('[data-kill-id]'), row => row.getAttribute('data-kill-id'));
  expect(allIds).toHaveLength(7);
  expect(firstIds.every(id => allIds.includes(id))).toBe(true);
  expect(document.activeElement).toBe(next);
  expect(next.textContent).toBe('Show fewer kills');
});

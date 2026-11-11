import { setDebugMode } from '../../src/lib/debug/debug';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import HeroCard from '../../src/lib/shell/HeroCard.svelte';
import { railView, scopedFights } from '../../src/lib/shell/rail';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { dungeonObservations } from '../../src/lib/home/dungeon';
import { homeSnapshot, mountV2, flushHomeRender } from './fixtures';

afterEach(() => { vi.restoreAllMocks(); setDebugMode(false); });

it('memoizes scoped fights by snapshot and window without sharing dungeon sorting', () => {
  const snapshot = homeSnapshot();
  const older = { ...snapshot.fights[0], id: 'older', started: '1000000' };
  const newer = { ...older, id: 'newer', started: '2000000' };
  snapshot.fights = [older, newer];
  const rows = scopedFights(snapshot);
  expect(scopedFights(snapshot, null)).toBe(rows);
  expect(dungeonObservations(snapshot, null).fights).toEqual([newer, older]);
  expect(scopedFights(snapshot)).toBe(rows);
  expect(rows).toEqual([older, newer]);
  const next = { ...snapshot, fights: [newer] };
  expect(scopedFights(next)).not.toBe(rows);
  expect(scopedFights(next)).toEqual([newer]);
  const window = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, runId: snapshot.run!.id,
    fights: [{ ...older, is_current_quest: true }], fightSequences: { older: String(BigInt(snapshot.sequence) + 1n) } };
  const merged = scopedFights(snapshot, window);
  expect(merged).not.toBe(rows);
  expect(merged[0].is_current_quest).toBe(true);
  expect(scopedFights(snapshot, window)).toBe(merged);
  const caughtUp = { ...window, fightSequences: { older: snapshot.sequence } };
  expect(scopedFights(snapshot, caughtUp)).not.toBe(merged);
  expect(scopedFights(snapshot, caughtUp)[0]).toBe(older);
  expect(scopedFights(snapshot)).toBe(rows);
  expect(scopedFights(null)).toBe(scopedFights(null));
});

it('rail_boost_null_cap_and_unconfirmed_conditions', async () => {
  const snapshot = homeSnapshot();
  snapshot.character!.stat_panel = { breakdown_provisional: true, lines: [
    { stat_id: 0, current: 0, total: 800, boost: null, at_cap: null },
    { stat_id: 3, current: null, total: 300, boost: null, at_cap: false },
    { stat_id: 20, total: 75, base: 63, cap: 63, boost: 12, gear: 4, exalt: 0, other: 8, at_cap: true,
      split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
    { stat_id: 21, total: 25, boost: -3, at_cap: false },
    { stat_id: 22, total: 50, boost: 0, at_cap: null },
    { stat_id: 26, total: 60, boost: null, at_cap: null },
    { stat_id: 27, total: null, boost: null, at_cap: null },
    { stat_id: 28, total: 50, boost: null, at_cap: null },
  ], conditions: [
    { name: 'Slowed', bit: 3, secondary: false, confirmed: true },
    { name: 'Unconfirmed synthetic buff', bit: 9, secondary: true, confirmed: false },
  ] };
  const page = render(HeroCard, { view: railView(snapshot) });
  expect(page.container.textContent).toMatch(/0\s*\/\s*800/);
  expect(page.container.textContent).toContain('+12');
  expect(page.container.textContent).toMatch(/[-\u2212]3/);
  expect(page.container.textContent).not.toContain('+0');
  expect(page.container.textContent).not.toMatch(/\+24|\+4|\+8|exalt\s*[:+]\s*0|from party/i);
  expect(screen.queryByText('Slowed')).toBeNull();
  expect(screen.queryByText('Unconfirmed synthetic buff')).toBeNull();
  expect(page.container.textContent).not.toMatch(/observed.*boost|boost.*observed/i);
  expect(page.container.textContent).not.toMatch(/provisional/i);
  // The actual gold class must follow the supplied flag, not total/base.
  const capped = page.container.querySelector('[data-stat-id="20"]');
  expect(capped?.classList.contains('at-cap')).toBe(true);
  for (const id of [0, 3, 21, 22, 26, 27, 28]) {
    expect(page.container.querySelector(`[data-stat-id="${id}"]`)?.classList.contains('at-cap')).toBe(false);
  }
  expect(page.container.querySelector('[data-stat-id="20"]')?.getAttribute('data-tone')).toBe('max');
  expect(page.container.querySelector('[data-stat-id="21"]')?.getAttribute('data-tone')).toBe('boosted');
  expect(page.container.querySelector('[data-stat-id="22"]')?.getAttribute('data-tone')).toBe('plain');
  expect(page.container.querySelector('[data-stat-id="26"]')?.textContent).not.toContain('+0');
  expect(page.container.querySelector<HTMLElement>('[data-stat-id="3"] .fill')?.style.width).toBe('0%');
  expect(page.container.querySelector('[data-stat-id="3"]')?.getAttribute('title')).toBeNull();
  setDebugMode(true); await fireEvent.click(page.container);
  expect(screen.getByText('Slowed')).toBeTruthy();
  expect(page.container.querySelector('[data-stat-id="3"]')?.getAttribute('title')).toMatch(/fill unavailable/);
  expect(page.container.textContent).toMatch(/provisional/i);
  setDebugMode(false);
  expect(page.container.querySelector<HTMLElement>('[data-stat-id="0"] .fill')?.style.width).toBe('0%');
  delete snapshot.character!.stat_panel;
  await page.rerender({ view: railView(snapshot) });
  expect(page.container.querySelectorAll('[data-stat-id]')).toHaveLength(8);
  expect(page.container.querySelector('[data-stat-id="20"] dd')?.textContent).toBe('— (—)');
  expect(page.container.textContent).not.toMatch(/\+0|with bonuses/i);
});

it('rail_stat_labels_match_protocol_ids', () => {
  // Protocol: 26 VITALITY, 27 WISDOM, 28 DEXTERITY. Distinct totals catch any label swap.
  const expected = [['ATT', 20, 61], ['DEF', 21, 62], ['SPD', 22, 63], ['DEX', 28, 64], ['VIT', 26, 65], ['WIS', 27, 66]] as const;
  const snapshot = homeSnapshot();
  snapshot.character!.stat_panel = { breakdown_provisional: false, conditions: [],
    lines: expected.map(([, stat_id, total]) => ({ stat_id, total, boost: null, at_cap: false })) };
  const page = render(HeroCard, { view: railView(snapshot) });
  const cells = [...page.container.querySelectorAll('.stat-grid [data-stat-id]')];
  expect(cells.map(cell => cell.querySelector('dt')?.textContent)).toEqual(expected.map(([label]) => label));
  for (const [label, id, total] of expected) {
    const cell = cells.find(cell => cell.querySelector('dt')?.textContent === label);
    expect(cell?.getAttribute('data-stat-id')).toBe(String(id));
    expect(cell?.querySelector('dd')?.textContent).toBe(`${total} (—)`);
  }
});

it('rail_all_confirmed_conditions_and_synthetic_local_sprite', () => {
  setDebugMode(true);
  const snapshot = homeSnapshot();
  const conditions = Array.from({ length: 20 }, (_, bit) => ({ name: `Confirmed synthetic condition ${bit}`, bit, secondary: false, confirmed: true }));
  snapshot.character!.stat_panel = { lines: [], conditions, breakdown_provisional: true };
  const sprite = { key: 'synthetic', sheet: 'characters.png', x: 0, y: 0, width: 8, height: 8 };
  const visual = { object_type: snapshot.character!.class_id!, name: 'Synthetic class', sprite, bag: null };
  const page = render(HeroCard, { view: railView(snapshot, [visual]), sheetUrl: `http://maxed-sprite.localhost/${'a'.repeat(64)}/characters.png/0-0-8-8.png` });
  for (const condition of conditions) expect(page.container.textContent).toContain(condition.name);
  expect(screen.getByRole('img', { name: 'Synthetic class' }).getAttribute('data-fallback')).toBe('false');
  expect(page.container.querySelector('img')?.src).toMatch(/^http:\/\/maxed-sprite\.localhost\//);
});

it('rail_identity_equipment_and_capture_controls', async () => {
  const snapshot = homeSnapshot();
  snapshot.character!.name = 'Synthetic character with a deliberately long name';
  snapshot.character!.equipment = [3, 0, 2, 1].map(slot => ({ ...snapshot.character!.equipment[0], slot, item_type: 200 + slot }));
  const page = mountV2(snapshot);
  await flushHomeRender();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  expect(rail.textContent).toContain(snapshot.character!.name);
  for (const [slot, name] of ['Weapon', 'Ability', 'Armor', 'Ring'].entries()) {
    const equipment = within(rail).getByRole('img', { name: new RegExp(name, 'i') });
    expect(equipment.getAttribute('data-fallback')).toBe('true');
    expect(equipment.closest('[data-slot]')?.getAttribute('data-slot')).toBe(String(slot));
    expect(equipment.closest('[data-slot]')?.getAttribute('data-item-type')).toBe(String(200 + slot));
  }
  const stop = vi.spyOn(page.client, 'stopCapture');
  await fireEvent.click(within(rail).getByRole('button', { name: 'Stop capture' }));
  await flushHomeRender();
  expect(stop).toHaveBeenCalledOnce();
  page.transport.publishCapture({ ...snapshot.capture, state: 'PermissionDenied', message: 'Synthetic permission failure' });
  await flushHomeRender();
  const start = vi.spyOn(page.client, 'startCapture');
  await fireEvent.click(within(rail).getByRole('button', { name: 'Retry capture' }));
  expect(start).toHaveBeenCalledOnce();
  const stale = render(HeroCard, { view: railView({ ...snapshot, character: { ...snapshot.character!, identity_verified: false, died_at: '5000000' } }, [], true) });
  expect(stale.container.textContent).not.toMatch(/last seen/i);
  expect(stale.container.textContent).not.toMatch(/unverified identity/i);
  expect(stale.container.textContent).toMatch(/Death/);
});

it.each(['#/', '#/runs', '#/alerts', '#/settings'])('rail_present_on_all_routes (%s)', async hash => {
  const page = mountV2(homeSnapshot(), undefined, { hash, features: { lootBrowsing: false, logs: false, stats: false, party: false, character: false, homeRedesign: false } });
  await flushHomeRender();
  expect(screen.getByRole('complementary', { name: 'Character' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Stop capture' })).toBeTruthy();
  const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
  for (const name of ['Home', 'Runs', 'Alerts']) expect(nav.getByRole('link', { name })).toBeTruthy();
  expect(screen.getByRole('link', { name: 'Settings' })).toBeTruthy();
  expect(nav.queryByRole('link', { name: 'Loot' })).toBeNull();
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledOnce();
});

import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import type { HomeLiveWindow } from '../../src/lib/home/v2/contracts';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { setDebugMode } from '../../src/lib/debug/debug';
import { homeSnapshot } from '../home/fixtures';
import { readFileSync } from 'node:fs';
afterEach(() => setDebugMode(false));

it.each(['miniboss', 'quest'] as const)('falls back to an open boss when shared focus is a %s, until run end', async target_kind => {
  const snapshot = homeSnapshot();
  const boss = { ...snapshot.fights[0], target_hp: 80, max_hp: 100 };
  const other = { ...boss, id: 'other-target', target_kind };
  snapshot.fights = [boss, other];
  const liveWindow: HomeLiveWindow = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id,
    runId: snapshot.run!.id, focusedFightId: other.id };
  const page = render(DungeonHome, { snapshot, stale: false, liveWindow });
  expect(screen.getByRole('meter', { name: 'Target HP' }).getAttribute('aria-valuenow')).toBe('80');
  expect(screen.getByRole('tab', { name: 'Whole run' }).getAttribute('aria-selected')).toBe('false');
  await page.rerender({ snapshot: { ...snapshot, run: { ...snapshot.run!, ended: snapshot.at } }, stale: false, liveWindow });
  expect(screen.queryByRole('meter', { name: 'Target HP' })).toBeNull();
  expect(screen.getByRole('tab', { name: 'Whole run' }).getAttribute('aria-selected')).toBe('true');
});

it.each([false, true])('keeps the defeated boss focused until a new boss engages (event window: %s)', async events => {
  const snapshot = homeSnapshot();
  snapshot.fights[0].target_hp = 80;
  snapshot.fights[0].max_hp = 100;
  let liveWindow: HomeLiveWindow | null = null;
  const tracker = createHomeLiveWindow(value => { liveWindow = value; });
  if (events) tracker.snapshot(snapshot);
  const page = render(DungeonHome, { snapshot, stale: false, liveWindow });
  const killed = { ...snapshot.fights[0], ended: snapshot.at,
    kill: { at: snapshot.at, provisional: false, evidence: 'victory_quest_target' as const } };
  if (events) tracker.event({ stream_id: snapshot.stream_id, sequence: String(BigInt(snapshot.sequence) + 1n),
    at: snapshot.at, event: { fight_updated: killed } });
  await page.rerender({ snapshot: events ? snapshot : { ...snapshot, fights: [killed] }, stale: false, liveWindow });
  expect(screen.getByRole('meter', { name: 'Target HP' }).getAttribute('aria-valuenow')).toBe('0');
  expect(page.container.querySelector('.boss-hp')?.textContent).toContain('Defeated');
  const next = { ...snapshot.fights[0], id: 'next-boss', target_hp: 80, max_hp: 100,
    started: String(BigInt(snapshot.at) + 1n), last_hit: String(BigInt(snapshot.at) + 1n) };
  if (events) tracker.event({ stream_id: snapshot.stream_id, sequence: String(BigInt(snapshot.sequence) + 2n),
    at: snapshot.at, event: { fight_started: next } });
  await page.rerender({ snapshot: events ? snapshot : { ...snapshot, fights: [killed, next] }, stale: false, liveWindow });
  expect(screen.getByRole('meter', { name: 'Target HP' }).getAttribute('aria-valuenow')).toBe('80');
  expect(page.container.querySelector('.boss-hp')?.textContent).not.toContain('Defeated');
  tracker.dispose();
});

it('shows the full player window and lets the chart flex before the page grows', () => {
  const page = render(DungeonHome, { snapshot: homeSnapshot(), stale: false });
  const table = screen.getByRole('table', { name: 'Player Damage' });
  expect(table.parentElement?.parentElement).toBe(page.container.querySelector('.fight-chart')?.parentElement);
  expect(screen.queryByRole('region', { name: 'Fight players' })).toBeNull();
  // JSDOM cannot measure layout; protect the sizing contract for browser verification.
  const css = readFileSync('src/lib/home/v3/area-home.css', 'utf8');
  const rule = (selector: string) => css.slice(css.indexOf(`${selector} {`)).split('}')[0];
  expect(rule('.area-home')).toContain('height: 100%');
  expect(rule('.area-home')).toContain('min-height: 0');
  // The bounded track gives surplus height to the chart; its intrinsic minimum
  // keeps the full table and plot floor inside the card when main must scroll.
  expect(rule('.area-home')).toContain('minmax(min-content, 1fr)');
  expect(rule('.area-home .fight-panel')).toMatch(/display: flex; flex-direction: column/);
  expect(rule('.area-home .fight-panel')).toContain('min-height: min-content');
  expect(rule('.area-home .fight-detail > .damage-table')).toContain('flex: none');
  expect(css).not.toContain('.fight-players');
  expect(css).toMatch(/@media \(max-width: 950px\) \{\s*\.area-home \{ height: auto;/);
  const shell = readFileSync('src/lib/shell/Shell.svelte', 'utf8');
  expect(shell).toMatch(/@media \(min-width: 951px\) \{\s*\.home-page > \.home-content:has\(:global\(\.area-home\)\) \{ flex: 1 1 0;/);
  const home = readFileSync('src/routes/Home.svelte', 'utf8');
  expect(home).toMatch(/@media \(min-width: 951px\) \{\s*\.home:has\(> :global\(\.area-home\)\) \{ display: flex; flex-direction: column; height: 100%;/);
  expect(home).toMatch(/\.home > :global\(\.area-home\) \{ flex: 1;/);
  const chart = readFileSync('src/lib/home/v3/FightChart.svelte', 'utf8');
  expect(chart).toMatch(/\.fight-chart \{[^}]*min-height: 0;[^}]*flex: 1;/);
  expect(chart).toMatch(/\.plot \{[^}]*min-height: 56px;[^}]*flex: 1;/);
  expect(chart).toMatch(/\.plot \{ position: relative;/);
  expect(chart).toMatch(/svg \{ position: absolute; inset: 0;[^}]*width: 100%; height: 100%;/);
  const timeline = readFileSync('src/lib/home/v2/DungeonTimeline.svelte', 'utf8');
  expect(timeline).toContain('--span-height: 22px');
  expect(timeline).toMatch(/\.track \{[^}]*min-height: 58px/);
  expect(timeline).toMatch(/\.row \{[^}]*height: 18px/);
  expect(timeline).toMatch(/\.marker::before \{[^}]*position: absolute;[^}]*inset: -3px 0;/);
  expect(page.container.querySelectorAll('.track > .row')).toHaveLength(3);
});

it('uses a short local clock in the Dungeon header with a missing-time fallback', async () => {
  const snapshot = homeSnapshot();
  snapshot.run!.started = String(new Date(2026, 9, 3, 19, 35, 42).getTime() * 1000);
  const page = render(DungeonHome, { snapshot, stale: false });
  expect(page.container.querySelector('.area-header p')?.textContent).toBe('Entered 7:35 PM');
  await page.rerender({ snapshot: { ...snapshot, run: null }, stale: false });
  expect(page.container.querySelector('.area-header p')?.textContent).toBe('Entered \u2014');
});

it.each([-12, 0])('renders Run fame %s without a positive prefix', (fame) => {
  const props = fixture();
  props.snapshot.run_totals!.fame_delta = fame;
  const page = render(DungeonHome, { ...props, stale: false });
  expect(page.container.querySelector('[data-kpi="fame"] .value')?.textContent).toBe(String(fame));
});

function fixture() {
  const snapshot = homeSnapshot(), seed = snapshot.fights[0];
  snapshot.at = '100000000'; snapshot.run!.started = '1000000';
  snapshot.fights = Array.from({ length: 64 }, (_, i) => ({ ...seed, id: `f${i}`,
    target_kind: 'boss' as const, started: String(2_000_000 + i * 1_000_000),
    ended: i === 63 ? null : String(3_000_000 + i * 1_000_000), last_hit: '100000000' }));
  const players = Array.from({ length: 85 }, (_, i) => ({ ...seed.players[0], player_id: i === 14 ? snapshot.character!.id : `p${i}`,
    name: `Player${i}`, is_local: i === 14, rank: i + 1, share: .01, damage: 8500 - i * 10 }));
  snapshot.fights[63].players = players;
  snapshot.run_totals = { fights: 64, kills: 63, players, local: players[14], total_damage: 500000,
    unattributed_damage: 123, partial: false, fame_delta: 186 };
  snapshot.drops = Array.from({ length: 128 }, (_, i) => ({ ...snapshot.drops[0], id: `b${i}`, at: '90000000' }));
  const liveWindow: HomeLiveWindow = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, runId: snapshot.run!.id,
    visitKey: 'visit', visitStarted: '1000000', partial: false, reasons: [], drops: snapshot.drops };
  return { snapshot, liveWindow };
}
it.each([null, '5000000', '4000000'])('Left tile excludes departures at or after completion %s', completed_at => {
  const props = fixture();
  props.snapshot.run!.completed_at = completed_at;
  props.liveWindow.events = ['4999999', '5000000', '5000001'].map((at, index) => ({
    id: `departure-${index}`, at, session_id: props.snapshot.run!.session_id,
    run_id: props.snapshot.run!.id, provisional: true,
    kind: { type: 'player_left', object_id: index, player_name: null, class_id: null },
  }));
  const page = render(DungeonHome, { ...props, stale: false });
  expect(page.container.querySelector('[data-kpi="left"] .value')?.textContent).toBe(completed_at === null ? '3' : completed_at === '5000000' ? '1' : '0');
});
it('64 fights, 85 players and 128 bags stay bounded; whole run has nine rows and no chart', async () => {
  const props = fixture();
  const page = render(DungeonHome, { ...props, stale: false });
  expect(screen.getAllByRole('tab')).toHaveLength(65);
  expect(page.container.querySelectorAll('.damage-row')).toHaveLength(6);
  expect(screen.getByRole('link', { name: '+79 more players ›' }).getAttribute('href')).toContain('/fights/f63');
  const loot = screen.getByRole('region', { name: 'LOOT THIS RUN' });
  expect(loot.querySelectorAll('[data-drop-id]').length).toBeLessThan(128);
  expect(loot.textContent).toContain('128');
  await fireEvent.click(screen.getByRole('tab', { name: 'Whole run' }));
  expect(page.container.querySelectorAll('.damage-row')).toHaveLength(9);
  expect(page.container.querySelector('.fight-chart')).toBeNull();
  expect(screen.getByRole('link', { name: '+76 more players ›' }).getAttribute('href')).toBe(`#/runs/${props.snapshot.run!.id}`);
  expect(page.container.querySelector('[data-unattributed]')?.textContent).toContain('123');
});
it('numeric updates keep selection and timer slots; changed run resets selection', async () => {
  const props = fixture(), onSelectFight = vi.fn();
  const page = render(DungeonHome, { ...props, stale: false, onSelectFight });
  await fireEvent.click(screen.getByRole('tab', { name: 'Whole run' }));
  const kpi = page.container.querySelector('.area-kpis');
  props.snapshot.at = '9999000000';
  await page.rerender({ ...props, snapshot: structuredClone(props.snapshot), stale: false, onSelectFight });
  expect(page.container.querySelector('.area-kpis')).toBe(kpi);
  expect(screen.getByRole('tab', { selected: true }).textContent).toBe('Whole run');
  const next = structuredClone(props.snapshot); next.run!.id = 'next';
  next.fights.forEach(fight => fight.run_id = 'next');
  await page.rerender({ snapshot: next, liveWindow: null, stale: false, onSelectFight });
  expect(screen.getByRole('tab', { selected: true }).textContent).not.toBe('Whole run');
});
it('an explicitly selected live boss remains selected when it ends and a new boss arrives', async () => {
  const props = fixture(), onSelectFight = vi.fn();
  const page = render(DungeonHome, { ...props, stale: false, onSelectFight });
  const liveTab = screen.getAllByRole('tab').find(tab => tab.classList.contains('live'))!;
  await fireEvent.click(liveTab);
  const next = structuredClone(props.snapshot);
  next.fights[63].ended = next.at; next.fights[63].target_hp = 73; next.fights[63].max_hp = 100;
  next.fights[63].kill = null; next.fights[63].end_reason = 'map_changed';
  next.fights.push({ ...next.fights[63], id: 'next-boss', started: next.at, ended: null });
  await page.rerender({ ...props, snapshot: next, stale: false, onSelectFight });
  expect(screen.getByRole('tab', { selected: true })).toBe(liveTab);
  expect(page.container.querySelector('.boss-status')?.textContent).toBe('Ended');
  expect(page.container.querySelector('.boss-hp')?.textContent).toContain('73 /');
  expect(onSelectFight).not.toHaveBeenCalledWith(null);
});
it('PB value turns red only after the all-time best, with a separate delta tone', async () => {
  const snapshot = homeSnapshot(); snapshot.run!.started = '1000000'; snapshot.at = '40000000';
  const dungeonBests = [{ dungeon: snapshot.run!.dungeon, best_clear_micros: 50_000_000 }];
  const page = render(DungeonHome, { snapshot, dungeonBests, stale: false });
  const cell = page.container.querySelector('[data-kpi="time"]')!;
  expect(cell.getAttribute('data-tone')).toBe('plain');
  expect(cell.querySelector('[data-sub-tone="good"]')?.textContent).toContain('−0:11');
  await page.rerender({ snapshot: { ...snapshot, at: '60000000' }, dungeonBests, stale: false });
  expect(page.container.querySelector('[data-kpi="time"]')).toBe(cell);
  expect(cell.getAttribute('data-tone')).toBe('bad');
  expect(cell.querySelector('[data-sub-tone="bad"]')?.textContent).toContain('+0:09');
  await page.rerender({ snapshot, dungeonBests: null, stale: false });
  expect(cell.textContent).toContain('PB —'); expect(cell.getAttribute('data-tone')).toBe('plain');
});
it('unknown contents still feature one bag and exclude it from the other bags', () => {
  const snapshot = homeSnapshot(), seed = snapshot.drops[0];
  snapshot.drops = [0, 1, 2].map(i => ({ ...seed, id: `unknown-${i}`, bag_rank: 9 - i,
    contents_known: false, contents: [], items: [] }));
  const page = render(DungeonHome, { snapshot, stale: false });
  const loot = page.container.querySelector('.area-loot')!;
  expect(loot.querySelector('.best-card')?.getAttribute('data-drop-id')).toBe('unknown-0');
  expect(loot.querySelector('.best-card .item-box')).toBeNull();
  expect(loot.querySelectorAll('[data-drop-id="unknown-0"]')).toHaveLength(1);
  expect(loot.querySelector('.bag-list-root')).toBeTruthy();
  expect(loot.textContent).not.toContain('No bags');
});
it('PB and counts render; names, killer and level are only in tooltips', async () => {
  const props = fixture();
  props.liveWindow.events = [{ id: 'death', session_id: props.snapshot.session!.id, run_id: props.snapshot.run!.id,
    at: '5000000', provisional: false, kind: { type: 'player_death', player_name: 'TestHero', level: 20,
      killed_by: 'Guardian', is_local: false } }];
  const dungeonBests = [{ dungeon: props.snapshot.run!.dungeon, best_clear_micros: 50_000_000 }];
  const page = render(DungeonHome, { ...props, dungeonBests, stale: false });
  const time = page.container.querySelector('[data-kpi="time"]')!;
  expect(time.textContent).toContain('PB 0:50 · +0:49');
  expect(time.getAttribute('data-tone')).toBe('bad');
  expect(time.querySelector('[data-sub-tone="bad"]')).toBeTruthy();
  const kpis = page.container.querySelector<HTMLElement>('.area-kpis')!;
  const tip = within(kpis).getByRole('tooltip', { hidden: true });
  expect(tip.textContent).toContain('TestHero · Level 20 · Killed by Guardian');
  expect(tip.closest('[hidden]')).toBeTruthy();
  const trigger = kpis.querySelector<HTMLElement>(`[aria-describedby="${tip.id}"]`)!;
  await fireEvent.focusIn(trigger); expect(tip.closest('[hidden]')).toBeNull();
});
it('no fight, unknown HP, unverified self, ST loot and debug are honest', async () => {
  const snapshot = homeSnapshot();
  snapshot.character!.identity_verified = false;
  snapshot.fights[0].target_hp = null; snapshot.fights[0].max_hp = null;
  snapshot.drops[0].contents = [{ ...snapshot.drops[0].contents[0], item_type: 200,
    enchant: { state: 'missing', slots: null, applied: null, ids: [] } }];
  const visuals = { ...emptyHomeVisuals(), objects: [{ object_type: 200, name: 'ST fallback', sprite: null, bag: null, item_tier: 'ST' }] };
  const page = render(DungeonHome, { snapshot, visuals, stale: false });
  expect(page.container.querySelector('.boss-hp')?.getAttribute('data-state')).toBe('unknown');
  expect(page.container.querySelector('.damage-row.self')).toBeNull();
  expect(page.container.textContent).toContain('ST fallback');
  expect(page.container.textContent).not.toMatch(/NaN|undefined|provisional|coverage/i);
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('Current HP or positive maximum was not observed.');
  await page.rerender({ snapshot: { ...snapshot, fights: [], run_totals: null }, visuals, stale: false });
  expect(page.container.textContent).toContain('No Totals');
  expect(page.container.querySelector('.fight-chart')).toBeNull();
});

it.each([false, true])('new ST bag row and its merged timeline marker animate once; reduced=%s', async reduced => {
  const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'animate');
  const calls: Array<{ node: HTMLElement; frames: Keyframe[]; options: KeyframeAnimationOptions }> = [];
  Object.defineProperty(HTMLElement.prototype, 'animate', { configurable: true, value: function (
    this: HTMLElement, frames: Keyframe[], options: KeyframeAnimationOptions,
  ) { calls.push({ node: this, frames, options }); return { cancel() {}, onfinish: null }; } });
  vi.stubGlobal('matchMedia', () => ({ matches: reduced, addEventListener() {}, removeEventListener() {} }));
  vi.stubGlobal('ResizeObserver', class {
    constructor(private callback: ResizeObserverCallback) {}
    observe() { this.callback([{ contentRect: { height: 400 } } as ResizeObserverEntry], this as unknown as ResizeObserver); }
    unobserve() {} disconnect() {}
  });
  try {
    const snapshot = homeSnapshot(), seed = snapshot.drops[0];
    snapshot.at = '100000000'; snapshot.run!.started = '1000000';
    snapshot.drops = [{ ...seed, id: 'old-ut', at: '50000000', bag_rank: 255, contents_known: true,
      items: [200], contents: [{ ...seed.contents[0], item_type: 200 }] }];
    const visuals = { ...emptyHomeVisuals(), objects: [
      { object_type: 200, name: 'UT item', sprite: null, bag: null, item_tier: 'UT' },
      { object_type: 201, name: 'ST item', sprite: null, bag: null, item_tier: 'ST' },
    ] };
    const page = render(DungeonHome, { snapshot, visuals, stale: false }); await tick(); await tick();
    const next = structuredClone(snapshot);
    next.drops.push({ ...seed, id: 'new-st', at: '50000001', bag_rank: 90, contents_known: true,
      items: [201], contents: [{ ...seed.contents[0], item_type: 201 }] });
    await page.rerender({ snapshot: next, visuals, stale: false }); await tick(); await tick();
    if (reduced) expect(calls).toHaveLength(0);
    else {
      const glow = calls.find(call => call.node.dataset.dropId === 'new-st');
      expect(glow?.frames).toContainEqual({ boxShadow: '0 0 14px var(--loot-arrival-colour)', offset: .35 });
      expect(glow?.node.style.getPropertyValue('--loot-arrival-colour')).toBe('var(--bag-orange)');
      const pulse = calls.find(call => call.node.dataset.markerMembers?.includes('drop:new-st'));
      expect(pulse?.node.dataset.count).toBe('2');
      expect(pulse?.frames).toContainEqual({ transform: 'scale(1.18)', offset: .35 });
      expect(calls).toHaveLength(2);
      expect(calls.every(call => call.options.duration === 400)).toBe(true);
    }
    const count = calls.length;
    await page.rerender({ snapshot: { ...next, at: '101000000' }, visuals, stale: false }); await tick();
    expect(calls).toHaveLength(count); page.unmount();
  } finally {
    if (original) Object.defineProperty(HTMLElement.prototype, 'animate', original);
    else Reflect.deleteProperty(HTMLElement.prototype, 'animate');
    vi.unstubAllGlobals();
  }
});

it('updates the arrival glow colour when catalog tiers load', async () => {
  const snapshot = homeSnapshot(), seed = snapshot.drops[0];
  const loading = { ...emptyHomeVisuals(), phase: 'loading' as const, objects: [
    { object_type: 999, item_tier: null, name: 'Unrelated', sprite: null, bag: null },
  ] };
  const page = render(DungeonHome, { snapshot, visuals: loading, stale: false });
  const fresh = { ...seed, id: 'late-colour', bag_rank: 255, items: [2591, 3000],
    contents: [2591, 3000].map((item_type, slot) => ({ ...seed.contents[0], item_type, slot })) };
  await page.rerender({ snapshot: { ...snapshot, drops: [...snapshot.drops, fresh] } }); await tick(); await tick();
  const node = page.container.querySelector<HTMLElement>('[data-drop-id="late-colour"]')!;
  expect(node.style.getPropertyValue('--loot-arrival-colour')).toBe('var(--text-secondary)');
  await page.rerender({ visuals: { ...loading, phase: 'ready', objects: [
    ...loading.objects, { object_type: 3000, item_tier: 'UT', name: 'Synthetic UT', sprite: null, bag: null },
  ] } }); await tick(); await tick();
  expect(page.container.querySelector('[data-drop-id="late-colour"]')).toBe(node);
  expect(node.style.getPropertyValue('--loot-arrival-colour')).toBe('var(--tier-ut)');
});

import { fireEvent, render, screen } from '@testing-library/svelte';
import { readFileSync } from 'node:fs';
import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import RealmHome from '../../src/lib/home/RealmHome.svelte';
import { homeVisualTypes, emptyHomeVisuals } from '../../src/lib/home/visuals';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import type { HomeLiveWindow } from '../../src/lib/home/v2/contracts';
import { realmView } from '../../src/lib/home/realm';
import { clockTime } from '../../src/lib/home/nexus';
import { setDebugMode } from '../../src/lib/debug/debug';
import { catalog, fight, homeSnapshot, realmSnapshot } from '../home-v2/fixtures';
afterEach(() => setDebugMode(false));

it('shares the Dungeon desktop height-fill contract through both parents', () => {
  const page = render(RealmHome, { snapshot: realmSnapshot() });
  expect(page.container.querySelector('.realm-home')?.classList.contains('area-home')).toBe(true);
  // JSDOM does not measure geometry; pin the parent rules used by browser layout.
  const shell = readFileSync('src/lib/shell/Shell.svelte', 'utf8');
  expect(shell).toMatch(/@media \(min-width: 951px\) \{\s*\.home-page > \.home-content:has\(:global\(\.area-home\)\) \{ flex: 1 1 0;/);
  const home = readFileSync('src/routes/Home.svelte', 'utf8');
  expect(home).toMatch(/@media \(min-width: 951px\) \{\s*\.home:has\(> :global\(\.area-home\)\) \{ display: flex; flex-direction: column; height: 100%;/);
  expect(home).toMatch(/\.home > :global\(\.area-home\) \{ flex: 1;/);
});

function fixture() {
  const snapshot = realmSnapshot(); snapshot.at = '100000000';
  snapshot.instance!.visit_fame_delta = 412;
  snapshot.instance!.realm_score = { current: 186000, max: 300000 };
  const seed = fight('live', { run_id: null, started: '46000000', last_hit: snapshot.at,
    local_last_hit: snapshot.at, target_kind: 'event_boss', target_type: 300, target_hp: 2880000, max_hp: 6000000 });
  seed.players = Array.from({ length: 85 }, (_, i) => ({ ...seed.players[0],
    player_id: i === 13 ? snapshot.character!.id : `p${i}`, name: `Player${i}`, rank: i + 1,
    is_local: i === 13, share: .03, damage: 100000 - i * 1000 }));
  const dead = fight('dead', { run_id: null, target_type: 301, target_kind: 'boss', started: '10000000', ended: '20000000',
    kill: { at: '20000000', evidence: 'hp_zero_removed', provisional: false } });
  const quest = { ...dead, id: 'quest', target_type: 302, target_kind: 'quest' as const };
  const mini = { ...dead, id: 'mini', target_type: 303, target_kind: 'miniboss' as const };
  snapshot.fights = [seed, dead];
  const drop = homeSnapshot().drops[0];
  const window: HomeLiveWindow = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, visitKey: 'realm-visit',
    visitStarted: '1000000', runId: null, focusedFightId: seed.id, partial: false, reasons: [],
    fights: [dead, seed, quest, mini], drops: Array.from({ length: 128 }, (_, i) => ({ ...drop, id: `bag${i}`,
      run_id: null, at: '30000000' })),
    events: [{ id: 'merged', session_id: snapshot.session!.id, run_id: null, at: '20000000', provisional: true,
      kind: { type: 'realm_event_killed', boss_key: 'Grand Sphinx', object_type: 301, killer: 'Torvik' } }],
  };
  const visuals = { ...emptyHomeVisuals(), objects: [catalog(300, 'Skull Shrine'), catalog(301, 'Grand Sphinx'), catalog(1796, 'Realm Portal')] };
  return { snapshot, window, visuals };
}
it('uses visit fame, score and shared ranked table; requests the realm portal', () => {
  const props = fixture();
  const page = render(RealmHome, props);
  expect(homeVisualTypes(props.snapshot, null)).toContain(1796);
  expect(page.container.querySelector('[data-kpi="fame"]')?.textContent).toContain('+412');
  expect(page.container.querySelector('[data-kpi="score"]')?.textContent).toContain('186,000');
  expect(screen.getByRole('progressbar', { name: 'Realm score' }).getAttribute('aria-valuenow')).toBe('62');
  expect(screen.getByRole('heading', { name: 'LIVE FIGHT' })).toBeTruthy();
  expect(page.container.querySelector('.boss-hp')?.textContent).toContain('2,880,000 / 6,000,000 · 48%');
  expect(page.container.querySelectorAll('.damage-row')).toHaveLength(6);
  expect(page.container.querySelector('.damage-row.self')?.textContent).toContain('14');
  expect(screen.getByRole('link', { name: '+79 more players ›' }).getAttribute('href')).toBe('#/fights/live');
  expect(page.container.querySelectorAll('.area-loot [data-drop-id]').length).toBeLessThan(128);
  expect(page.container.querySelector('.area-loot')?.textContent).toContain('128');
});
it('consumes merged kills including quest and miniboss targets without remerging in the screen', async () => {
  const props = fixture();
  const expected = realmView(props.snapshot, props.window).kills;
  expect(expected.filter(row => row.targetType === 301)).toHaveLength(1);
  expect(expected.filter(row => /quest|mini/.test(row.id))).toHaveLength(2);
  const page = render(RealmHome, props);
  expect(page.container.querySelector('.kill-count')?.textContent).toBe(String(expected.length));
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(2);
  await fireEvent.click(screen.getByRole('button', { name: /1 more kill/ }));
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(3);
});
it('no live fight becomes LAST FIGHT; killed and unknown HP are distinct', async () => {
  const props = fixture();
  const page = render(RealmHome, props);
  expect(page.container.querySelector('.fight-heading .ended-tag')).toBeNull();
  const ended = { ...props.snapshot.fights[0], ended: props.snapshot.at,
    kill: { at: props.snapshot.at, evidence: 'hp_zero_removed' as const, provisional: false } };
  props.snapshot.fights = [ended]; props.window.fights = [ended];
  await page.rerender({ ...props, snapshot: structuredClone(props.snapshot), window: structuredClone(props.window) });
  expect(screen.getByRole('heading', { name: 'LAST FIGHT' })).toBeTruthy();
  expect(page.container.querySelector('.fight-heading .ended-tag')?.textContent).toBe('Ended');
  expect(page.container.querySelector('.boss-hp .track .hp-text')?.textContent).toBe(`Defeated · ${clockTime(ended.kill.at)}`);
  expect(page.container.querySelector('.boss-hp .boss-status')).toBeNull();
  expect(page.container.querySelector('.boss-hp')?.getAttribute('data-state')).toBe('killed');
  props.snapshot.fights = [{ ...ended, kill: null, target_hp: null, max_hp: null }];
  props.window.fights = props.snapshot.fights;
  await page.rerender({ ...props, snapshot: structuredClone(props.snapshot), window: structuredClone(props.window) });
  expect(page.container.querySelector('.boss-hp')?.getAttribute('data-state')).toBe('unknown');
});

it.each([[3599, '59:59'], [3600, '1:00:00'], [4360, '1:12:40']] as const)(
  'formats Time here at %s seconds as %s', (seconds, expected) => {
    const props = fixture();
    props.snapshot.at = String(BigInt(props.window.visitStarted!) + BigInt(seconds) * 1_000_000n);
    const page = render(RealmHome, props);
    expect(page.container.querySelector('[data-kpi="time"] .value')?.textContent).toBe(expected);
  },
);

it('keeps a confirmed kill visible inside the HP bar even when HP is unknown', () => {
  const props = fixture();
  const at = String(new Date(2026, 9, 3, 22, 14, 37).getTime() * 1000);
  const killed = { ...props.snapshot.fights[0], ended: at, target_hp: null, max_hp: null,
    kill: { at, evidence: 'hp_zero_removed' as const, provisional: false } };
  props.snapshot.at = at; props.snapshot.fights = [killed]; props.window.fights = [killed];
  const page = render(RealmHome, props);
  expect(page.container.querySelector('.boss-hp .track')?.classList.contains('unknown')).toBe(false);
  expect(page.container.querySelector('.boss-hp .hp-text')?.textContent).toBe(`Defeated · ${clockTime(at)}`);
  expect(page.container.querySelector('.boss-hp')?.textContent).not.toMatch(/22:14:37|10:14:37/);
  expect(page.container.querySelector('.boss-hp [role="meter"]')).toBeNull();
});
it('new visit has no inherited fame, kills or loot; diagnostics are debug-only', async () => {
  const props = fixture(); const page = render(RealmHome, props);
  const snapshot = realmSnapshot(); snapshot.instance!.visit_fame_delta = null;
  await page.rerender({ snapshot, window: emptyHomeLiveWindow(), visuals: props.visuals });
  expect(page.container.textContent).toContain('No Boss');
  expect(page.container.textContent).toContain('No Kills');
  expect(page.container.textContent).toContain('No Loot');
  expect(page.container.querySelector('[data-kpi="fame"] .value')?.textContent).toBe('—');
  expect(page.container.textContent).not.toMatch(/NaN|undefined|coverage|partial|provisional/i);
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('Visit observations unavailable until the next observed entry');
  expect(page.container.textContent).toContain('Fame this visit unavailable');
});
it('timer and table containers survive numeric churn without changing focus', async () => {
  const props = fixture(); const page = render(RealmHome, props);
  const table = page.container.querySelector('table'), timer = page.container.querySelector('.fight-time');
  const columns = [...page.container.querySelectorAll('col')].map(node => node.getAttribute('style'));
  const next = structuredClone(props.snapshot); next.at = '9999000000';
  next.fights[0].last_hit = next.at; next.fights[0].local_last_hit = next.at;
  next.fights[0].players.forEach(row => row.dps = 9999);
  await page.rerender({ ...props, snapshot: next });
  expect(page.container.querySelector('table')).toBe(table);
  expect(page.container.querySelector('.fight-time')).toBe(timer);
  expect([...page.container.querySelectorAll('col')].map(node => node.getAttribute('style'))).toEqual(columns);
  expect(page.container.querySelector('.fight-target')?.textContent).toContain('Skull Shrine');
});

it.each([false, true])('a newly confirmed damaged boss flashes and its kill slides once; reduced=%s', async reduced => {
  const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'animate');
  const calls: Array<{ node: HTMLElement; frames: Keyframe[]; options: KeyframeAnimationOptions }> = [];
  Object.defineProperty(HTMLElement.prototype, 'animate', { configurable: true, value: function (
    this: HTMLElement, frames: Keyframe[], options: KeyframeAnimationOptions,
  ) { calls.push({ node: this, frames, options }); return { cancel() {}, onfinish: null }; } });
  vi.stubGlobal('matchMedia', () => ({ matches: reduced, addEventListener() {}, removeEventListener() {} }));
  try {
    const props = fixture(); const page = render(RealmHome, props); await tick();
    // Historical kills at mount remain still.
    expect(calls).toHaveLength(0);
    const snapshot = structuredClone(props.snapshot), window = structuredClone(props.window);
    const killed = { ...snapshot.fights[0], ended: snapshot.at, target_hp: 0,
      end_reason: 'killed' as const, kill: { at: snapshot.at, evidence: 'hp_zero_removed' as const, provisional: false } };
    snapshot.fights = [killed]; window.fights = [killed]; window.events = [];
    await page.rerender({ ...props, snapshot, window }); await tick(); await tick();
    if (reduced) expect(calls).toHaveLength(0);
    else {
      expect(calls).toHaveLength(2);
      expect(calls[0].node.classList.contains('realm-fight')).toBe(true);
      expect(calls[0].frames).toContainEqual({ boxShadow: '0 0 14px #f2c14e', offset: .35 });
      expect(calls[1].node.dataset.killId).toBe(realmView(snapshot, window).kills[0].id);
      expect(calls[1].frames[0]).toEqual({ transform: 'translateY(-12px)', opacity: 0 });
      expect(calls.every(call => call.options.duration === 500 && call.options.iterations === 1)).toBe(true);
    }
    const count = calls.length;
    // Numeric churn and the same kill with a later announcement must not replay it.
    window.events = [{ id: 'late-notice', session_id: snapshot.session!.id, run_id: null,
      at: snapshot.at, provisional: true, kind: { type: 'realm_event_killed',
        boss_key: 'Skull Shrine', object_type: 300, killer: 'Torvik' } }];
    await page.rerender({ ...props, snapshot: { ...snapshot, at: '101000000' }, window }); await tick();
    expect(calls).toHaveLength(count);
    // Returning to the retained ended fight also cannot flash it again.
    await page.rerender({ ...props, snapshot, window: { ...window, focusedFightId: null } }); await tick();
    expect(calls).toHaveLength(count); page.unmount();
  } finally {
    if (original) Object.defineProperty(HTMLElement.prototype, 'animate', original);
    else Reflect.deleteProperty(HTMLElement.prototype, 'animate');
    vi.unstubAllGlobals();
  }
});
it.each(['zero damage', 'unverified', 'announcement'] as const)('no kill motion for %s', async kind => {
  const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'animate');
  const animate = vi.fn(() => ({ cancel() {}, onfinish: null }));
  Object.defineProperty(HTMLElement.prototype, 'animate', { configurable: true, value: animate });
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
  try {
    const props = fixture();
    if (kind === 'unverified') props.snapshot.character!.identity_verified = false;
    if (kind === 'zero damage') props.snapshot.fights[0].players[13].damage = 0;
    if (kind === 'announcement') { props.snapshot.fights = []; props.window.fights = []; props.window.events = []; }
    const page = render(RealmHome, props); await tick();
    const snapshot = structuredClone(props.snapshot), window = structuredClone(props.window);
    if (kind === 'announcement') {
      window.events = [{ id: 'announcement-new', session_id: snapshot.session!.id, run_id: null,
        at: snapshot.at, provisional: true, kind: { type: 'realm_event_killed', boss_key: 'Skull Shrine', object_type: 300, killer: 'Torvik' } }];
    } else {
      const killed = { ...snapshot.fights[0], ended: snapshot.at, target_hp: 0,
        kill: { at: snapshot.at, evidence: 'hp_zero_removed' as const, provisional: false } };
      snapshot.fights = [killed]; window.fights = [killed]; window.events = [];
    }
    await page.rerender({ ...props, snapshot, window }); await tick(); await tick();
    expect(animate).not.toHaveBeenCalled(); page.unmount();
  } finally {
    if (original) Object.defineProperty(HTMLElement.prototype, 'animate', original);
    else Reflect.deleteProperty(HTMLElement.prototype, 'animate');
    vi.unstubAllGlobals();
  }
});
it('unknown bag contents render the bag-only feature alongside other bags', () => {
  const props = fixture();
  props.window.drops = props.window.drops.slice(0, 3).map((drop, i) => ({ ...drop,
    bag_rank: 9 - i, contents_known: false, contents: [], items: [] }));
  const page = render(RealmHome, props), loot = page.container.querySelector('.area-loot')!;
  expect(loot.querySelector('.best-card')?.getAttribute('data-drop-id')).toBe(props.window.drops[0].id);
  expect(loot.querySelector('.best-card .item-box')).toBeNull();
  expect(loot.querySelector('.bag-list-root')).toBeTruthy();
  expect(loot.querySelectorAll(`[data-drop-id="${props.window.drops[0].id}"]`)).toHaveLength(1);
  expect(loot.textContent).not.toContain('No Bags');
});

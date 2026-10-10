import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import RealmKills from '../../src/lib/home/v3/RealmKills.svelte';
import type { RealmKillRow } from '../../src/lib/home/realm';
import type { HomeLiveWindow } from '../../src/lib/home/v2/contracts';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { fight, realmSnapshot } from '../home-v2/fixtures';
afterEach(() => vi.unstubAllGlobals());

function fixture() {
  const snapshot = realmSnapshot();
  const fights = Array.from({ length: 6 }, (_, i) => fight(`kill${i}`, { run_id: null,
    started: String(1_000_000 + i * 10_000_000), ended: String(5_000_000 + i * 10_000_000) }));
  const rows: RealmKillRow[] = fights.map((row): RealmKillRow => ({ id: `fight:${row.id}`, targetType: row.target_type,
    bossKey: null, at: row.ended!, evidence: 'hp_zero_removed', provisional: false,
    partial: false, killer: null, fightId: row.id, started: row.started, ended: row.ended,
    self: { ...row.players[0], rank: 2, share: .18, damage: 212000 } })).reverse();
  const window: HomeLiveWindow = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id,
    visitKey: 'v', visitStarted: '0', partial: false, fights };
  return { rows, window, visuals: emptyHomeVisuals(), resolveSpriteUrl: () => null };
}
it('fits two rows in 140px, keeps a counted button and expands keyboard-reachably', async () => {
  let resize: ResizeObserverCallback = () => {};
  const disconnect = vi.fn();
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe() {} unobserve() {} disconnect = disconnect;
  });
  const page = render(RealmKills, fixture());
  resize([{ contentRect: { height: 140 } } as ResizeObserverEntry], {} as ResizeObserver); await tick();
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(2);
  const more = screen.getByRole('button', { name: '+4 more kills ›' });
  more.focus(); await fireEvent.click(more);
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(6);
  const less = screen.getByRole('button', { name: 'Show fewer kills' });
  expect(document.activeElement).toBe(less);
  await fireEvent.click(less);
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(2);
  page.unmount(); expect(disconnect).toHaveBeenCalledOnce();
});

it('refits retained kills when the observed box grows and shrinks after mount', async () => {
  let resize: ResizeObserverCallback = () => {};
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe() {} unobserve() {} disconnect() {}
  });
  const page = render(RealmKills, fixture());
  for (const [height, count, hidden] of [[113, 1, 5], [153.6, 2, 4], [113, 1, 5]]) {
    resize([{ contentRect: { height } } as ResizeObserverEntry], {} as ResizeObserver); await tick();
    expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(count);
    expect(screen.getByRole('button', { name: `+${hidden} more kills ›` })).toBeTruthy();
  }
});
it('announced-only row shows last hit name without damage, rank or duration', () => {
  const props = fixture();
  props.rows = [{ ...props.rows[0], id: 'announcement:a', evidence: 'realm_announcement', self: null,
    killer: 'Torvik', fightId: null, started: null, ended: null }];
  props.window.events = [{ id: 'a', session_id: 'demo-session', run_id: null, at: '5000000', provisional: true,
    kind: { type: 'realm_event_killed', boss_key: 'Hermit', killer: 'Torvik', object_type: 300 } }];
  const page = render(RealmKills, props);
  const row = page.container.querySelector('[data-kill-id]')!;
  expect(row.textContent).toContain('last hit Torvik');
  expect(row.querySelector('.kill-stats')).toBeNull();
  expect(row.textContent).not.toMatch(/fight|damage|rank/i);
});
it('renders supplied contribution and exact fight length; missing share has no fill', async () => {
  const props = fixture(); props.rows = [props.rows[0]];
  const page = render(RealmKills, props);
  const row = page.container.querySelector('[data-kill-id]')!;
  expect(row.textContent).toContain('0:04 fight');
  expect(row.textContent).toContain('#2 · 18%');
  expect(row.querySelector('.kill-stats b')?.textContent).toBe('Rank #2');
  expect(row.querySelector('.kill-stats b .sr-only')?.textContent).toBe('Rank ');
  expect(row.querySelector('.kill-stats b span[aria-hidden="true"]')?.textContent).toBe('#');
  expect(row.querySelector('.medal')?.getAttribute('data-rank')).toBe('2');
  expect(row.textContent).toContain('212,000');
  expect(row.querySelector<HTMLElement>('.share-fill')?.style.width).toBe('18%');
  props.rows = [{ ...props.rows[0], self: { ...props.rows[0].self!, share: null } }];
  await page.rerender(props);
  expect(row.querySelector<HTMLElement>('.share-fill')?.style.width).toBe('0%');
  expect(row.textContent).not.toMatch(/NaN|undefined/);
});
it('renders an unknown rank as a dash without a rank prefix', () => {
  const props = fixture();
  props.rows = [{ ...props.rows[0], self: { ...props.rows[0].self!, rank: null } }];
  const page = render(RealmKills, props);
  const rank = page.container.querySelector('.kill-stats b');
  expect(rank?.textContent).toBe('—');
  expect(rank?.querySelector('.sr-only')).toBeNull();
  expect(rank?.hasAttribute('aria-label')).toBe(false);
});
it('an empty visit has a stable card body', () => {
  const page = render(RealmKills, { ...fixture(), rows: [] });
  expect(page.container.querySelector('.kill-space')).toBeTruthy();
  expect(page.container.textContent).toContain('No Kills');
});

it('reclaims the overflow line when all remaining rows fit the full list height', async () => {
  let resize: ResizeObserverCallback = () => {};
  const observe = vi.fn();
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe = observe; unobserve() {} disconnect() {}
  });
  const props = fixture(); props.rows = props.rows.slice(0, 3);
  const page = render(RealmKills, props);
  // Two 50px rows and their 4px gap fit without a 24px footer + 4px gap.
  resize([{ contentRect: { height: 104 } } as ResizeObserverEntry], {} as ResizeObserver); await tick();
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(1);
  expect(screen.getByRole('button', { name: '+2 more kills ›' })).toBeTruthy();
  await page.rerender({ ...props, rows: props.rows.slice(0, 2) });
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(2);
  expect(screen.queryByRole('button')).toBeNull();
  expect(observe).toHaveBeenCalledWith(page.container.querySelector('.realm-kills'));
  resize([{ contentRect: { height: 103 } } as ResizeObserverEntry], {} as ResizeObserver); await tick();
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(1);
  expect(screen.getByRole('button', { name: '+1 more kill ›' })).toBeTruthy();
  resize([{ contentRect: { height: 127 } } as ResizeObserverEntry], {} as ResizeObserver); await tick();
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(2);
  expect(screen.queryByRole('button')).toBeNull();
});

it('uses singular overflow copy and fits the compact rows before its footer', async () => {
  let resize: ResizeObserverCallback = () => {};
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe() {} unobserve() {} disconnect() {}
  });
  const props = fixture(); props.rows = props.rows.slice(0, 3);
  const page = render(RealmKills, props);
  resize([{ contentRect: { height: 140 } } as ResizeObserverEntry], {} as ResizeObserver); await tick();
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(2);
  expect(screen.getByRole('button', { name: '+1 more kill ›' })).toBeTruthy();
});

it('resizing to zero hides rows and preserves counted overflow', async () => {
  let resize: ResizeObserverCallback = () => {};
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe() {} unobserve() {} disconnect() {}
  });
  const page = render(RealmKills, fixture());
  resize([{ contentRect: { height: 0 } } as ResizeObserverEntry], {} as ResizeObserver); await tick();
  expect(page.container.querySelectorAll('[data-kill-id]')).toHaveLength(0);
  expect(screen.getByRole('button', { name: '+6 more kills \u203a' })).toBeTruthy();
});

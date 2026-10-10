import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import { dungeonView } from '../../src/lib/home/dungeon';
import { railView } from '../../src/lib/shell/rail';
import { homeSnapshot } from './fixtures';

describe('snapshot-only Dungeon Home', () => {
  it('scrolling encounters does not select the current fight', async () => {
    const snapshot = homeSnapshot(), current = snapshot.fights[0];
    snapshot.fights = [current, ...Array.from({ length: 6 }, (_, i) => ({
      ...structuredClone(current), id: `retained-${i}`, ended: '4000000',
    }))];
    const onSelectFight = vi.fn();
    const page = render(DungeonHome, { snapshot, stale: false, onSelectFight });
    const strip = page.container.querySelector<HTMLElement>('.fight-scroll')!;
    strip.scrollBy = vi.fn();
    await fireEvent.click(screen.getByRole('button', { name: /more/ }));
    expect(onSelectFight).not.toHaveBeenCalled();
    expect(strip.scrollBy).toHaveBeenCalledOnce();
    await fireEvent.click(screen.getByRole('tab', { selected: true }));
    expect(onSelectFight).toHaveBeenCalledExactlyOnceWith(current.id);
    const next = structuredClone(snapshot); next.run!.id = 'next-run';
    next.fights.forEach(row => row.run_id = next.run!.id);
    await page.rerender({ snapshot: next, stale: false, onSelectFight });
    expect(screen.getAllByRole('tab')).toHaveLength(8);
  });

  it('unknown_identity_and_zero_duration_never_make_zero_rank_or_nan', async () => {
    const snapshot = homeSnapshot();
    snapshot.character!.identity_verified = false;
    const fight = snapshot.fights[0];
    fight.last_hit = fight.first_hit;
    fight.players[0] = { ...fight.players[0], damage: 0, hits: 0, dps: null, share: null, rank: null };
    fight.total_damage = 0;
    expect(dungeonView(snapshot).self).toBeNull();
    const page = render(DungeonHome, { snapshot, stale: false });
    expect(page.container.querySelector('.damage-row.self')).toBeNull();
    expect(within(page.container.querySelector('table')!).queryByText('You', { exact: true })).toBeNull();
    expect(page.container.textContent).not.toMatch(/NaN|Infinity|Rank 0|0% share/);
    expect(page.container.innerHTML).not.toMatch(/(?:width|height)[^;]*NaN|Infinity/);
    expect(page.container.textContent).toContain("\u2014");
    snapshot.character!.identity_verified = true;
    await page.rerender({ snapshot: structuredClone(snapshot), stale: false });
    expect(dungeonView(snapshot).self?.damage).toBe(0);
    expect(dungeonView(snapshot).self?.rank).toBeNull();
    expect(within(page.container.querySelector('table')!).getByText('You', { exact: true })).toBeTruthy();
    expect(page.container.textContent).not.toMatch(/Rank 0|0% share|NaN|Infinity/);
  });

  it('only a unique verified self is highlighted without mutating supplied local flags', async () => {
    const snapshot = homeSnapshot();
    const original = structuredClone(snapshot);
    const page = render(DungeonHome, { snapshot, stale: false });
    expect(page.container.querySelectorAll('.damage-row.self')).toHaveLength(1);
    expect(snapshot).toEqual(original);
    snapshot.fights[0].players.push({ ...snapshot.fights[0].players[0], player_id: 'invented-second-local', name: 'Second local candidate' });
    await page.rerender({ snapshot: structuredClone(snapshot), stale: false });
    expect(dungeonView(snapshot).self).toBeNull();
    expect(page.container.querySelectorAll('.damage-row.self')).toHaveLength(0);
    expect(within(page.container.querySelector('table')!).queryByText('You', { exact: true })).toBeNull();
    expect(snapshot.fights[0].players.every(player => player.is_local)).toBe(true);
  });

  it('restricts rail, fight and loot to the current run even when an older target is more active', () => {
    const snapshot = homeSnapshot();
    const current = snapshot.fights[0];
    snapshot.fights.push({ ...structuredClone(current), id: 'old-fight', run_id: 'old-run', last_hit: '9999999', total_damage: 999999 });
    snapshot.fights.push({ ...structuredClone(current), id: 'realm-fight', run_id: null, last_hit: '99999999' });
    snapshot.drops.push({ ...snapshot.drops[0], id: 'old-drop', run_id: 'old-run' });
    expect(dungeonView(snapshot).fight?.id).toBe(current.id);
    expect(dungeonView(snapshot).drops.map(drop => drop.id)).toEqual(['demo-drop']);
    expect(railView(snapshot).fight).toEqual(railView(homeSnapshot()).fight);
  });

  it('empty player list has no denominator and partial coverage cannot certify zero deaths', () => {
    const snapshot = homeSnapshot(); snapshot.fights[0].players = [];
    snapshot.run!.complete_capture = false; snapshot.run!.issues = ['stream_gap'];
    expect(dungeonView(snapshot)).toMatchObject({ self: null, observedPlayers: null, personalDeaths: null, partial: true });
    snapshot.run!.died_at = '5000000';
    expect(dungeonView(snapshot).personalDeaths).toBe(1);
    snapshot.fights.push(structuredClone(snapshot.fights[0]));
    expect(dungeonView(snapshot).personalDeaths).toBe(1);
  });

  it('complete known non-death coverage allows zero but never sums copied fight death counters', () => {
    const snapshot = homeSnapshot();
    expect(dungeonView(snapshot).personalDeaths).toBe(0);
    snapshot.run!.died_at = '5000000';
    snapshot.run!.completed_at = '4000000'; snapshot.run!.outcome = 'cleared';
    snapshot.fights[0].players[0].deaths = 1;
    snapshot.fights.push({ ...structuredClone(snapshot.fights[0]), id: 'second-fight' });
    expect(dungeonView(snapshot).personalDeaths).toBe(1);
    expect(snapshot.run!.outcome).toBe('cleared');
  });

  it('preserves supplied rank/share and source estimates', () => {
    const snapshot = homeSnapshot();
    snapshot.fights[0].players[0].rank = 3;
    snapshot.fights[0].players[0].share = 0.123;
    snapshot.fights[0].players[0].sources[0].estimated = true;
    snapshot.fights[0].players[0].sources[0].reasons = ['missing_stats'];
    expect(dungeonView(snapshot).self).toMatchObject({ rank: 3, share: 0.123 });
    const page = render(DungeonHome, { snapshot, stale: false });
    expect(page.container.textContent).not.toMatch(/estimated/i);
    expect(page.container.textContent).not.toMatch(/observed/i);
  });

  it('unknown contents and resource eviction stay visible without catalog calls', () => {
    const snapshot = homeSnapshot();
    snapshot.drops[0] = { ...snapshot.drops[0], contents_known: false, contents: [], items: [], issues: ['resource_limit'] };
    snapshot.issues = ['resource_limit'];
    render(DungeonHome, { snapshot, stale: false });
    expect(screen.queryByText(/Contents unavailable/i)).toBeNull();
    expect(screen.queryByText(/Some observed loot may be missing/i)).toBeNull();
    expect(screen.getByRole('heading', { name: 'LOOT THIS RUN' })).toBeTruthy();
  });

  it('reused object IDs retain distinct keyed rows and known damage ordering', async () => {
    const snapshot = homeSnapshot();
    const local = snapshot.fights[0].players[0];
    snapshot.fights[0].players = [
      { ...local, player_id: 'z', name: 'Synthetic Zed', is_local: false, damage: 100 },
      { ...local, player_id: 'a', name: 'Synthetic Ada', is_local: false, damage: 100 },
      { ...local, player_id: 'b', name: 'Synthetic Bea', is_local: false, damage: 200 },
    ];
    const page = render(DungeonHome, { snapshot, stale: false });
    const rows = [...page.container.querySelectorAll('.damage-row')];
    expect(rows.map(row => row.textContent)).toEqual([
      expect.stringContaining('Synthetic Bea'), expect.stringContaining('Synthetic Ada'), expect.stringContaining('Synthetic Zed'),
    ]);
    snapshot.fights[0].players.reverse();
    await page.rerender({ snapshot: structuredClone(snapshot), stale: false });
    expect([...page.container.querySelectorAll('.damage-row')]).toEqual(rows);
  });

  it.each([NaN, Infinity, -1])('defensively renders invalid numeric values as unavailable (%s)', value => {
    const snapshot = homeSnapshot();
    Object.assign(snapshot.fights[0].players[0], { dps: value, share: value, biggest_hit: value, rank: value });
    const page = render(DungeonHome, { snapshot, stale: false });
    expect(page.container.textContent).not.toMatch(/NaN|Infinity|-1 DPS|-100%/);
    expect(page.container.innerHTML).not.toMatch(/(?:width|height)[^;]*(?:NaN|Infinity)/);
  });
});

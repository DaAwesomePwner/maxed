import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { InstanceEventKind } from '../../src/lib/bindings/InstanceEventKind';
import type { InstanceEventView } from '../../src/lib/bindings/InstanceEventView';
import type { ObjectVisual } from '../../src/lib/bindings/ObjectVisual';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import DungeonTimeline from '../../src/lib/home/v2/DungeonTimeline.svelte';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import {
  MARKER_MERGE_PX, dungeonClusters, dungeonNames, dungeonObservations, dungeonTimeline, markerSummary,
  type DungeonMarker, type MarkerContext,
} from '../../src/lib/home/dungeon';
import { homeSnapshot } from './fixtures';

const unenchanted = { state: 'missing' as const, slots: null, applied: null, ids: [] };
const START = 1_000_000;
const sec = (seconds: number) => String(START + seconds * 1_000_000);
const AT = sec(600);

function event(id: string, seconds: number, kind: InstanceEventKind, provisional = false): InstanceEventView {
  return { id, session_id: 'demo-session', run_id: 'demo-run', at: sec(seconds), kind, provisional };
}
const death = (id: string, seconds: number, name: string, extra: Partial<Extract<InstanceEventKind, { type: 'player_death' }>> = {}) =>
  event(id, seconds, { type: 'player_death', player_name: name, is_local: false, ...extra });
const left = (id: string, seconds: number, name: string | null) =>
  event(id, seconds, { type: 'player_left', object_id: 7, player_name: name });

function drop(id: string, seconds: number, overrides: Partial<DropView> = {}): DropView {
  return { ...homeSnapshot().drops[0], id, at: sec(seconds), bag_type: 500, bag_rank: 1, contents_known: true,
    items: [], contents: [], issues: [], source_estimated: false, ...overrides };
}

function fight(id: string, from: number, until: number | null, overrides: Partial<FightView> = {}): FightView {
  return { ...homeSnapshot().fights[0], id, started: sec(from), ended: until === null ? null : sec(until),
    target_type: 300, target_kind: 'boss', complete_capture: true, issues: [], kill: null, end_reason: null, ...overrides };
}

const objects: ObjectVisual[] = [
  [300, 'Synthetic Overlord'], [500, 'Synthetic Pouch'], [501, 'Synthetic Chest'],
  [600, 'Synthetic Blade'], [601, 'Synthetic Ring'],
].map(([object_type, name]) => ({ object_type: object_type as number, name: name as string, sprite: null, bag: null, item_tier: null }));
const visuals = { ...emptyHomeVisuals(), phase: 'ready' as const, objects };
const context: MarkerContext = { start: sec(0), end: AT, name: type => objects.find(row => row.object_type === type)?.name ?? null };
const marker = (markers: DungeonMarker[], id: string) => markers.find(row => row.id === id)!;

describe('timeline projection keeps notification detail', () => {
  it('keeps death, departure, bag and boss detail and drops ordinary encounters', () => {
    const markers = dungeonTimeline(
      [fight('boss', 60, 105, { kill: { at: sec(105), evidence: 'hp_zero_removed', provisional: false } }),
        fight('ordinary', 10, 20, { target_kind: 'ordinary' }), fight('live-boss', 500, null)],
      [drop('bag', 110, { contents: [
        { slot: 2, item_type: 601, quantity: 1, enchant: unenchanted, taken_observed_at: null },
        { slot: 0, item_type: 600, quantity: 3, enchant: unenchanted, taken_observed_at: null },
      ] }), drop('unseen', 120, { contents_known: false })],
      [death('d', 200, 'TestHero', { level: 17, killed_by: 'Synthetic Guardian', grave_type: 800, class_id: 782, object_id: 9, is_local: true }),
        left('l', 210, 'Synthetic Sable')],
    );
    expect(markers.map(row => row.id)).toEqual(['fight:boss', 'drop:bag', 'drop:unseen', 'event:d', 'event:l', 'fight:live-boss']);
    expect(marker(markers, 'fight:boss').detail).toEqual({ type: 'boss', objectType: 300, outcome: 'killed' });
    expect(marker(markers, 'fight:live-boss').detail).toMatchObject({ outcome: 'live' });
    expect(marker(markers, 'drop:bag').detail).toEqual({ type: 'bag', bagType: 500, bagRank: 1, contentsKnown: true,
      items: [{ itemType: 600, quantity: 3 }, { itemType: 601, quantity: 1 }] });
    expect(marker(markers, 'drop:unseen').detail).toMatchObject({ contentsKnown: false, items: [] });
    expect(marker(markers, 'event:d')).toMatchObject({ objectType: 800, detail: { type: 'death', name: 'TestHero', level: 17,
      killer: 'Synthetic Guardian', classId: 782, graveType: 800, local: true } });
    expect(marker(markers, 'event:l').detail).toEqual({ type: 'left', name: 'Synthetic Sable', classId: null });
  });

  it('ended fights without a kill read as ended, not killed', () => {
    const [row] = dungeonTimeline([fight('gone', 0, 30, { end_reason: 'map_changed' })], [], []);
    expect(row.detail).toMatchObject({ outcome: 'ended' });
    expect(dungeonTimeline([fight('dead', 0, 30, { end_reason: 'killed' })], [], [])[0].detail).toMatchObject({ outcome: 'killed' });
  });

  it('summaries name the player, level, killer, bag contents and boss duration and outcome', () => {
    const markers = dungeonTimeline(
      [fight('boss', 60, 105, { end_reason: 'killed' }), fight('live', 500, null)],
      [drop('bag', 110, { items: [600, 601, 601], contents: [] }), drop('empty', 111), drop('unseen', 112, { contents_known: false })],
      [death('d', 200, 'TestHero', { level: 17, killed_by: 'Synthetic Guardian', is_local: true }),
        death('bare', 201, 'Synthetic Bram'), left('l', 210, 'Synthetic Sable'), left('anon', 211, null)],
    );
    const summary = (id: string) => markerSummary(marker(markers, id), context);
    expect(summary('fight:boss')).toEqual({ title: 'Synthetic Overlord · 1:00', lines: ['Duration 0:45', 'Killed'] });
    expect(summary('fight:live')).toEqual({ title: 'Synthetic Overlord · 8:20', lines: ['Duration 1:40', 'Live'] });
    expect(summary('drop:bag')).toEqual({ title: 'Synthetic Pouch · 1:50', lines: ['Synthetic Blade', 'Synthetic Ring', 'Synthetic Ring'] });
    expect(summary('drop:empty').lines).toEqual(['Empty']);
    expect(summary('drop:unseen').lines).toEqual(['Contents —']);
    expect(summary('event:d')).toEqual({ title: 'TestHero (You) died · 3:20', lines: ['Level 17', 'Killed by Synthetic Guardian'] });
    expect(summary('event:bare').lines).toEqual(['Level —', 'Killed by —']);
    expect(summary('event:l')).toEqual({ title: 'Synthetic Sable left · 3:30', lines: [] });
    expect(summary('event:anon').title).toBe('Player left · 3:31');
  });

  it('header names keep notification order and collapse repeats', () => {
    const snapshot = homeSnapshot();
    const window = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, runId: snapshot.run!.id, visitKey: 'visit',
      events: [left('b', 30, 'Synthetic Sable'), left('a', 10, 'Synthetic Morrow'), left('c', 40, 'Synthetic Sable'), death('d', 20, 'TestHero')] };
    const observations = dungeonObservations(snapshot, window);
    expect(observations.left.names).toEqual(['Synthetic Morrow', 'Synthetic Sable', 'Synthetic Sable']);
    expect(observations.deaths.names).toEqual(['TestHero']);
    expect(dungeonNames(observations.left.names)).toEqual(['Synthetic Morrow', 'Synthetic Sable ×2']);
  });
});

describe('marker merging', () => {
  const points = (...xs: number[]) => xs.map((x, index) => ({ marker: dungeonTimeline([], [drop(`b${index}`, index)], [])[0], x }));

  it('merges below the threshold and separates at it', () => {
    expect(MARKER_MERGE_PX).toBeGreaterThanOrEqual(20);
    expect(MARKER_MERGE_PX).toBeLessThanOrEqual(24);
    expect(dungeonClusters(points(100, 100 + MARKER_MERGE_PX - 0.1)).map(row => row.markers.length)).toEqual([2]);
    expect(dungeonClusters(points(100, 100 + MARKER_MERGE_PX)).map(row => row.markers.length)).toEqual([1, 1]);
    expect(dungeonClusters(points(100, 119.9), 20).map(row => row.markers.length)).toEqual([2]);
    expect(dungeonClusters(points(100, 120), 20).map(row => row.markers.length)).toEqual([1, 1]);
  });

  it('chains neighbours so merged centres never sit closer than the threshold', () => {
    const clusters = dungeonClusters(points(0, 15, 30, 45, 80, 200));
    expect(clusters.map(row => row.markers.map(item => item.id))).toEqual([
      ['drop:b0', 'drop:b1', 'drop:b2', 'drop:b3'], ['drop:b4'], ['drop:b5']]);
    expect(clusters.map(row => row.x)).toEqual([22.5, 80, 200]);
    for (let index = 1; index < clusters.length; index++) expect(clusters[index].x - clusters[index - 1].x).toBeGreaterThanOrEqual(MARKER_MERGE_PX);
    expect(clusters[0].id).toBe('cluster:drop:b0');
    expect(clusters[1].id).toBe('drop:b4');
  });

  it('orders members by time regardless of input order and ignores non-finite positions', () => {
    const [a, b] = points(10, 12);
    expect(dungeonClusters([b, a, { ...a, x: NaN }])[0].markers.map(row => row.id)).toEqual(['drop:b0', 'drop:b1']);
  });
});

describe('Dungeon timeline component', () => {
  const run = { ...homeSnapshot().run!, started: sec(0), ended: null };
  const tooltip = (element: Element) => document.getElementById(element.getAttribute('aria-describedby')!)!;

  it('renders the whole run on one track without paging', () => {
    const drops = Array.from({ length: 30 }, (_, index) => drop(`bag-${index}`, index * 20));
    const page = render(DungeonTimeline, { run, at: AT, partial: false, visuals, markers: dungeonTimeline([], drops, []) });
    expect(screen.queryByRole('button', { name: /Earlier|Later/ })).toBeNull();
    const rendered = [...page.container.querySelectorAll('.marker')];
    expect(rendered.reduce((sum, row) => sum + Number(row.getAttribute('data-count')), 0)).toBe(30);
    expect(page.container.querySelector('[data-now]')).toBeTruthy();
  });

  it('keeps overlapping boss lanes between the compact bag and event rows', () => {
    const markers = dungeonTimeline([fight('a', 60, 120), fight('b', 70, 130)],
      [drop('bag', 100)], [death('death', 110, 'TestHero')]);
    const page = render(DungeonTimeline, { run, at: AT, partial: false, visuals, markers });
    const rows = page.container.querySelectorAll('.track > .row');
    expect(rows).toHaveLength(3);
    expect(rows[0].querySelector('[data-marker-id="drop:bag"]')).toBeTruthy();
    expect(rows[1].querySelectorAll('.span-slot')).toHaveLength(2);
    expect(rows[1].getAttribute('style')).toContain('calc(2 * var(--span-height))');
    expect(rows[2].querySelector('.glyph.death')).toBeTruthy();
    expect(page.container.querySelector('.axis-notes')?.textContent).toContain('0:00');
    expect(page.container.querySelector('.axis-notes')?.textContent).toContain('Now: 10:00');
  });

  it('merges close markers into one counted marker whose tooltip lists every member', async () => {
    // 600 s over the 776 px usable track: 10 s is about 13 px apart, 30 s about 39 px.
    const markers = dungeonTimeline([], [drop('near-a', 100), drop('near-b', 110, { bag_type: 501, bag_rank: 5 }), drop('far', 140)],
      [death('d', 300, 'TestHero', { level: 20, killed_by: 'Synthetic Guardian' }), left('l', 305, 'Synthetic Sable')]);
    const page = render(DungeonTimeline, { run, at: AT, partial: false, visuals, markers });
    const merged = screen.getByRole('link', { name: '2 Bags · 1:40–1:50' });
    expect(merged.getAttribute('data-count')).toBe('2');
    expect(merged.textContent).toContain('2');
    expect(tooltip(merged).getAttribute('role')).toBe('tooltip');
    expect(tooltip(merged).closest('[hidden]')).toBeTruthy();
    await fireEvent.focusIn(merged);
    expect(tooltip(merged).closest('[hidden]')).toBeNull();
    expect([...tooltip(merged).querySelectorAll('[data-tip-marker]')].map(row => row.getAttribute('data-tip-marker'))).toEqual(['drop:near-a', 'drop:near-b']);
    expect(tooltip(merged).textContent).toContain('Synthetic Chest · 1:50');
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(tooltip(merged).closest('[hidden]')).toBeTruthy();

    const single = screen.getByRole('link', { name: 'Synthetic Pouch · 2:20' });
    expect(single.getAttribute('data-count')).toBe('1');

    const people = screen.getByRole('link', { name: '2 Events · 5:00–5:05' });
    expect(people.classList.contains('death')).toBe(true);
    expect(tooltip(people).textContent).toContain('TestHero died · 5:00');
    expect(tooltip(people).textContent).toContain('Level 20');
    expect(tooltip(people).textContent).toContain('Killed by Synthetic Guardian');
    expect(tooltip(people).textContent).toContain('Synthetic Sable left · 5:05');
    expect(page.container.querySelectorAll('.marker')).toHaveLength(3);
  });

  it('shows boss fights as labelled spans with duration and outcome on hover', async () => {
    const markers = dungeonTimeline([fight('boss', 60, 105, { end_reason: 'killed' }), fight('trash', 10, 50, { target_kind: 'ordinary' })], [], []);
    const page = render(DungeonTimeline, { run: { ...run, ended: AT }, at: AT, partial: false, visuals, markers });
    expect(page.container.querySelector('[data-marker-id="fight:trash"]')).toBeNull();
    const span = screen.getByRole('link', { name: 'Synthetic Overlord · 1:00' });
    expect(span.textContent).toContain('Synthetic Overlord');
    await fireEvent.pointerEnter(span.closest('.tooltip-root')!);
    expect(tooltip(span).closest('[hidden]')).toBeNull();
    expect(tooltip(span).textContent).toContain('Duration 0:45');
    expect(tooltip(span).textContent).toContain('Killed');
    expect(page.container.querySelector('[data-now]')).toBeNull();
  });
  it('caps and keeps tooltips inside a scrolling pane narrower than the viewport', async () => {
    const pane = document.body.appendChild(document.createElement('div'));
    pane.style.overflowX = 'auto';
    Object.defineProperty(pane, 'clientWidth', { configurable: true, value: 200 });
    const rects = new Map<Element, Partial<DOMRect>>([[pane, { left: 100, right: 300, width: 200 }]]);
    const original = Element.prototype.getBoundingClientRect;
    Element.prototype.getBoundingClientRect = function () {
      const known = rects.get(this) ?? (this.classList.contains('tooltip-position') ? { left: 250, right: 434, width: 184, top: 0, bottom: 50, height: 50 } : {});
      return { left: 0, right: 0, width: 0, top: 0, bottom: 0, height: 0, x: 0, y: 0, ...known, toJSON() {} } as DOMRect;
    };
    try {
      render(DungeonTimeline, { target: pane, props: { run, at: AT, partial: false, visuals, markers: dungeonTimeline([], [drop('edge', 590)], []) } });
      const marker = screen.getByRole('link', { name: /Synthetic Pouch/ });
      await fireEvent.focusIn(marker);
      const position = tooltip(marker).parentElement as HTMLElement;
      expect(position.style.maxWidth.replace(/\s/g, '')).toBe('min(20rem,184px)');
      // Right edge 434 moves to 300 - 8: the tooltip ends inside the pane.
      expect(position.style.transform).toBe('translateX(-142px)');
    } finally {
      Element.prototype.getBoundingClientRect = original;
      pane.remove();
    }
  });
});

describe('Dungeon header name tooltips', () => {
  it('Departures and Deaths show counts on screen and names on hover or focus', async () => {
    const snapshot = homeSnapshot();
    const liveWindow = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, runId: snapshot.run!.id, visitKey: 'visit',
      events: [left('a', 1, 'Synthetic Morrow'), left('b', 2, 'Synthetic Sable'), death('d', 3, 'TestHero', { killed_by: 'Synthetic Guardian' })] };
    const page = render(DungeonHome, { snapshot, stale: false, liveWindow });
    const cell = (key: string) => page.container.querySelector(`[data-kpi="${key}"]`)!;
    const departures = cell('left').querySelector('[aria-describedby]')!;
    const deaths = cell('deaths').querySelector('[aria-describedby]')!;
    expect(cell('left').querySelector('dd .value')?.textContent).toBe('2');
    expect(cell('deaths').querySelector('dd .value')?.textContent).toBe('1');
    const names = (trigger: Element) => document.getElementById(trigger.getAttribute('aria-describedby')!)!;
    expect(names(departures).closest('[hidden]')).toBeTruthy();
    await fireEvent.focusIn(departures);
    expect(names(departures).closest('[hidden]')).toBeNull();
    expect(names(departures).textContent).toBe('Synthetic Morrow\nSynthetic Sable');
    expect(names(deaths).textContent).toContain('TestHero · Level — · Killed by Synthetic Guardian');
  });

  it('cells without names keep a plain count', () => {
    const snapshot = homeSnapshot();
    const liveWindow = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, runId: snapshot.run!.id, visitKey: 'visit' };
    const page = render(DungeonHome, { snapshot, stale: false, liveWindow });
    expect(page.container.querySelector('.area-kpis [aria-describedby]')).toBeNull();
  });
});

it('v3 timeline excludes quest and miniboss spans while retaining boss event detail', () => {
  const rows = dungeonTimeline([
    fight('boss-v3', 1, 5, { target_kind: 'boss' }),
    fight('event-v3', 2, 6, { target_kind: 'event_boss' }),
    fight('mini-v3', 3, 7, { target_kind: 'miniboss' }),
    fight('quest-v3', 4, 8, { target_kind: 'quest' }),
  ], [drop('bag-v3', 9)], [death('death-v3', 10, 'TestHero', { level: 20, killed_by: 'Guardian' })]);
  expect(rows.map(row => row.id)).toEqual(['fight:boss-v3', 'fight:event-v3', 'drop:bag-v3', 'event:death-v3']);
  expect(rows[3].detail).toMatchObject({ name: 'TestHero', level: 20, killer: 'Guardian' });
});

it.each([null, sec(200)])('stops a confirmed encounter at the kill for live and stored fights (%s)', ended => {
  const markers = dungeonTimeline([fight('boss', 10, null, { ended,
    kill: { at: sec(100), evidence: 'victory_quest_target', provisional: false } })], [], []);
  expect(markers[0].until).toBe(sec(100));
  const provisional = dungeonTimeline([fight('boss', 10, null, {
    kill: { at: sec(100), evidence: 'realm_announcement', provisional: true } })], [], []);
  expect(provisional[0].until).toBeNull();
  expect(provisional[0].detail).toMatchObject({ outcome: 'live' });
});

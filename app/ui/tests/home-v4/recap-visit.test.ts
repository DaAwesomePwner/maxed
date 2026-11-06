import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createRecapPresentation } from '../../src/lib/home/v4/recap-presentation';
import { writable } from 'svelte/store';
import visitEventsSource from '../../src/lib/home/v4/VisitEventsCard.svelte?raw';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import type { RecapState } from '../../src/lib/home/v4/recap';
import type { VisitRecapData } from '../../src/lib/home/v4/recap-reads';
import { visitRecapView, visitRecapVisualTypes, type VisitRecapView } from '../../src/lib/home/v4/recap-view';
import * as recapViews from '../../src/lib/home/v4/recap-view';
import * as recapImages from '../../src/lib/home/v4/recap-image';
import * as clipboard from '../../src/lib/home/v4/clipboard';
import VisitRecapScreen from '../../src/lib/home/v4/VisitRecapScreen.svelte';
import { SHELL_CONNECTION } from '../../src/lib/shell/context';
import Home from '../../src/routes/Home.svelte';
import { homeSnapshot, saved } from '../home/fixtures';
import { fightSummary } from '../runs/fixtures';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
const sprite: SpriteRef = { key: 'synthetic', sheet: 'synthetic.png', x: 0, y: 0, width: 8, height: 8 };

function data(): VisitRecapData {
  const snapshot = homeSnapshot(), fight = snapshot.fights[0], drop = snapshot.drops[0];
  return { kind: 'visit', baselines: null, visit: {
    visit_id: 'live-synthetic-home:visit:4', session_id: 'synthetic-session', character_id: 'synthetic-player', title: 'Synthetic Saved Realm',
    periods: [{ visit_id: 'live-synthetic-home:visit:4', started: '1000000', ended: '65000000' }],
    fights: [fightSummary(1, { id: 'synthetic-boss', target_type: 4300, target_kind: 'event_boss',
      local_dps: 200, local_rank: 2, player_count: 4, first_hit: '1000000', last_hit: '11000000',
      party: { avg_dps: 100, players: 3 } })],
    boss_fights: [{ ...fight, id: 'synthetic-boss', target_type: 4300, target_kind: 'event_boss',
      started: '1000000', ended: '11000000', kill: { at: '11000000', evidence: 'hp_zero_removed', provisional: false },
      players: [{ ...fight.players[0], is_local: true, dps: 200, rank: 2, share: .25 }] }],
    boss_fights_limited: false,
    announcements: [{ id: 'synthetic-announcement', session_id: 'synthetic-session', at: '30000000', provisional: true,
      kind: { type: 'realm_event_killed', object_type: 4301, boss_key: 'synthetic-announced', killer: null } }],
    announcements_limited: false,
    drops: [{ ...drop, id: 'synthetic-estimated', at: '12000000', source_enemy: 4300, source_estimated: true,
      bag_type: 4506, bag_rank: 7, contents_known: true, items: [4507], contents: [{ ...drop.contents[0], item_type: 4507 }] },
    { ...drop, id: 'synthetic-observed', at: '13000000', source_enemy: 4300, source_estimated: false,
      bag_type: 4508, bag_rank: 1, contents_known: false, contents: [], items: [] }],
    drops_limited: false, fame_delta: 12, best_dps: [], chain: null,
  } };
}
function props(view = visitRecapView(data(), 'UTC')) {
  return { view, visuals: { ...emptyHomeVisuals(), objects: [
    [1796, 'Synthetic Realm Portal'], [4300, 'Synthetic Guardian'], [4301, 'Synthetic Announced'],
    [4506, 'Synthetic White Bag'], [4507, 'Synthetic Sword'], [4508, 'Synthetic Brown Bag'],
  ].map(([object_type, name]) => ({ object_type: Number(object_type), name: String(name), sprite, bag: null })) },
  resolveSpriteUrl: () => '/synthetic-sprite.png', onDismiss: vi.fn() };
}
function chain(): NonNullable<VisitRecapView['chain']> {
  return { label: 'Synthetic Realm → Synthetic Castle · inferred', steps: [
    { visit_id: 'synthetic-visit', run_id: null, kind: 'realm', name: 'Synthetic Realm', started: '1000000', ended: '65000000',
      local_dps: null, party_dps: null, bags: 2, whites: 1, item_types: [4510] },
  ], totals: { time: '1:04', bags: 2, whites: 1 } };
}

it('visit_banner_and_time_here', async () => {
  const input = props(), page = render(VisitRecapScreen, input);
  expect(page.getByRole('region', { name: 'Recap' }).classList.contains('visit-recap-screen')).toBe(true);
  expect(page.getByRole('heading', { name: 'Synthetic Saved Realm · REALM VISIT' })).toBeTruthy();
  expect(page.getByText('Time here')).toBeTruthy(); expect(page.getByText('1:04')).toBeTruthy();
  expect(page.getByText('ended 00:01')).toBeTruthy();
  expect(page.queryByRole('img', { name: 'Synthetic Realm Portal' })).toBeNull();
  expect(page.container.querySelector('.recap-banner > .frame img')?.getAttribute('src'))
    .toBe(new URL('/synthetic-sprite.png', window.location.href).href);
  expect(page.queryByText(chain().label)).toBeNull();
  await page.rerender({ view: { ...input.view, chain: chain() } });
  expect(page.getByText(chain().label)).toBeTruthy();
});

it('announced_rows_faded_without_dps', () => {
  const input = props();
  Object.assign(input.view.events.find(row => row.state === 'announced')!, { dps: 9999, rank: 1, players: 9, share: 1 });
  const page = render(VisitRecapScreen, input), row = page.getByRole('row', { name: /Synthetic Announced/ });
  expect(row.classList.contains('announced')).toBe(true);
  expect(visitEventsSource).toMatch(/\.announced\s*\{\s*opacity:\s*\.55;/);
  expect(within(row).getByText("Announced · you weren't there")).toBeTruthy();
  expect(row.textContent).not.toMatch(/DPS|9,999|#1|100%/);
  expect(row.querySelector('.party-sparkline')).toBeNull();
  expect(within(row).getByText("Announced · you weren't there").getAttribute('colspan')).toBe('4');
});

it('event_bags_with_estimate_note', () => {
  const page = render(VisitRecapScreen, props()), row = page.getByRole('row', { name: /Synthetic Guardian/ });
  const estimated = within(row).getByRole('link', { name: 'Synthetic White Bag' });
  expect(estimated.getAttribute('title')).toBe('Linked by estimated source');
  expect(estimated.querySelector('img')?.getAttribute('src')).toBe(new URL('/synthetic-sprite.png', window.location.href).href);
  expect(within(row).getByRole('link', { name: 'Synthetic Brown Bag' }).hasAttribute('title')).toBe(false);
  expect(row.querySelectorAll('[data-drop-id]')).toHaveLength(2);
});

it('event_unknowns_and_single_accessible_names', () => {
  const input = props();
  input.view.events.push({ key: 'synthetic-unknown', targetType: null, name: 'Unknown target', at: null,
    state: 'killed', dps: null, rank: null, players: null, share: null, bags: [], kill: null, vsParty: null, buckets: null, best: false });
  const before = structuredClone(input.view.events), page = render(VisitRecapScreen, input);
  const rows = page.getByRole('region', { name: 'Events this visit' }).querySelectorAll('tbody tr');
  expect([...rows].map(row => row.getAttribute('data-event-id'))).toEqual([
    input.view.events[0].key, input.view.events[1].key, 'synthetic-unknown',
  ]);
  expect(rows[1].textContent).toContain('200');
  expect(rows[2].textContent).toContain('—'); expect(rows[2].textContent).not.toMatch(/\b0\b|NaN|undefined/);
  expect(within(rows[2] as HTMLElement).getAllByRole('cell')[2].textContent).toBe('—');
  expect(within(rows[1] as HTMLElement).queryByRole('img', { name: 'Synthetic Guardian' })).toBeNull();
  expect(page.container.querySelector('[role="group"][aria-label]')).toBeNull();
  expect(input.view.events).toEqual(before);
});

it('killed_event_rows_use_fight_by_fight_columns', () => {
  const savedData = data();
  savedData.visit.boss_fights[0].buckets = [{ from: '1000000', until: '2000000', total: 300, local: 200,
    others_damage: 100, others_presence: 1 }];
  savedData.baselines = { rows: [{ target_type: 4300, fights: 4, avg_dps: 100, best_dps: 200,
    kill_samples: 3, median_kill_micros: 15000000 }], truncated: false };
  const page = render(VisitRecapScreen, props(visitRecapView(savedData, 'UTC')));
  const region = page.getByRole('region', { name: 'Events this visit' }), row = within(region).getByRole('row', { name: /Synthetic Guardian/ });
  expect(within(region).getAllByRole('columnheader').map(cell => cell.textContent)).toEqual([
    'Event', 'Your DPS', 'vs party', 'Rank', 'DPS over fight', 'Bags',
  ]);
  expect(within(row).getByText('0:10 · 5 s faster than usual')).toBeTruthy();
  expect(row.querySelector('time')?.textContent).not.toBe('—');
  expect(row.querySelector('.party-sparkline polyline.you')).not.toBeNull();
  expect(row.querySelector('.party-sparkline polyline.party')).not.toBeNull();
  expect(within(region).getByText('You')).toBeTruthy(); expect(within(region).getByText('Party average')).toBeTruthy();
  expect(within(row).getByText('+100%').getAttribute('aria-hidden')).toBe('true');
  const comparison = within(row).getByText('100% above party average');
  expect(comparison.classList.contains('sr-only')).toBe(true);
  expect(comparison.closest('[data-tone]')?.getAttribute('data-tone')).toBe('up');
  const readable = comparison.closest('td')!.cloneNode(true) as HTMLElement;
  readable.querySelectorAll('[aria-hidden="true"]').forEach(node => node.remove());
  expect(readable.textContent?.trim()).toBe('100% above party average');
  expect(comparison.closest('td')!.querySelector('[role="group"], [aria-label]')).toBeNull();
  const bestRank = page.getByText('Best rank').closest('dl')!.querySelector('.recap-tile-value')!.textContent!;
  expect(bestRank).toBe('#2 of 4'); expect(within(row).getByText(bestRank)).toBeTruthy();
  expect(within(row).getByText('25% of damage')).toBeTruthy();
  expect(row.querySelectorAll('[data-drop-id]')).toHaveLength(2);
});

it('event_row_without_party_line_says_not_recorded', () => {
  const savedData = data();
  savedData.visit.boss_fights[0].buckets = [{ from: '1000000', until: '2000000', total: 300, local: 200 }];
  const page = render(VisitRecapScreen, props(visitRecapView(savedData))), row = page.getByRole('row', { name: /Synthetic Guardian/ });
  const sentence = within(row).getByText('Party average line not recorded for this fight');
  expect(sentence.classList.contains('sr-only')).toBe(true);
  expect(within(row).getByText('Not recorded').getAttribute('aria-hidden')).toBe('true');
  const readable = sentence.closest('td')!.cloneNode(true) as HTMLElement;
  readable.querySelectorAll('[aria-hidden="true"]').forEach(node => node.remove());
  expect(readable.textContent?.trim()).toBe('Party average line not recorded for this fight');
  expect(row.querySelector('.party-sparkline')).toBeNull();
  expect(page.getByRole('region', { name: 'Events this visit' }).querySelector('.recap-legend')).toBeNull();
});

it('rank_cell_carries_damage_share', async () => {
  const input = props(), page = render(VisitRecapScreen, input);
  const row = page.getByRole('row', { name: /Synthetic Guardian/ });
  const rank = within(row).getAllByRole('cell')[2];
  expect(within(rank).getByText('#2 of 4')).toBeTruthy();
  const share = within(rank).getByText('25% of damage');
  expect(share.classList.contains('recap-row-detail')).toBe(true);
  expect(rank.lastElementChild).toBe(share);
  await page.rerender({ view: { ...input.view, events: input.view.events.map(event => ({ ...event, share: null })) } });
  expect(rank.textContent?.trim()).toBe('#2 of 4');
  expect(row.textContent).not.toContain('of damage');
});

it('visit_sparkline_is_narrower', () => {
  const savedData = data();
  savedData.visit.boss_fights[0].buckets = [{ from: '1000000', until: '2000000', total: 300, local: 200,
    others_damage: 100, others_presence: 1 }];
  const page = render(VisitRecapScreen, props(visitRecapView(savedData, 'UTC')));
  const sparkline = page.getByRole('row', { name: /Synthetic Guardian/ }).querySelector('.party-sparkline');
  expect(sparkline?.getAttribute('width')).toBe('96');
  expect(sparkline?.getAttribute('height')).toBe('28');
});

it.each([
  [100, '0%', '0% vs party average', 'neutral'],
  [50, '−50%', '50% below party average', 'down'],
  [null, '—', null, null],
] as const)('event_party_comparison_preserves_tone_and_unknowns: %s', (dps, visible, sentence, tone) => {
  const savedData = data(); savedData.visit.fights[0].local_dps = dps;
  const page = render(VisitRecapScreen, props(visitRecapView(savedData))), row = page.getByRole('row', { name: /Synthetic Guardian/ });
  const cell = within(row).getAllByRole('cell')[1];
  expect(cell.getAttribute('data-tone')).toBe(tone);
  if (sentence) {
    expect(within(cell).getByText(visible).getAttribute('aria-hidden')).toBe('true');
    expect(within(cell).getByText(sentence).classList.contains('sr-only')).toBe(true);
  } else expect(cell.textContent).toBe('—');
});

it('visit_cards_are_named_by_unique_visible_headings', () => {
  const pages = [render(VisitRecapScreen, props()), render(VisitRecapScreen, props())];
  const ids: string[] = [];
  for (const page of pages) for (const name of ['Events this visit', 'Loot this visit']) {
    const region = within(page.container).getByRole('region', { name });
    const heading = within(region).getByRole('heading', { level: 3, name });
    expect(heading.id).not.toBe(''); expect(region.getAttribute('aria-labelledby')).toBe(heading.id);
    expect(region.hasAttribute('aria-label')).toBe(false); ids.push(heading.id);
  }
  expect(new Set(ids).size).toBe(ids.length);
});

it('visit_tiles', () => {
  const page = render(VisitRecapScreen, props());
  const tiles = [...page.container.querySelectorAll('.recap-tiles dl')];
  expect(tiles.map(tile => tile.querySelector('dt')?.textContent)).toEqual([
    'Your DPS', 'vs party avg', 'Events killed', 'Best rank', 'Fame this visit',
  ]);
  expect(tiles[0].querySelector('dd')?.textContent?.trim()).toBe('200');
  expect(tiles[1].querySelector('.sr-only')?.textContent).toBe('100% above party average');
  expect(tiles[1].querySelector('[aria-hidden="true"]')?.textContent).toBe('+100%');
  expect(tiles[2].querySelector('.recap-tile-value')?.textContent).toBe('1');
  expect(tiles[2].querySelector('.recap-tile-detail')?.textContent).toBe('+1 announced');
  expect(tiles[3].textContent).toContain('#2 of 4'); expect(tiles[4].textContent).toContain('+12');
});

it('unknown_time_and_metrics_remain_unknown', () => {
  const savedData = data(); savedData.visit.periods = []; savedData.visit.fights = [];
  savedData.visit.fame_delta = null; savedData.visit.boss_fights = []; savedData.visit.announcements = [];
  const page = render(VisitRecapScreen, props(visitRecapView(savedData)));
  expect(page.container.querySelector('.recap-clear-time')?.textContent?.trim()).toBe('—');
  const values = [...page.container.querySelectorAll('.recap-tile-value')].map(value => value.textContent);
  expect(values).toEqual(['—', '—', '0', '—', '—']); // A known empty event list is zero.
  expect(page.getByText('No events observed')).toBeTruthy();
});

it('partial_events_follow_tile_key_after_reordering_and_relabeling', () => {
  const input = props(); input.view.tiles.reverse();
  Object.assign(input.view.tiles.find(tile => tile.key === 'events')!, { partial: true, label: 'Synthetic event count' });
  const page = render(VisitRecapScreen, input);
  expect(within(page.getByRole('region', { name: 'Events this visit' })).getByText('Partial event history')).toBeTruthy();
});

it('visit_loot_includes_best_and_remaining_bags', () => {
  const page = render(VisitRecapScreen, props()), loot = page.getByRole('region', { name: 'Loot this visit' });
  expect(within(loot).getByText('Synthetic Sword')).toBeTruthy();
  const bags = within(loot).getByRole('region', { name: 'All bags' });
  expect(within(bags).getByRole('img', { name: 'Synthetic Brown Bag' })).toBeTruthy();
  expect(bags.querySelectorAll('[data-drop-id]')).toHaveLength(1);
});

it('visit_marks_partial_loot_and_records_on_the_saved_screen', async () => {
  const savedData = data(); savedData.visit.drops_limited = true;
  savedData.visit.best_dps = [{ fight_id: 'synthetic-boss', boss_type: 4300, dps: 200, previous_best_dps: 100 }];
  savedData.visit.fights[0].target_name = 'Synthetic Guardian';
  const page = render(VisitRecapScreen, props(visitRecapView(savedData)));
  expect(within(page.getByRole('region', { name: 'Loot this visit' })).getByText('Partial loot history')).toBeTruthy();
  expect(within(page.getByRole('row', { name: /Synthetic Guardian/ })).getByText('BEST DPS')).toBeTruthy();
  expect(within(page.getByRole('row', { name: /Synthetic Announced/ })).queryByText('BEST DPS')).toBeNull();
  expect(page.getByText('Best DPS on Synthetic Guardian')).toBeTruthy();
  savedData.visit.drops_limited = false;
  await page.rerender({ view: visitRecapView(savedData) });
  expect(page.queryByText('Partial loot history')).toBeNull();
});

it('no_run_story_link', async () => {
  const input = props(), page = render(VisitRecapScreen, input);
  expect(page.queryByRole('link', { name: /Open run story|Full party meter/ })).toBeNull();
  expect(page.getByText('Available from Last recap until the next recap')).toBeTruthy();
  await fireEvent.click(page.getByRole('button', { name: 'Back' }));
  expect(input.onDismiss).toHaveBeenCalledOnce();
});

it('visit_visual_types_include_all_saved_evidence_once', () => {
  const view = visitRecapView(data()), drop = view.drops[0];
  view.events.push({ ...view.events[0], key: 'synthetic-extra', targetType: null,
    bags: [{ dropId: 'synthetic-event-only', bagType: 4511, estimated: false }] });
  view.drops.push({ ...drop, contents: [], items: [9999], contents_known: false });
  view.drops[0].items.push(4509); view.chain = chain();
  view.best = { drop: { ...drop, bag_type: 4512, contents: [], items: [4513], contents_known: true } };
  const before = structuredClone(view);
  expect(visitRecapVisualTypes(view).sort((a, b) => a - b)).toEqual([1796, 4300, 4301, 4506, 4507, 4508, 4509, 4510, 4511, 4512, 4513]);
  expect(view).toEqual(before);
});

async function home(options: { ready?: boolean; redesign?: boolean; title?: string; fail?: boolean } = {}) {
  const snapshot = homeSnapshot({ area_kind: 'nexus', map: 'Nexus', run: null, fights: [], drops: [] });
  const client = createClient(createFixtureTransport(snapshot));
  const read = vi.spyOn(client, 'getVisitRecap');
  if (options.fail) read.mockRejectedValue(new Error('Synthetic read failure'));
  else read.mockResolvedValue({ status: 'ready', value: data().visit });
  vi.spyOn(client, 'getHomeBossBaselines').mockResolvedValue({ status: 'ready', value: { rows: [], truncated: false } });
  const visuals = vi.spyOn(client, 'getObjectVisuals').mockResolvedValue({ client_fingerprint: 'synthetic', objects: [] });
  const recap: RecapState = { watermark: '13',
    source: { kind: 'visit', streamId: snapshot.stream_id, visitKey: 'synthetic-visit', enteredSequence: '4' },
    summary: { kind: 'visit', title: options.title ?? 'Synthetic Live Realm', outcome: null, startedAt: '1000000',
      endedAt: '65000000', localDps: null, rank: null, players: null, fights: [], best: null, runId: null } };
  const recapPresentation = createRecapPresentation(); recapPresentation.remember('synthetic-visit');
  const homeRecap = writable<RecapState | null>(recap), dismiss = vi.fn(recapPresentation.back);
  const page = render(Home, { props: { client, snapshot, store: saved(options.ready === false ? '12' : '99'),
    redesign: options.redesign ?? true }, context: new Map([[SHELL_CONNECTION, { homeRecap, recapPresentation, dismissHomeRecap: dismiss }]]) });
  await settle();
  if (options.redesign !== false) {
    await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  }
  return { page, read, visuals, snapshot, dismiss };
}

it('home_renders_visit_recap_when_saved', async () => {
  const build = vi.spyOn(recapViews, 'visitRecapView'), types = vi.spyOn(recapViews, 'visitRecapVisualTypes');
  const { page, read, visuals, snapshot, dismiss } = await home();
  expect(page.container.querySelector('.visit-recap-screen')).not.toBeNull();
  expect(page.container.querySelector('.recap-placeholder')).toBeNull();
  expect(page.getByRole('heading', { name: 'Synthetic Live Realm · REALM VISIT' })).toBeTruthy();
  expect(build).toHaveBeenCalledWith(expect.objectContaining({ kind: 'visit' }), undefined, 'Synthetic Live Realm');
  expect(visuals.mock.calls.flatMap(([types]) => types)).toEqual(expect.arrayContaining([1796, 4300, 4301, 4506, 4507, 4508]));
  const before = page.container.querySelector('.visit-recap-screen')!.textContent;
  await page.rerender({ snapshot: { ...snapshot, sequence: '15', at: '67000000' } }); await settle();
  await page.rerender({ capture: { ...snapshot.capture, state: 'Recovering' } }); await vi.advanceTimersByTimeAsync(2000);
  expect(page.container.querySelector('.visit-recap-screen')!.textContent).toBe(before);
  expect(build).toHaveBeenCalledOnce(); expect(types).toHaveBeenCalledOnce(); expect(read).toHaveBeenCalledOnce();
  await fireEvent.click(page.getByRole('button', { name: 'Back' })); await settle();
  expect(dismiss).toHaveBeenCalledOnce(); expect(page.container.querySelector('.visit-recap-screen')).toBeNull();
});

it('home_visit_uses_saved_title_when_live_title_unknown', async () => {
  const { page } = await home({ title: '—' });
  expect(page.getByRole('heading', { name: 'Synthetic Saved Realm · REALM VISIT' })).toBeTruthy();
});

it('home_visit_keeps_saving_and_partial_placeholder_until_store_catches_up', async () => {
  const { page, read } = await home({ ready: false });
  expect(page.getByText('Saving visit…')).toBeTruthy(); expect(read).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(5000);
  expect(page.getByText('Partial')).toBeTruthy(); expect(page.container.querySelector('.visit-recap-screen')).toBeNull();
  expect(page.getByRole('button', { name: 'Retry' })).toBeTruthy();
  await page.rerender({ store: saved('99') }); await settle();
  expect(page.container.querySelector('.visit-recap-screen')).not.toBeNull();
  expect(read).toHaveBeenCalledOnce();
});

it('home_visit_failed_load_retries_from_placeholder', async () => {
  const { page, read } = await home({ fail: true });
  expect(page.getByText('Partial')).toBeTruthy(); expect(page.container.querySelector('.visit-recap-screen')).toBeNull();
  read.mockResolvedValue({ status: 'ready', value: data().visit });
  await fireEvent.click(page.getByRole('button', { name: 'Retry' })); await settle();
  expect(page.container.querySelector('.visit-recap-screen')).not.toBeNull(); expect(read).toHaveBeenCalledTimes(2);
});

it('flag_off_keeps_v3_and_does_not_read_visit_recap', async () => {
  const { page, read } = await home({ redesign: false });
  expect(page.container.querySelector('[data-home="v3"] .nexus-home')).not.toBeNull();
  expect(page.container.querySelector('.visit-recap-screen')).toBeNull(); expect(read).not.toHaveBeenCalled();
});

it('visit_copy_footer_requires_matching_verified_identity', async () => {
  const model = vi.spyOn(recapImages, 'recapImageModel');
  vi.spyOn(recapImages, 'renderRecapImage').mockResolvedValue(new Blob(['synthetic'], { type: 'image/png' }));
  vi.spyOn(clipboard, 'copyPng').mockResolvedValue('copied');
  const { page, snapshot, visuals } = await home();
  visuals.mockResolvedValue({ client_fingerprint: 'synthetic', objects: [
    { object_type: 900, name: 'Synthetic Knight', sprite: null, bag: null },
  ] });
  expect(model).not.toHaveBeenCalled();
  for (const [id, verified, name] of [
    ['synthetic-player', true, 'Synthetic Hero'], ['another-synthetic-player', true, null], ['synthetic-player', false, null],
  ] as const) {
    await page.rerender({ snapshot: { ...snapshot, character: { ...snapshot.character!, id, identity_verified: verified,
      name: 'Synthetic Hero', class_id: 900 } } }); await settle();
    await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
    expect(model).toHaveBeenLastCalledWith(expect.objectContaining({ characterId: 'synthetic-player' }),
      { names: true, loot: true, fights: true }, expect.objectContaining({ character: name,
        className: name === null ? null : 'Synthetic Knight', date: recapImages.recapDate('65000000') }));
  }
});

it('flag_off_has_no_copy_control_or_image_work', async () => {
  const model = vi.spyOn(recapImages, 'recapImageModel'), raster = vi.spyOn(recapImages, 'renderRecapImage');
  const { page } = await home({ redesign: false });
  expect(page.queryByRole('button', { name: 'Copy recap image' })).toBeNull();
  expect(model).not.toHaveBeenCalled(); expect(raster).not.toHaveBeenCalled();
});

import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createRecapPresentation } from '../../src/lib/home/v4/recap-presentation';
import { readable } from 'svelte/store';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import type { RunRecapData } from '../../src/lib/home/v4/recap-reads';
import { runRecapView, type RunRecapView } from '../../src/lib/home/v4/recap-view';
import * as recapViews from '../../src/lib/home/v4/recap-view';
import * as recapImages from '../../src/lib/home/v4/recap-image';
import * as clipboard from '../../src/lib/home/v4/clipboard';
import RecapScreen from '../../src/lib/home/v4/RecapScreen.svelte';
import PartySparkline from '../../src/lib/home/v4/PartySparkline.svelte';
import { productionFeatures, routeHref } from '../../src/lib/navigation/routes';
import Shell from '../../src/lib/shell/Shell.svelte';
import { SHELL_CONNECTION } from '../../src/lib/shell/context';
import Home from '../../src/routes/Home.svelte';
import { homeSnapshot, saved } from '../home/fixtures';
import { fakeHistory } from '../navigation/fake-history';
import { fightSummary } from '../runs/fixtures';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);

function data(): RunRecapData {
  const snapshot = homeSnapshot(), fight = snapshot.fights[0];
  const local = { ...fight.players[0], is_local: true, name: 'Synthetic A', dps: 200, rank: 1, share: .5, deaths: 1 };
  return { kind: 'run', recordsUnavailable: false, story: {
    run: { ...snapshot.run!, dungeon: 'Synthetic Sanctum', started: '1000000', completed_at: '61000000', ended: '65000000', outcome: 'cleared' },
    character: snapshot.character, fights: [fightSummary(1, { id: 'saved-boss', target_type: 4300, target_name: 'Synthetic Guardian',
      target_kind: 'boss', local_dps: 200, local_rank: 1, player_count: 4, first_hit: '1000000', last_hit: '11000000',
      party: { avg_dps: 100, players: 3 } })],
    boss_fights: [{ ...fight, id: 'saved-boss', players: [local], buckets: [
      { from: '1000000', until: '2000000', total: 300, local: 200, others_damage: 100, others_presence: 1 },
      { from: '2000000', until: '3000000', total: 500, local: 300, others_damage: 200, others_presence: 1 },
    ] }], boss_fights_limited: false,
    drops: [{ ...snapshot.drops[0], id: 'synthetic-saved-drop', bag_type: 4506, bag_rank: 7, items: [4507],
      contents: [{ ...snapshot.drops[0].contents[0], item_type: 4507 }], contents_known: true }], timeline: [], comparison: null,
    totals: { fights: 1, kills: 1, total_damage: 4000, unattributed_damage: 0, local,
      players: [local, ...['B', 'C', 'D'].map((name, i) => ({ ...local, player_id: `synthetic-${name}`, name: `Synthetic ${name}`,
        is_local: false, rank: i + 2, share: .1, deaths: 0 }))], fame_delta: 12, partial: false },
  }, extras: { run_id: snapshot.run!.id, clear_number: 38, in_a_row: 3, chain: null,
    pb_clear: { clear_micros: 60000000, previous_best_micros: 65000000 }, dry_streak_end: { runs: 8, had_previous_white: true },
    best_dps: [{ fight_id: 'saved-boss', boss_type: 4300, dps: 200, previous_best_dps: 100 }],
    milestones: [{ kind: 'new_items', item_types: [4507], more: 0 }] },
  profile: { dungeon: 'Synthetic Sanctum', clears: 38, usual_run_dps: 100, run_dps_samples: 4,
    usual_clear_micros: 65000000, clear_samples: 4, bosses: [] }, baselines: null,
  streak: { dungeon: 'Synthetic Sanctum', runs_ended: 38, runs_since_last_white: 0, avg_runs_per_white: 8, last_white_at: '65000000' } };
}
const props = (view = runRecapView(data(), 'UTC')) => ({ view, visuals: emptyHomeVisuals(), portalType: null,
  resolveSpriteUrl: () => '', onDismiss: vi.fn() });
function chain(): NonNullable<RunRecapView['chain']> {
  return { label: 'Synthetic Realm → Synthetic Sanctum · inferred', steps: [
    { visit_id: 'synthetic-realm', run_id: null, kind: 'realm', name: 'Synthetic Realm', started: '0', ended: '30000000',
      local_dps: null, party_dps: null, bags: 2, whites: 0, item_types: [] },
    { visit_id: 'synthetic-sanctum', run_id: 'synthetic-run', kind: 'sanctuary', name: 'Synthetic Sanctum', started: '30000000', ended: '90000000',
      local_dps: 200, party_dps: 100, bags: 1, whites: 1, item_types: [4507] },
  ], totals: { time: '1:30', bags: 3, whites: 1 } };
}

it('renders_banner_tiles_fights_loot_chips_party', () => {
  const page = render(RecapScreen, props());
  expect(page.getByRole('region', { name: 'Recap' }).classList.contains('recap-screen')).toBe(true);
  expect(page.getByRole('heading', { name: 'Synthetic Sanctum · Cleared' })).toBeTruthy();
  expect(page.container.querySelector('[data-outcome="cleared"]')).not.toBeNull();
  expect(page.getByText('1:00')).toBeTruthy();
  expect(page.getByText(/Clear #38 · 3rd Synthetic Sanctum in a row tonight/)).toBeTruthy();
  expect(page.container.querySelectorAll('.recap-tiles dl')).toHaveLength(5);
  expect(page.getByRole('region', { name: 'Fight by fight' })).toBeTruthy();
  expect(page.getByRole('region', { name: 'Loot' }).textContent).toContain('8');
  expect(page.getByRole('list', { name: 'Run highlights' }).textContent).toContain('New in collection');
  expect(page.getByText('Synthetic A')).toBeTruthy(); expect(page.getByText('50%')).toBeTruthy();
  expect(page.getByRole('region', { name: 'Party leaders' })).toBeTruthy();
  expect(page.getByText('1 more')).toBeTruthy();
});

it('pb_chip_only_when_pb', async () => {
  const input = props(), page = render(RecapScreen, input);
  expect(page.getByText('PB', { exact: true })).toBeTruthy();
  expect(page.container.querySelector('[data-kind="pb"]')?.textContent?.trim()).toBe('PB clear 1:00 · 0:05 faster');
  await page.rerender({ view: { ...input.view, pb: false } });
  expect(page.queryByText('PB', { exact: true })).toBeNull();
  expect(page.container.querySelector('[data-kind="pb"]')?.textContent?.trim()).toBe('PB clear 1:00 · 0:05 faster');
  const chips = input.view.chips.filter(chip => chip.kind !== 'pb');
  await page.rerender({ view: { ...input.view, chips } });
  expect(page.getByText('PB', { exact: true })).toBeTruthy();
  expect(page.container.querySelector('[data-kind="pb"]')).toBeNull();
  await page.rerender({ view: { ...input.view, pb: false, chips } });
  expect(page.queryByText('PB', { exact: true })).toBeNull();
  expect(page.container.querySelector('[data-kind="pb"]')).toBeNull();
});

it('best_dps_tag_on_matching_row', () => {
  const input = props(); input.view.fights.push({ ...input.view.fights[0], fightId: 'other', name: 'Synthetic Other', best: false });
  const page = render(RecapScreen, input);
  expect(within(page.getByRole('row', { name: /Synthetic Guardian/ })).getByText('Best DPS')).toBeTruthy();
  expect(within(page.getByRole('row', { name: /Synthetic Other/ })).queryByText('Best DPS')).toBeNull();
  expect(page.getAllByText('Party average', { exact: true })).toHaveLength(1);
});

it('run_cards_are_named_by_unique_visible_headings', () => {
  const pages = [render(RecapScreen, props()), render(RecapScreen, props())];
  const ids: string[] = [];
  for (const page of pages) for (const name of ['Fight by fight', 'Loot']) {
    const region = within(page.container).getByRole('region', { name });
    const heading = within(region).getByRole('heading', { level: 3, name });
    expect(heading.id).not.toBe(''); expect(region.getAttribute('aria-labelledby')).toBe(heading.id);
    expect(region.hasAttribute('aria-label')).toBe(false); ids.push(heading.id);
  }
  expect(new Set(ids).size).toBe(ids.length);
});

it('sparkline_absent_shows_party_line_not_recorded', () => {
  const input = props(); input.view.fights[0].buckets = null;
  const page = render(RecapScreen, input);
  expect(page.getByText('Party average line not recorded for this fight')).toBeTruthy();
  expect(page.container.querySelector('.party-sparkline')).toBeNull();
  expect(page.container.querySelector('.recap-legend')).toBeNull();
});

it('sparkline_breaks_at_nulls_and_time_gaps', () => {
  const bucket = data().story.boss_fights![0].buckets[0];
  const page = render(PartySparkline, { buckets: [bucket,
    { ...bucket, from: '2000000', until: '3000000', local: null, others_presence: 0 },
    { ...bucket, from: '3000000', until: '4000000' },
    { ...bucket, from: '6000000', until: '7000000' }] });
  const svg = page.container.querySelector('svg')!;
  expect(svg.getAttribute('aria-hidden')).toBe('true'); expect(svg.getAttribute('viewBox')).toBe('0 0 120 28');
  expect(svg.querySelectorAll('polyline.you')).toHaveLength(3);
  expect(svg.querySelectorAll('polyline.party')).toHaveLength(3);
  expect(svg.textContent).toBe(''); expect(svg.outerHTML).not.toMatch(/NaN|Infinity/);
});

it('chain_strip_only_with_chain', async () => {
  const input = props(), page = render(RecapScreen, input);
  expect(page.queryByRole('region', { name: 'Chain' })).toBeNull();
  await page.rerender({ view: { ...input.view, chain: chain() } });
  expect(page.getByRole('region', { name: 'Chain' })).toBeTruthy();
  expect(page.getByText(chain().label)).toBeTruthy();
});

it('chain_strip_totals', () => {
  const input = props(); input.view.chain = chain(); const page = render(RecapScreen, input);
  const strip = within(page.getByRole('region', { name: 'Chain' }));
  expect(strip.getByText('Chain total · 1:30 · 3 bags · 1 white')).toBeTruthy();
  expect(strip.getAllByRole('row')).toHaveLength(3);
  expect(strip.getByRole('row', { name: /Synthetic Realm/ }).textContent).toContain('—');
  expect(strip.getByRole('row', { name: /Synthetic Sanctum/ }).textContent).toContain('+100%');
});

it('fights_partial_note_when_limited', async () => {
  const input = props(), page = render(RecapScreen, input);
  expect(page.queryByText('Partial fight history')).toBeNull();
  await page.rerender({ view: { ...input.view, fightsPartial: true } });
  expect(page.getByText('Partial fight history')).toBeTruthy();
});

it('run_story_and_party_links', () => {
  const input = props(), page = render(RecapScreen, input);
  for (const name of ['Open run story ›', 'Full party meter ›']) {
    expect(page.getByRole('link', { name }).getAttribute('href')).toBe(routeHref({ kind: 'run', id: input.view.runId }));
  }
});

it('back_closes_recap', async () => {
  const input = props(), page = render(RecapScreen, input);
  await fireEvent.click(page.getByRole('button', { name: 'Back' })); expect(input.onDismiss).toHaveBeenCalledOnce();
  expect(page.getByText('Available from Last recap until the next recap')).toBeTruthy();
});

it('tile_comparisons_are_read_once', () => {
  const page = render(RecapScreen, props());
  for (const [label, sentence, visible] of [
    ['Your DPS', '100% above your usual', '▲ 100% vs your usual'],
    ['vs party avg', '100% above party average', '+100%'],
  ]) {
    const tile = within(page.container.querySelector('.recap-tiles') as HTMLElement).getByText(label, { exact: true }).closest('dl')!;
    const accessible = tile.cloneNode(true) as HTMLElement;
    accessible.querySelectorAll('[aria-hidden="true"]').forEach(node => node.remove());
    expect(accessible.textContent!.split(sentence)).toHaveLength(2);
    expect(tile.querySelector('.sr-only')?.textContent).toBe(sentence);
    expect(tile.querySelector('[aria-hidden="true"]')?.textContent).toBe(visible);
    expect(tile.querySelector('[role="group"], [aria-labelledby]')).toBeNull();
  }
});

it('tile_comparisons_do_not_depend_on_label_copy', () => {
  const input = props();
  input.view.tiles[0].label = 'Synthetic DPS label';
  input.view.tiles[1].label = 'Synthetic party label';
  const page = render(RecapScreen, input);
  expect(page.getByText('100% above your usual').classList.contains('sr-only')).toBe(true);
  const tiles = within(page.container.querySelector('.recap-tiles')! as HTMLElement);
  expect(tiles.getByText('100% above party average').classList.contains('sr-only')).toBe(true);
});

async function home(durable = true, away = false) {
  const snapshot = homeSnapshot(), transport = createFixtureTransport(snapshot), client = createClient(transport), savedData = data();
  vi.spyOn(client, 'queryStore').mockResolvedValue(saved(durable ? '99' : '12'));
  const read = vi.spyOn(client, 'getRun').mockResolvedValue({ status: 'ready', value: savedData.story });
  vi.spyOn(client, 'getRunRecap').mockResolvedValue({ status: 'ready', value: savedData.extras });
  const portal = vi.spyOn(client, 'getDungeonPortalType').mockResolvedValue(4400);
  const visuals = vi.spyOn(client, 'getObjectVisuals');
  const page = render(Shell, { client, features: { ...productionFeatures, homeRedesign: true },
    preview: true, historyAdapter: fakeHistory('#/').adapter });
  await settle();
  const nav = within(page.getByRole('navigation', { name: 'Navigation' }));
  if (away) { await fireEvent.click(nav.getByRole('link', { name: 'Runs' })); await settle(); }
  portal.mockClear(); visuals.mockClear();
  transport.publishSnapshot({ ...snapshot, sequence: '13', at: '65000000', run: savedData.story.run });
  await settle();
  transport.publishSnapshot({ ...snapshot, sequence: '14', at: '66000000', area_kind: 'nexus', map: 'Nexus',
    run: null, run_totals: null, fights: [], drops: [] });
  await settle();
  if (away) { await fireEvent.click(nav.getByRole('link', { name: 'Home' })); await settle(); }
  await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  return { page, portal, visuals, read, snapshot, transport };
}

it('home_renders_recap_screen_when_saved', async () => {
  const { page, transport, snapshot, read } = await home();
  expect(page.container.querySelector('.recap-screen')).not.toBeNull();
  expect(page.container.querySelector('.recap-placeholder')).toBeNull();
  const before = page.container.querySelector('.recap-screen')!.textContent;
  transport.publishCapture({ ...snapshot.capture, state: 'Recovering' });
  await vi.advanceTimersByTimeAsync(2000);
  expect(page.container.querySelector('.recap-screen')!.textContent).toBe(before);
  expect(read).toHaveBeenCalledOnce();
});

it('run_copy_uses_saved_character_after_live_character_switch', async () => {
  const model = vi.spyOn(recapImages, 'recapImageModel');
  vi.spyOn(recapImages, 'renderRecapImage').mockResolvedValue(new Blob(['synthetic'], { type: 'image/png' }));
  vi.spyOn(clipboard, 'copyPng').mockResolvedValue('copied');
  const { page, snapshot, transport } = await home();
  expect(model).not.toHaveBeenCalled();
  transport.publishSnapshot({ ...snapshot, sequence: '15', at: '67000000', area_kind: 'nexus', map: 'Nexus',
    run: null, fights: [], character: { ...snapshot.character!, id: 'another-synthetic-player', name: 'Synthetic Replacement' } });
  await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
  expect(model).toHaveBeenCalledOnce();
  expect(model.mock.calls[0][2]).toMatchObject({ character: data().story.character!.name, date: recapImages.recapDate('65000000') });
  expect(model.mock.calls[0][2].character).not.toBe('Synthetic Replacement');
});

it('home_keeps_placeholder_while_saving', async () => {
  const { page } = await home(false);
  expect(page.container.querySelector('.recap-screen')).toBeNull(); expect(page.getByText('Saving run…')).toBeTruthy();
  await vi.advanceTimersByTimeAsync(5000);
  expect(page.container.querySelector('.recap-placeholder')).not.toBeNull(); expect(page.getByText('Partial')).toBeTruthy();
});

it('home_requests_recap_portal_in_nexus', async () => {
  const { page, portal } = await home(true, true);
  expect(page.container.querySelector('.recap-screen')).not.toBeNull();
  expect(portal).toHaveBeenLastCalledWith('Synthetic Sanctum');
});

it('home_requests_saved_fight_and_drop_visual_types', async () => {
  const { visuals } = await home();
  const types = visuals.mock.calls.flatMap(([types]) => types);
  expect(types).toEqual(expect.arrayContaining([4300, 4506, 4507]));
});

it('saved_view_is_built_once_and_survives_numeric_packets', async () => {
  const build = vi.spyOn(recapViews, 'runRecapView');
  const visualTypes = vi.spyOn(recapViews, 'recapVisualTypes');
  const { page, transport, snapshot } = await home();
  expect(build).toHaveBeenCalledOnce();
  expect(visualTypes).toHaveBeenCalledOnce();
  const before = page.container.querySelector('.recap-screen')!.textContent;
  transport.publishSnapshot({ ...snapshot, sequence: '15', at: '67000000', area_kind: 'nexus', map: 'Nexus',
    run: null, run_totals: null, fights: [], drops: [] });
  await settle();
  expect(build).toHaveBeenCalledOnce(); expect(page.container.querySelector('.recap-screen')!.textContent).toBe(before);
  expect(visualTypes).toHaveBeenCalledOnce();
});

it('home_replaces_saved_visit_placeholder_with_visit_recap', async () => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', map: 'Nexus', run: null });
  const client = createClient(createFixtureTransport(snapshot));
  const read = vi.spyOn(client, 'getVisitRecap').mockResolvedValue({ status: 'ready', value: {
    visit_id: `${snapshot.stream_id}:visit:4`, session_id: 'synthetic-session', character_id: 'synthetic-player', title: 'Synthetic Realm',
    periods: [{ visit_id: `${snapshot.stream_id}:visit:4`, started: '1000000', ended: '65000000' }], fights: [], boss_fights: [],
    boss_fights_limited: false, announcements: [], announcements_limited: false, drops: [], drops_limited: false,
    fame_delta: null, best_dps: [], chain: null,
  } });
  const recapPresentation = createRecapPresentation(); recapPresentation.remember('synthetic-visit');
  const page = render(Home, { props: { client, snapshot, store: saved('99'), redesign: true }, context: new Map([
    [SHELL_CONNECTION, { recapPresentation, homeRecap: readable({ watermark: '13',
      source: { kind: 'visit', streamId: snapshot.stream_id, visitKey: 'synthetic-visit', enteredSequence: '4' },
      summary: { kind: 'visit', title: 'Synthetic Realm', outcome: null, startedAt: '1000000', endedAt: '65000000',
        localDps: null, rank: null, players: null, fights: [], best: null, runId: null } }) }],
  ]) });
  await settle();
  await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  expect(read).toHaveBeenCalledOnce(); expect(page.container.querySelector('.recap-placeholder')).toBeNull();
  expect(page.queryByText('Saving visit…')).toBeNull(); expect(page.queryByText('Partial')).toBeNull();
  expect(page.container.querySelector('.visit-recap-screen')).not.toBeNull();
  expect(page.getByRole('heading', { name: 'Synthetic Realm · Realm visit' })).toBeTruthy();
});

it.each(['left', 'unknown'] as const)('outcome_tone_and_unknown_metrics: %s', outcome => {
  const savedData = data(); savedData.story.run.outcome = outcome; savedData.story.totals = null;
  savedData.story.boss_fights = []; savedData.story.boss_fights_limited = true;
  const page = render(RecapScreen, props(runRecapView(savedData)));
  expect(page.container.querySelector(`[data-outcome="${outcome}"]`)).not.toBeNull();
  expect([...page.container.querySelectorAll('.recap-tile-value')].map(value => value.textContent)).toEqual(Array(5).fill('—'));
  expect(page.container.querySelector('.recap-clear-time')).toBeNull();
  expect(page.getByRole('row', { name: /Synthetic Guardian/ }).textContent).toContain('—');
});

it('last_recap_loads_the_remembered_saved_run_after_route_and_area_changes', async () => {
  const { page, transport, snapshot, read } = await home();
  await fireEvent.click(within(page.getByRole('region', { name: 'Recap' })).getByRole('button', { name: 'Back' })); await settle();
  const nav = within(page.getByRole('navigation', { name: 'Navigation' }));
  await fireEvent.click(nav.getByRole('link', { name: 'Runs' })); await settle();
  read.mockClear();
  const next = { ...snapshot.run!, id: 'synthetic-next', dungeon: 'Synthetic next dungeon', started: '70000000' };
  transport.publishSnapshot({ ...snapshot, sequence: '15', at: next.started, run: next, map: next.dungeon }); await settle();
  await fireEvent.click(nav.getByRole('link', { name: 'Home' })); await settle();
  expect(page.container.querySelector('.dungeon-screen')).toBeTruthy();
  expect(page.queryByRole('region', { name: 'Recap ready' })).toBeNull();
  await fireEvent.click(page.getByRole('button', { name: 'Last recap: Synthetic Sanctum' })); await settle();
  expect(page.getByRole('heading', { name: 'Synthetic Sanctum · Cleared' })).toBeTruthy();
  expect(read).toHaveBeenCalledExactlyOnceWith(snapshot.run!.id);
  await fireEvent.click(within(page.getByRole('region', { name: 'Recap' })).getByRole('button', { name: 'Back' })); await settle();
  expect(within(page.getByRole('main')).getByRole('heading', { name: next.dungeon })).toBeTruthy();
});

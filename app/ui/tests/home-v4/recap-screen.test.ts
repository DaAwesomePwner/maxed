import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Outcome } from '../../src/lib/bindings/Outcome';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import type { RecapState } from '../../src/lib/home/v4/recap';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import RecapPlaceholder from '../../src/lib/home/v4/RecapPlaceholder.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { productionFeatures, routeHref } from '../../src/lib/navigation/routes';
import Shell from '../../src/lib/shell/Shell.svelte';
import { homeSnapshot, saved } from '../home/fixtures';
import { fakeHistory } from '../navigation/fake-history';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
const recap = (outcome: Outcome | null = 'cleared'): RecapState => ({
  watermark: '13',
  source: { kind: 'run', streamId: 'synthetic-stream', runId: 'synthetic:run/1' },
  summary: { kind: 'run', title: 'Synthetic Sanctum', outcome, startedAt: '1000000', endedAt: '65000000',
    localDps: 4321, rank: 2, players: 12, fights: [], best: null, runId: 'synthetic:run/1' },
});
const props = () => ({ recap: recap(), visuals: emptyHomeVisuals(), resolveSpriteUrl: () => '', onDismiss: vi.fn(),
  status: 'saved' as const, onRetry: vi.fn() });

it('saving_status_shows_saving_run', () => {
  const page = render(RecapPlaceholder, { ...props(), status: 'saving' });
  expect(page.getByText('Saving run…')).toBeTruthy(); expect(page.queryByText('Partial')).toBeNull();
});

it('placeholder_keeps_the_status_node_and_shows_the_partial_message', async () => {
  const page = render(RecapPlaceholder, { ...props(), status: 'saving', message: 'Synthetic saved history failure.' });
  const status = page.getByRole('status');
  expect(page.queryByText('Synthetic saved history failure.')).toBeNull();
  await page.rerender({ status: 'partial' });
  expect(page.getByRole('status')).toBe(status);
  expect(status.textContent).toBe('Partial');
  expect(page.getByText('Synthetic saved history failure.')).toBeTruthy();
  await page.rerender({ status: 'saved' });
  expect(page.getByRole('status')).toBe(status);
  expect(status.textContent).toBe('');
  expect(page.queryByText('Synthetic saved history failure.')).toBeNull();
});

it('partial_status_offers_retry', async () => {
  const input = { ...props(), status: 'partial' as const }, page = render(RecapPlaceholder, input);
  expect(page.getByText('Partial')).toBeTruthy();
  await fireEvent.click(page.getByRole('button', { name: 'Retry' })); expect(input.onRetry).toHaveBeenCalledOnce();
  await page.rerender({ status: 'saved' });
  expect(page.queryByText('Partial')).toBeNull(); expect(page.queryByRole('button', { name: 'Retry' })).toBeNull();
});

it('unavailable_partial_has_no_retry', () => {
  const page = render(RecapPlaceholder, { ...props(), status: 'partial', reason: 'unavailable' });
  expect(page.getByText('Partial')).toBeTruthy();
  expect(page.queryByRole('button', { name: 'Retry' })).toBeNull();
  expect(page.getByRole('button', { name: 'Back' })).toBeTruthy();
});

it.each(['timeout', 'failed'] as const)('recoverable_partial_keeps_retry: %s', async reason => {
  const input = { ...props(), status: 'partial' as const, reason }, page = render(RecapPlaceholder, input);
  expect(page.getByText('Partial')).toBeTruthy();
  await fireEvent.click(page.getByRole('button', { name: 'Retry' })); expect(input.onRetry).toHaveBeenCalledOnce();
});

it.each<[Outcome | null, string]>([['cleared', 'Cleared'], ['died', 'Died'], ['left', 'Left early'],
  ['unknown', 'Outcome not established'], [null, 'Outcome not established']])('placeholder_outcome_labels: %s', (outcome, label) => {
  const page = render(RecapPlaceholder, { ...props(), recap: recap(outcome) });
  expect(page.getByText(label)).toBeTruthy();
  expect(page.getByRole('heading', { name: 'Synthetic Sanctum' })).toBeTruthy();
  expect(page.getByText('Available from Last recap until the next recap')).toBeTruthy();
});

it('placeholder_uses_capture_duration_and_unknowns', async () => {
  const page = render(RecapPlaceholder, props());
  expect(page.getByText('1:04')).toBeTruthy();
  expect(page.getByText('4,321')).toBeTruthy();
  expect(page.getByText('#2')).toBeTruthy();
  await page.rerender({ recap: { ...recap(), summary: { ...recap().summary,
    startedAt: '', endedAt: null, localDps: null, rank: null, players: null } } });
  expect(page.getAllByText('—').length).toBeGreaterThanOrEqual(6);
  expect(page.container.textContent).not.toMatch(/NaN|undefined|0:00/);
});

it('back_closes_recap', async () => {
  const input = props(), page = render(RecapPlaceholder, input);
  await fireEvent.click(page.getByRole('button', { name: 'Back' }));
  expect(input.onDismiss).toHaveBeenCalledOnce();
});

it('caps_observed_fights_at_twelve_and_shows_the_remaining_count', () => {
  const fights = Array.from({ length: 15 }, (_, i) => ({ id: `synthetic-fight-${i}`, target_type: 300 + i, kill: null }));
  const page = render(RecapPlaceholder, { ...props(), recap: { ...recap(), summary: { ...recap().summary, fights } } });
  const list = page.getByRole('list');
  expect(within(list).getAllByRole('listitem')).toHaveLength(12);
  expect(within(list).getAllByRole('link', { name: 'Unknown target' })).toHaveLength(12);
  expect(page.getByText('+3 more')).toBeTruthy();
});

it.each([false, true])('best_bag_labels_contents_evidence: %s', contents_known => {
  const drop = { ...homeSnapshot().drops[0], contents_known, contents: [], items: [] };
  const page = render(RecapPlaceholder, { ...props(), recap: { ...recap(),
    summary: { ...recap().summary, best: { drop } } } });
  expect(page.getByText(contents_known ? 'Observed contents' : 'Contents not observed')).toBeTruthy();
  expect(page.queryByText(contents_known ? 'Contents not observed' : 'Observed contents')).toBeNull();
});

it('distinguishes_an_empty_known_fight_list_from_a_partial_window', async () => {
  const page = render(RecapPlaceholder, props());
  expect(page.getByText('No fights observed')).toBeTruthy();
  expect(page.queryByRole('list')).toBeNull();
  await page.rerender({ recap: { ...recap(), summary: { ...recap().summary, fights: null } } });
  expect(page.queryByText('No fights observed')).toBeNull();
  expect(page.getByRole('heading', { name: 'Observed fights' }).parentElement?.querySelector('p')?.textContent).toBe('—');
});

it('run_story_link_only_for_runs', async () => {
  const page = render(RecapPlaceholder, props());
  expect(page.getByRole('link', { name: 'Open run story ›' }).getAttribute('href'))
    .toBe(routeHref({ kind: 'run', id: recap().summary.runId! }));
  await page.rerender({ recap: { watermark: '13', source: { kind: 'visit', streamId: 'synthetic-stream', visitKey: 'synthetic-visit', enteredSequence: null },
    summary: { ...recap().summary, kind: 'visit', runId: null } } });
  expect(page.queryByRole('link', { name: 'Open run story ›' })).toBeNull();
});

it('best_bag_names_and_sprites_resolve_from_the_catalog', async () => {
  const drop = { ...homeSnapshot().drops[0], source_estimated: true };
  const itemType = drop.contents[0]?.item_type ?? drop.items[0];
  const sprite: SpriteRef = { key: 'synthetic', sheet: 'synthetic.png', x: 0, y: 0, width: 8, height: 8 };
  const input = { ...props(), recap: { ...recap(), summary: { ...recap().summary, best: { drop } } },
    resolveSpriteUrl: () => '/synthetic-sprite.png' };
  const page = render(RecapPlaceholder, input);
  expect(page.container.textContent).not.toMatch(/Item \d|Bag \d/);
  await page.rerender({ visuals: { ...emptyHomeVisuals(), objects: [
    { object_type: drop.bag_type, name: 'Synthetic white bag', sprite, bag: null },
    { object_type: itemType, name: 'Synthetic sword', sprite, bag: null },
  ] } });
  expect(page.getByText('Synthetic sword')).toBeTruthy();
  expect(page.getByText('Source estimated')).toBeTruthy();
  expect(page.getAllByRole('img').map(image => image.querySelector('img')?.getAttribute('src')))
    .toEqual(Array(2).fill(new URL('/synthetic-sprite.png', window.location.href).href));
});

async function shell(redesign = true) {
  const snapshot = homeSnapshot();
  const transport = createFixtureTransport(snapshot);
  const client = createClient(transport);
  const page = render(Shell, { client, features: { ...productionFeatures, homeRedesign: redesign },
    preview: true, historyAdapter: fakeHistory('#/').adapter });
  await settle();
  const nav = within(page.getByRole('navigation', { name: 'Navigation' }));
  const end = async () => {
    transport.publishSnapshot({ ...snapshot, sequence: '13', at: '8000000',
      run: { ...snapshot.run!, ended: '8000000', outcome: 'cleared' } });
    await settle();
  };
  return { page, nav, end, transport, snapshot, client };
}

it('home_passes_store_progress_and_retry_to_the_recap_controller', async () => {
  const { page, client, snapshot, end, transport } = await shell();
  const store = vi.spyOn(client, 'queryStore').mockResolvedValue(saved('12'));
  const getRun = vi.spyOn(client, 'getRun').mockResolvedValue({ status: 'ready', value: null });
  transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '13', at: '8000000',
    event: { run_ended: { ...snapshot.run!, ended: '8000000', outcome: 'cleared' } } });
  await end();
  await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  expect(within(page.getByRole('region', { name: 'Recap' })).getByText('Saving run…')).toBeTruthy();
  await vi.advanceTimersByTimeAsync(5000);
  expect(within(page.getByRole('region', { name: 'Recap' })).getByText('Partial')).toBeTruthy();
  expect(getRun).not.toHaveBeenCalled();
  store.mockResolvedValue(saved('13'));
  await vi.advanceTimersByTimeAsync(1000);
  expect(getRun).toHaveBeenCalledOnce();
  getRun.mockResolvedValue({ status: 'ready', value: { run: { ...snapshot.run!, ended: '8000000' },
    character: snapshot.character, fights: [], drops: [], timeline: [], comparison: null } });
  await fireEvent.click(page.getByRole('button', { name: 'Retry' })); await settle();
  const region = within(page.getByRole('region', { name: 'Recap' }));
  expect(region.queryByText('Partial')).toBeNull(); expect(region.queryByText('Saving run…')).toBeNull();
  expect(getRun).toHaveBeenCalledTimes(2);
});

it('flag_off_does_not_start_recap_reads', async () => {
  const { client, end } = await shell(false);
  vi.spyOn(client, 'queryStore').mockResolvedValue(saved('99'));
  const read = vi.spyOn(client, 'getRunRecap');
  const getRun = vi.spyOn(client, 'getRun');
  const getVisitRecap = vi.spyOn(client, 'getVisitRecap');
  await end(); await vi.advanceTimersByTimeAsync(6000);
  expect(read).not.toHaveBeenCalled();
  expect(getRun).not.toHaveBeenCalled();
  expect(getVisitRecap).not.toHaveBeenCalled();
});

it('recap_waits_while_home_is_not_mounted', async () => {
  const { page, nav, end, transport, snapshot } = await shell();
  await fireEvent.click(nav.getByRole('link', { name: 'Runs' })); await settle();
  expect(page.queryByLabelText('Session HUD')).toBeNull();
  await end();
  transport.publishSnapshot({ ...snapshot, sequence: '14', at: '9000000', area_kind: 'nexus', map: 'Nexus',
    run: null, run_totals: null, fights: [], drops: [] });
  await settle();
  await fireEvent.click(nav.getByRole('link', { name: 'Home' })); await settle();
  await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  const region = page.getByRole('region', { name: 'Recap' });
  expect(within(region).getByText('Cleared')).toBeTruthy();
  expect(within(region).getByRole('heading', { name: 'Synthetic Sanctum' })).toBeTruthy();
  expect(page.getByLabelText('Session HUD')).toBeTruthy();
  await fireEvent.click(within(page.getByRole('region', { name: 'Recap' })).getByRole('button', { name: 'Back' })); await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  await fireEvent.click(nav.getByRole('link', { name: 'Runs' })); await settle();
  await fireEvent.click(nav.getByRole('link', { name: 'Home' })); await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
});

it('flag_off_preserves_v3_after_run_end', async () => {
  const { page, end } = await shell(false);
  await end();
  expect(page.container.querySelector('[data-home="v3"] .nexus-home')).not.toBeNull();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  expect(page.queryByRole('button', { name: /^Last recap:/ })).toBeNull();
  expect(page.queryByLabelText('Session HUD')).toBeNull();
});

it('shell_recap_survives_capture_loss_and_yields_to_the_next_live_area', async () => {
  const { page, end, snapshot, transport } = await shell();
  await end();
  await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  transport.publishCapture({ ...snapshot.capture, state: 'Recovering' });
  await vi.advanceTimersByTimeAsync(2000);
  expect(page.getByRole('region', { name: 'Recap' })).toBeTruthy();
  expect(page.queryByRole('heading', { name: 'Reconnecting' })).toBeNull();
  transport.publishCapture(snapshot.capture);
  transport.publishSnapshot({ ...snapshot, sequence: '14', run: { ...snapshot.run!, id: 'synthetic-next', dungeon: 'Synthetic next dungeon' } });
  await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  expect(page.getByRole('heading', { name: 'Synthetic next dungeon' })).toBeTruthy();
  expect(page.getByRole('button', { name: /^Last recap:/ })).toBeTruthy();
});

it('placeholder_best_bag_names_are_read_once', () => {
  const drop = homeSnapshot().drops[0], input = props();
  const page = render(RecapPlaceholder, { ...input, recap: { ...input.recap,
    summary: { ...input.recap.summary, best: { drop } } } });
  const labelled = [...page.container.querySelectorAll('.bag [role="img"]')].map(node => node.getAttribute('aria-label'));
  // The visible span names the item when there is one, else the bag; that sprite stays unlabelled.
  expect(labelled.filter(label => label !== '')).toHaveLength(labelled.length - 1);
  expect(labelled.at(-1)).toBe('');
});

it('defers the recap item with no visuals and uses the same pick as Home after loading', async () => {
  const { bestItem } = await import('../../src/lib/home/v3/loot-rank');
  const drop = { ...homeSnapshot().drops[0], contents_known: true, items: [101, 102], contents: [] };
  const input = { ...props(), recap: { ...recap(), summary: { ...recap().summary, best: { drop } } } };
  const page = render(RecapPlaceholder, input);
  expect(bestItem(drop, input.visuals)).toBeNull();
  expect(page.container.querySelectorAll('.recap-loot .bag img')).toHaveLength(0);
  const visuals = { ...emptyHomeVisuals(), phase: 'ready' as const, objects: [
    { object_type: 101, item_tier: null, name: 'Synthetic potion', sprite: null, bag: null },
    { object_type: 102, item_tier: 'UT', name: 'Synthetic winner', sprite: null, bag: null },
  ] };
  await page.rerender({ visuals });
  expect(bestItem(drop, visuals)?.itemType).toBe(102);
  expect(page.container.querySelector('.recap-loot')?.textContent).toContain('Synthetic winner');
  expect(page.container.querySelector('.recap-loot')?.textContent).not.toContain('Synthetic potion');
});

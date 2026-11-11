import { cleanup, render, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import type { StatPanel } from '../../src/lib/bindings/StatPanel';
import type { DeathDetail } from '../../src/lib/bindings/DeathDetail';
import type { HpSample } from '../../src/lib/bindings/HpSample';
import type { ObjectVisual } from '../../src/lib/bindings/ObjectVisual';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import type { RunRecapData } from '../../src/lib/home/v4/recap-reads';
import { runRecapView } from '../../src/lib/home/v4/recap-view';
import RecapScreen from '../../src/lib/home/v4/RecapScreen.svelte';
import HpTrace from '../../src/lib/home/v4/HpTrace.svelte';
import { homeSnapshot } from '../home/fixtures';

afterEach(cleanup);
const sample = (at: string, hp = 400, max_hp: number | null = 800): HpSample => ({ at, hp, max_hp });
const death = (overrides: Partial<DeathDetail> = {}): DeathDetail => ({
  killed_by: 'Synthetic raw guardian', killed_by_type: 4300, fame: 1234,
  hp_trace: [sample('59000000', 800), sample('60000000'), sample('61000000', 0)], ...overrides,
});
const objects: ObjectVisual[] = [
  { object_type: 4300, name: 'Synthetic Catalog Guardian', bag: null,
    sprite: { key: 'synthetic-killer', sheet: 'synthetic.png', x: 0, y: 0, width: 8, height: 8 } },
  { object_type: 4400, name: 'Synthetic Class', sprite: null, bag: null },
];
function eligiblePanel(atCap: boolean): StatPanel {
  return { breakdown_provisional: true, conditions: [], lines:
    [0, 3, 20, 21, 22, 26, 27, 28].map(stat_id => ({ stat_id, base: atCap ? 75 : 74, cap: 75,
      exalt: 0, at_cap: atCap,
      split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } })) };
}
function data(): RunRecapData {
  const snapshot = homeSnapshot(), local = { ...snapshot.fights[0].players[0], is_local: true, dps: 200, rank: 1 };
  return { kind: 'run', recordsUnavailable: false, story: {
    run: { ...snapshot.run!, dungeon: 'Synthetic Sanctum', started: '1000000', ended: '65000000',
      died_at: '61000000', outcome: 'died', death: death() },
    character: { ...snapshot.character!, name: 'Synthetic Hero', class_id: 4400, maxed: 8, fame: 9999, stat_panel: eligiblePanel(true) },
    fights: [], boss_fights: [], drops: [], timeline: [], comparison: null,
    totals: { fights: 1, kills: 0, total_damage: 2000, unattributed_damage: 0, local, players: [local], fame_delta: 5, partial: false },
  }, extras: null, profile: null, baselines: null, streak: null };
}
function props(input = data()) {
  return { view: runRecapView(input, 'UTC'), visuals: { ...emptyHomeVisuals(), objects }, portalType: null,
    resolveSpriteUrl: vi.fn(() => ''), onDismiss: vi.fn() };
}
const banner = (container: HTMLElement) => within(container.querySelector('.death-banner') as HTMLElement);

it('death_banner_heading_and_killer', () => {
  const input = props(), page = render(RecapScreen, input), region = banner(page.container);
  expect(region.getByRole('heading', { name: 'Synthetic Sanctum · Died at 1:00' })).toBeTruthy();
  expect(region.getByText('Killed by Synthetic Catalog Guardian')).toBeTruthy();
  expect(region.queryByText(/Synthetic raw guardian/)).toBeNull();
  expect(region.queryByRole('img', { name: 'Synthetic Catalog Guardian' })).toBeNull();
  expect(input.resolveSpriteUrl).toHaveBeenCalledWith(objects[0].sprite);
  expect(region.getByText('Synthetic Hero · Synthetic Class · 8/8 · 1234 fame')).toBeTruthy();
  expect(page.container.querySelector('.death-banner')?.getAttribute('data-outcome')).toBe('died');
});

it('legacy_saved_character_maxed_is_unknown', () => {
  const input = data();
  for (const line of input.story.character!.stat_panel!.lines) delete line.split_evidence;
  const page = render(RecapScreen, props(input));
  expect(banner(page.container).getByText('Synthetic Hero · Synthetic Class · — · 1234 fame')).toBeTruthy();
});

it('killed_by_dash_when_empty', async () => {
  const input = data(); input.story.run.death = death({ killed_by: null, killed_by_type: null });
  const page = render(RecapScreen, props(input));
  expect(banner(page.container).getByText('Killed by —')).toBeTruthy();
  input.story.run.death = null;
  await page.rerender(props(input));
  expect(banner(page.container).getByText('Killed by —')).toBeTruthy();
});

it('raw_killer_is_marked_unverified', () => {
  const input = data(); input.story.run.death = death({ killed_by_type: 99999 });
  const page = render(RecapScreen, props(input));
  const raw = banner(page.container).getByTitle('Name from the death message; not matched to the catalog');
  expect(raw.textContent).toContain('Synthetic raw guardian');
  expect(raw.textContent?.replace(/\s+/g, ' ').trim()).toBe('Killed by Synthetic raw guardian unverified');
  expect(within(raw).getByText('unverified').closest('[aria-hidden="true"]')).toBeNull();
  expect(banner(page.container).queryByRole('img', { name: 'Synthetic raw guardian' })).toBeNull();
});

it('meta_line_unknowns_are_dashes', async () => {
  const input = data(); input.story.character = null; input.story.run.death = death({ fame: null });
  const page = render(RecapScreen, props(input));
  expect(banner(page.container).getByText('— · — · — · — fame')).toBeTruthy();
  input.story.character = { ...homeSnapshot().character!, name: ' ', class_id: 99999, maxed: null };
  await page.rerender(props(input));
  expect(banner(page.container).getByText('— · — · — · — fame')).toBeTruthy();
  input.story.character.maxed = 0; input.story.run.death!.fame = 0;
  input.story.character.stat_panel = eligiblePanel(false);
  await page.rerender(props(input));
  expect(banner(page.container).getByText('— · — · 0/8 · 0 fame')).toBeTruthy();
});

it('hp_trace_breaks_on_gaps', async () => {
  const { hpTraceView } = await import('../../src/lib/home/v4/death');
  const samples = [sample('56000000', 800), sample('57000000'), sample('58000001'), sample('59000001', 200), sample('61000000', 0)];
  const trace = hpTraceView(samples, 100, 40);
  expect(trace.gaps).toBe(2); expect(trace.paths).toHaveLength(3);
  const input = data(); input.story.run.death!.hp_trace = samples;
  const page = render(RecapScreen, props(input));
  expect(page.container.querySelectorAll('.hp-trace svg path')).toHaveLength(3);
});

it('hp_trace_scales_known_max_hp_and_unknown_max_against_peak', async () => {
  const { hpTraceView } = await import('../../src/lib/home/v4/death');
  expect(hpTraceView([sample('0', 800), sample('1000000', 200, 400)], 100, 40).paths).toEqual(['M 0 0 L 100 20']);
  expect(hpTraceView([sample('0', 800, null), sample('1000000', 400, null)], 100, 40).paths).toEqual(['M 0 0 L 100 20']);
  expect(hpTraceView([], 100, 40)).toEqual({ paths: [], gaps: 0 });
});

it('hp_sample_after_floored_death_time_is_clamped_to_right_edge', async () => {
  const { hpTraceView } = await import('../../src/lib/home/v4/death');
  const samples = [sample('60000000', 800), sample('61000000', 400), sample('61000500', 0)];
  const before = structuredClone(samples);
  expect(hpTraceView(samples, 100, 40, '61000000')).toEqual({ paths: ['M 0 0 L 100 20 L 100 40'], gaps: 0 });
  expect(samples).toEqual(before);
  const input = data(); input.story.run.death!.hp_trace = samples;
  const page = render(RecapScreen, props(input));
  const svg = page.container.querySelector('.hp-trace svg')!;
  const width = Number(svg.getAttribute('viewBox')!.split(' ')[2]);
  const points = svg.querySelector('path')!.getAttribute('d')!.match(/[ML] [\d.]+ [\d.]+/g)!;
  expect(points.slice(-2).map(point => Number(point.split(' ')[1]))).toEqual([width, width]);
});

it('hp_trace_retains_microsecond_precision_and_single_readings', async () => {
  const { hpTraceView } = await import('../../src/lib/home/v4/death');
  const start = 9007199254740992n;
  expect(hpTraceView([sample(String(start), 800), sample(String(start + 1n), 400), sample(String(start + 2n), 0)], 100, 40)
    .paths).toEqual(['M 0 0 L 50 20 L 100 40']);
  const single = hpTraceView([sample('1', 0, null)], 100, 40);
  expect(single.paths).toHaveLength(1); expect(single.paths.join('')).not.toMatch(/NaN|Infinity/);
});

it('no_hp_samples_message', () => {
  const input = data(); input.story.run.death!.hp_trace = [];
  const page = render(RecapScreen, props(input)), region = banner(page.container);
  expect(region.getByText('No HP readings before death')).toBeTruthy();
  expect(region.getByText('Final 5 seconds')).toBeTruthy();
  expect(region.queryByRole('img', { name: /observed HP/i })).toBeNull();
  expect(page.container.querySelector('.hp-trace .sr-only')).toBeNull();
});

it('bags_before_death_only', async () => {
  const { bagsBeforeDeath } = await import('../../src/lib/home/v4/death');
  const input = data(), drop = homeSnapshot().drops[0];
  input.story.drops = ['60000000', '61000000', '61001000', 'invalid'].map((at, i) => ({ ...drop, at, id: `synthetic-drop-${i}` }));
  const before = structuredClone(input.story.drops);
  expect(bagsBeforeDeath(input.story.drops, '61000000').map(row => row.id)).toEqual(['synthetic-drop-0', 'synthetic-drop-1']);
  expect(input.story.drops).toEqual(before);
  const page = render(RecapScreen, props(input));
  expect([...page.container.querySelectorAll('.death-banner [data-drop-id]')].map(row => row.getAttribute('data-drop-id')))
    .toEqual(['synthetic-drop-1', 'synthetic-drop-0']);
});

it('bags_use_bigint_microseconds_and_exclude_invalid_timestamps', async () => {
  const { bagsBeforeDeath } = await import('../../src/lib/home/v4/death');
  const drop = homeSnapshot().drops[0], at = '9007199254740992';
  const drops = [at, String(BigInt(at) + 999n), String(BigInt(at) + 1000n), '', 'oops'].map((at, i) => ({ ...drop, at, id: `synthetic-drop-${i}` }));
  expect(bagsBeforeDeath(drops, at)).toEqual([drops[0], drops[1]]);
  expect(bagsBeforeDeath(drops, 'invalid')).toEqual([]);
});

it('death_banner_right_side_has_dps_and_rank', () => {
  const page = render(RecapScreen, props()), region = banner(page.container);
  expect(region.getByText('Your DPS').closest('dl')?.textContent).toContain('200');
  expect(region.getByText('Your rank').closest('dl')?.textContent).toContain('#1 of 1');
  expect(region.queryByText('vs party avg')).toBeNull();
  expect(page.getByRole('region', { name: 'Fight by fight' })).toBeTruthy();
  expect(page.getByRole('button', { name: 'Back' })).toBeTruthy();
});

it('cleared_then_died_keeps_cleared_banner', () => {
  const input = data(); input.story.run.outcome = 'cleared'; input.story.run.completed_at = '60000000';
  const page = render(RecapScreen, props(input));
  expect(page.getByRole('heading', { name: 'Synthetic Sanctum · Cleared' })).toBeTruthy();
  expect(page.container.querySelector('.death-banner')).toBeNull();
  expect(page.queryByText(/Killed by/)).toBeNull();
});

it('trace_is_labelled_observed_hp_not_damage', () => {
  const page = render(RecapScreen, props()), region = banner(page.container);
  expect(region.getByText('Observed HP')).toBeTruthy();
  expect(region.getByText('Final 5 seconds')).toBeTruthy();
  expect(page.container.querySelector('.hp-trace svg')).toBeTruthy();
  expect(page.container.innerHTML.toLowerCase()).not.toContain('damage taken');
});

it('killer_verification_requires_a_catalog_type_match', async () => {
  const { killerLabel } = await import('../../src/lib/home/v4/death');
  expect(killerLabel(death(), objects)).toEqual({ text: 'Synthetic Catalog Guardian', spriteType: 4300, verified: true });
  expect(killerLabel(death({ killed_by_type: null, killed_by: 'Synthetic Catalog Guardian' }), objects))
    .toEqual({ text: 'Synthetic Catalog Guardian', spriteType: null, verified: false });
  expect(killerLabel(death({ killed_by_type: 99999 }), objects))
    .toEqual({ text: 'Synthetic raw guardian', spriteType: null, verified: false });
  expect(killerLabel(death({ killed_by_type: null, killed_by: ' ' }), objects)).toEqual({ text: '—', spriteType: null, verified: false });
  expect(killerLabel(null, objects)).toEqual({ text: '—', spriteType: null, verified: false });
  expect(killerLabel(death(), [{ ...objects[0], name: null }])).toEqual({ text: 'Synthetic raw guardian', spriteType: 4300, verified: false });
  expect(killerLabel(death({ killed_by: null }), [{ ...objects[0], name: ' ' }]))
    .toEqual({ text: '—', spriteType: 4300, verified: false });
});

it('hp_trace_is_read_once_with_a_summary', () => {
  const page = render(HpTrace, { samples: [sample('0', 800), sample('1000001', 200, 400)], diedAt: '2000000' });
  const trace = page.container.querySelector('.hp-trace')!;
  expect(trace.querySelector('figcaption')?.getAttribute('aria-hidden')).toBe('true');
  const svg = trace.querySelector('svg')!;
  expect(svg.getAttribute('aria-hidden')).toBe('true');
  expect(svg.hasAttribute('role')).toBe(false); expect(svg.hasAttribute('aria-label')).toBe(false);
  const readable = trace.cloneNode(true) as HTMLElement;
  readable.querySelectorAll('[aria-hidden="true"]').forEach(node => node.remove());
  expect(readable.textContent?.trim()).toBe('Observed HP over the final 5 seconds: 100% to 50%, 1 gap in readings');
  expect(readable.textContent?.match(/Observed HP over the final 5 seconds/g)).toHaveLength(1);
});

it('hp_summary_uses_valid_endpoints_peak_rounding_and_plural_gaps', async () => {
  const page = render(HpTrace, { samples: [sample('invalid', 9999), sample('0', 300, null),
    sample('1000001', 200, null), sample('2000002', 100, null), sample('3000000', -1)], diedAt: '4000000' });
  expect(page.container.querySelector('.sr-only')?.textContent)
    .toBe('Observed HP over the final 5 seconds: 100% to 33% of observed peak, 2 gaps in readings');
  await page.rerender({ samples: [sample('0', 400), sample('1', 0)], diedAt: '2' });
  expect(page.container.querySelector('.sr-only')?.textContent).toBe('Observed HP over the final 5 seconds: 50% to 0%');
});

it('died_without_time_keeps_death_banner', () => {
  const input = data(); input.story.run.died_at = null;
  input.story.drops = ['1', '65000000', 'invalid'].map((at, i) => ({ ...homeSnapshot().drops[0], at, id: `synthetic-drop-${i}` }));
  const rendered = props(input);
  expect(rendered.view.died).toEqual({ at: null, elapsed: '—' });
  const page = render(RecapScreen, rendered);
  expect(banner(page.container).getByRole('heading', { name: 'Synthetic Sanctum · Died at —' })).toBeTruthy();
  expect([...page.container.querySelectorAll('.death-banner [data-drop-id]')].map(row => row.getAttribute('data-drop-id')).sort())
    .toEqual(['synthetic-drop-0', 'synthetic-drop-1', 'synthetic-drop-2']);
});

it.each([sample('500000', NaN), sample('500000', Infinity), sample('500000', -1), sample('invalid')])(
  'invalid_hp_sample_splits_and_counts_a_gap: %j', async invalid => {
    const { hpTraceView } = await import('../../src/lib/home/v4/death');
    const trace = hpTraceView([sample('0'), invalid, sample('1000000')], 100, 40);
    expect(trace.paths).toEqual(['M 0 20 L 0 20', 'M 100 20 L 100 20']);
    expect(trace.gaps).toBe(1);
  });

it('death_tiles_are_selected_by_key_after_reordering', () => {
  const input = props();
  input.view.tiles = [input.view.tiles[4], input.view.tiles[2], input.view.tiles[1], input.view.tiles[0], input.view.tiles[3]];
  const page = render(RecapScreen, input), region = banner(page.container);
  expect(region.getByText('Your DPS')).toBeTruthy(); expect(region.getByText('Your rank')).toBeTruthy();
  expect(region.queryByText('Fame')).toBeNull(); expect(region.queryByText('vs party avg')).toBeNull();
});

it('drops_in_the_floored_death_millisecond_are_kept', async () => {
  const { bagsBeforeDeath } = await import('../../src/lib/home/v4/death');
  const drops = ['61000500', '61001000'].map((at, i) => ({ ...homeSnapshot().drops[0], at, id: `synthetic-drop-${i}` }));
  expect(bagsBeforeDeath(drops, '61000000')).toEqual([drops[0]]);
});

it('shared_timestamp_parser_preserves_microseconds_and_rejects_invalid_values', async () => {
  const { timestamp } = await import('../../src/lib/home/v4/recap-view');
  expect(timestamp('9007199254740993')).toBe(9007199254740993n);
  for (const value of [null, undefined, '', 'invalid', '-1', '1.5']) expect(timestamp(value)).toBeNull();
});

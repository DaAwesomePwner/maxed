import { fireEvent, render, screen } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import PlayerHeader from '../../src/lib/players/PlayerHeader.svelte';
import ScopeTabs from '../../src/lib/players/ScopeTabs.svelte';
import PlayerTimelineChart from '../../src/lib/players/PlayerTimelineChart.svelte';
import BossFightsTable from '../../src/lib/players/BossFightsTable.svelte';
import GearCard from '../../src/lib/players/GearCard.svelte';
import StatsCard from '../../src/lib/players/StatsCard.svelte';
import ActivityCard from '../../src/lib/players/ActivityCard.svelte';
import type { PlayerProfileView } from '../../src/lib/bindings/PlayerProfileView';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { source } from '../runs/fixtures';
import { gearAt, swapsIn } from '../../src/lib/players/player-view';
import { itemHoverParams } from '../../src/lib/components/item-hover';

const visuals = { ...emptyHomeVisuals(), objects: [
  { object_type: 100, name: 'Invented Wizard', sprite: null, bag: null },
  { object_type: 1000, name: 'Invented Staff', sprite: null, bag: null, item_tier: 'T14' },
  { object_type: 101, name: 'Invented Skin', sprite: null, bag: null },
] };
const profile: PlayerProfileView = { scope_id: 'run-1', player_id: 'player-1', object_id: 7, is_local: true, maxed_gated: true,
  name: 'Invented Player', class_id: 100, level: 20, stars: 75, maxed: 8, maxed_stats: [], fame: 4812, skin_id: 101,
  first_seen: '1000000', last_seen: '5000000', stats: [{ stat_id: 20, value: 87 }, { stat_id: 48, value: 12 }],
  equipment: [{ slot: 0, item_type: 1000, enchant: source({ enchant: { state: 'recorded', slots: 3, applied: 3, ids: [1, 2, 3] } }).enchant }] };

it('renders identity, flags, metadata and link navigation', () => {
  render(PlayerHeader, { name: profile.name!, classId: 100, isLocal: true, live: true, partial: true, profile,
    inRunMs: 4000, visuals, picker: [{ href: '#one', label: 'Invented Player', current: true }], prevHref: null, nextHref: '#two', nextName: 'Invented Neighbor' });
  expect(screen.getByRole('heading', { level: 2 }).textContent).toContain('Invented Player');
  for (const text of ['You', 'Live', 'partial']) expect(screen.getByText(text)).toBeTruthy();
  expect(screen.getByText(/Invented Wizard · Lv 20/).textContent).toContain('8/8');
  expect(screen.getByRole('button', { name: 'Previous player' }).hasAttribute('disabled')).toBe(true);
  expect(screen.getByRole('link', { name: 'Next player: Invented Neighbor' }).getAttribute('href')).toBe('#two');
  expect(screen.getByRole('link', { name: 'Invented Player' }).getAttribute('aria-current')).toBe('page');
});

it('renders scope links with a disabled missing-identity tab', () => {
  render(ScopeTabs, { tabs: [{ label: 'Whole run', sub: '34m', href: '#run', current: true },
    { label: 'Invented Boss', sub: '1:12', href: null, current: false }] });
  expect(screen.getByRole('link', { name: 'Whole run 34m' }).getAttribute('aria-current')).toBe('page');
  expect(screen.getByRole('button', { name: 'Invented Boss 1:12' }).hasAttribute('disabled')).toBe(true);
});

it('shows gear names, tiers, enchant pips, missing slots and recorded swaps', () => {
  render(GearCard, { equipment: profile.equipment, observedAt: 5000, swaps: [
    { at: '1000000', equipment: profile.equipment }, { at: '3000000', equipment: [] },
  ], visuals, resolveSpriteUrl: () => null });
  const gear = screen.getByText('Invented Staff').closest('li')!;
  expect(gear.hasAttribute('title')).toBe(false);
  expect(itemHoverParams(gear)).toEqual({ type: 1000, enchant: profile.equipment[0].enchant,
    name: 'Invented Staff', tier: 'T14' });
  expect(screen.getByText(/Weapon · T14/)).toBeTruthy();
  expect(screen.getAllByText('Not seen')).toHaveLength(3);
  for (const slot of screen.getAllByText('Not seen')) expect(itemHoverParams(slot.closest('li')!)).toBeUndefined();
  expect(screen.getByRole('img', { name: 'Legendary, 3 enchants' })).toBeTruthy();
  expect(screen.getByText(/Started with/)).toBeTruthy();
  expect(screen.getByText(/Weapon → Not seen/)).toBeTruthy();
});

it('shows the exact legacy gear message', () => {
  render(GearCard, { equipment: null, observedAt: null, swaps: [], visuals, resolveSpriteUrl: () => null });
  expect(screen.getByText('Not recorded for this run')).toBeTruthy();
});

it('composes fight-start gear and diffs the first in-fight swap against its baseline', () => {
  const log = [{ at: '1000000', equipment: profile.equipment }, { at: '3000000', equipment: [] }];
  const equipment = gearAt(log, 2000)!;
  render(GearCard, { equipment, observedAt: 2000, swaps: swapsIn(log, 2000, 4000),
    baseline: { at: 2000, equipment }, visuals, resolveSpriteUrl: () => null });
  expect(screen.getByText('Invented Staff')).toBeTruthy();
  expect(screen.getByText(/at fight start/)).toBeTruthy();
  expect(screen.getByText('Weapon → Not seen')).toBeTruthy();
  expect(screen.queryByText(/Started with/)).toBeNull();
});

it('highlights only stats with engine cap evidence and tones negative boosts separately', () => {
  const { container } = render(StatsCard, { profile: { ...profile, maxed_stats: [20], stats: [
    ...profile.stats, { stat_id: 21, value: 40 }, { stat_id: 49, value: -5 },
  ] }, visuals });
  expect(container.querySelector('[data-stat-id="20"]')?.getAttribute('data-tone')).toBe('max');
  expect(container.querySelector('[data-stat-id="21"]')?.getAttribute('data-tone')).toBe('boosted');
  expect(screen.getByText('−5').classList.contains('player-danger')).toBe(true);
});

it.each([true, false])('hides approx. for an unavailable compacted chart (disabled: %s)', disabled => {
  render(PlayerTimelineChart, { timeline: { compacted: true, disabled, buckets: [], points: [] },
    bands: [], unavailable: disabled ? undefined : 'limit' });
  expect(screen.getByText('Timeline unavailable (resource limit)')).toBeTruthy();
  expect(screen.queryByText('approx.')).toBeNull();
});

it('shows observed stats and boosts, count and metadata without inventing per-stat caps', () => {
  const { container } = render(StatsCard, { profile, visuals });
  expect(screen.getByText('87')).toBeTruthy();
  expect(screen.getByText('+12')).toBeTruthy();
  expect(screen.getByText(/8\/8/)).toBeTruthy();
  expect(screen.getByText('4,812')).toBeTruthy();
  expect(screen.getByText('★ 75')).toBeTruthy();
  expect(screen.getByText('Invented Skin')).toBeTruthy();
  expect(screen.getByText('Stats not observed')).toBeTruthy();
  expect(container.querySelector('[data-stat-id="20"]')?.getAttribute('data-tone')).toBe('boosted');
  expect(container.querySelector('[data-tone="max"]')).toBeNull();
  expect(screen.queryByText('0')).toBeNull();
});

it('shows legacy stats and omits unobserved stats with one message', () => {
  const page = render(StatsCard, { profile: null, visuals });
  expect(screen.getByText('Not recorded for this run')).toBeTruthy();
  page.unmount();
  render(StatsCard, { profile: { ...profile, stats: [] }, visuals });
  expect(screen.getAllByText('Stats not observed')).toHaveLength(1);
  expect(screen.queryByText('ATT')).toBeNull();
});

it('renders all eight stats in order with correct boost pairs and marks an unknown boost', () => {
  const ids = [0, 3, 20, 21, 22, 28, 26, 27], boosts = [46, 47, 48, 49, 50, 53, 51, 52];
  const { container } = render(StatsCard, { profile: { ...profile, stats: [
    ...ids.map(stat_id => ({ stat_id, value: 50 })),
    ...boosts.slice(1).map((stat_id, i) => ({ stat_id, value: i + 1 })),
  ] }, visuals });
  expect([...container.querySelectorAll('.player-stats dt')].map(el => el.textContent)).toEqual(['HP', 'MP', 'ATT', 'DEF', 'SPD', 'DEX', 'VIT', 'WIS']);
  expect(container.querySelector('[data-stat-id="28"] small')?.textContent).toBe('+5');
  expect(container.querySelector('[data-stat-id="27"] small')?.textContent).toBe('+7');
  expect(screen.getByTitle('Boost not seen').textContent).toBe('—');
  expect(screen.queryByText('Stats not observed')).toBeNull();
});

it('shows exact chart legacy and resource-limit messages', () => {
  const page = render(PlayerTimelineChart, { timeline: null, bands: [], unavailable: 'legacy' });
  expect(screen.getByText('Not recorded for this run')).toBeTruthy();
  page.unmount();
  render(PlayerTimelineChart, { timeline: null, bands: [], unavailable: 'limit' });
  expect(screen.getByText('Timeline unavailable (resource limit)')).toBeTruthy();
});

it('shows an optional chart note near the heading and removes it when cleared', async () => {
  const note = 'Includes damage to other targets during this fight';
  const page = render(PlayerTimelineChart, { timeline: { compacted: false, disabled: false,
    buckets: [{ from: '1000000', until: '2000000', party_total: 120, party_active: 2 }], points: [40] }, bands: [] });
  expect(screen.queryByText(note)).toBeNull();
  await page.rerender({ note });
  const text = screen.getByText(note);
  expect(text.tagName).toBe('SMALL');
  expect(text.parentElement?.classList.contains('player-muted')).toBe(true);
  expect(text.parentElement?.previousElementSibling).toBe(screen.getByRole('heading', { name: /DPS over time/ }));
  await page.rerender({ note: null });
  expect(screen.queryByText(note)).toBeNull();
});

it('plots bucket DPS with shaded bosses, approximate label and a keyboard crosshair tooltip', async () => {
  const { container } = render(PlayerTimelineChart, { timeline: { compacted: true, disabled: false,
    buckets: [{ from: '1000000', until: '3000000', party_total: 120, party_active: 2 }], points: [40] },
    bands: [{ from: 1000, until: 3000, label: 'Invented Boss' }] });
  expect(screen.getByText('approx.')).toBeTruthy();
  expect(container.querySelector('path[stroke="var(--gold)"]')).toBeTruthy();
  expect(container.querySelector('path[stroke="var(--violet)"]')?.hasAttribute('stroke-dasharray')).toBe(true);
  expect(container.querySelector('rect')?.getAttribute('width')).toBe('800');
  await fireEvent.input(screen.getByRole('slider', { name: 'Inspect timeline' }), { target: { value: '0' } });
  const tooltip = screen.getByRole('tooltip');
  expect(tooltip.textContent).toContain('Invented Boss');
  expect(tooltip.textContent).toContain('Player 20 DPS');
  expect(tooltip.textContent).toContain('Party average 30 DPS');
});

it('renders the boss table with current scope, links and unknown values', () => {
  const { container } = render(BossFightsTable, { rows: [
    { fight_id: 'one', dps_basis: 'player_active_window_v1', target_name: 'Invented Boss', started: '1000000', killed: true, player_count: 3, damage: 300, dps: 100, rank: 2, share: 0.25, deaths: 1 },
    { fight_id: 'two', dps_basis: 'whole_target_v0', started: '3000000', killed: false, player_count: 3 },
  ], currentFightId: 'one', hrefFor: row => row.fight_id === 'one' ? '#fight' : null });
  expect(screen.getByRole('link', { name: 'Invented Boss' }).getAttribute('href')).toBe('#fight');
  expect(container.querySelector('tr[data-current="true"]')).toBeTruthy();
  expect(screen.getByRole('cell', { name: 'Active DPS: 100' }).getAttribute('title')).toContain('that player');
  expect(screen.getByRole('cell', { name: 'DPS: —' }).getAttribute('title')).toContain('whole fight');
  expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(6);
});

it('renders activity labels and death markers without inventing absent events', () => {
  const { container } = render(ActivityCard, { items: [
    { at: 1000, kind: 'entered', label: 'Entered run' }, { at: 3000, kind: 'death', label: 'Died' },
  ] });
  expect(screen.getByText('Entered run')).toBeTruthy();
  expect(container.querySelector('[data-kind="death"]')).toBeTruthy();
  expect(screen.queryByText('Last hit')).toBeNull();
});

const evidenceCases = [
  { is_local: true, count: '—/8', tone: 'boosted' },
  { is_local: true, maxed_gated: false, count: '—/8', tone: 'boosted' },
  { is_local: true, maxed_gated: true, count: '8/8', tone: 'max' },
  { is_local: false, count: '8/8', tone: 'max' },
];
it.each(evidenceCases)('gates header counts with provenance %j', evidence => {
  const { maxed_gated: _, ...legacy } = profile;
  render(PlayerHeader, { name: profile.name!, classId: 100, isLocal: evidence.is_local,
    live: false, partial: false, profile: { ...legacy, ...evidence }, inRunMs: 4000,
    visuals, picker: [], prevHref: null, nextHref: null });
  expect(screen.getByText(/Invented Wizard · Lv 20/).textContent).toContain(evidence.count);
});
it.each(evidenceCases)('gates cap highlights and preserves boost tones with provenance %j', evidence => {
  const { maxed_gated: _, ...legacy } = profile;
  const { container } = render(StatsCard, { profile: { ...legacy, ...evidence, maxed_stats: [20, 21, 22], stats: [
    ...profile.stats, { stat_id: 21, value: 40 }, { stat_id: 49, value: -5 },
    { stat_id: 22, value: 50 }, { stat_id: 50, value: 0 },
  ] }, visuals });
  expect(screen.getByRole('heading').textContent).toContain(evidence.count);
  expect(container.querySelector('[data-stat-id="20"]')?.getAttribute('data-tone')).toBe(evidence.tone);
  expect(container.querySelector('[data-stat-id="21"]')?.getAttribute('data-tone')).toBe(evidence.tone);
  expect(screen.getByText('−5').classList.contains('player-danger')).toBe(true);
  if (evidence.count === '—/8') expect(container.querySelector('[data-tone="max"]')).toBeNull();
});

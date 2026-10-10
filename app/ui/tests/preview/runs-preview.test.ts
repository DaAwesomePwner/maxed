import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, expect, it, vi } from 'vitest';
import { clearAndDeath } from '../../src/lib/runs/story';
import { defaultFilters } from '../../src/lib/navigation/state';
import RunsPreview from './RunsPreview.svelte';
import { createRunsPreview } from './runs-scenario';
import { dayGroups, vsBest } from '../../src/lib/runs/runs-list';

const now = new Date(2026, 9, 2, 20);
beforeEach(() => { vi.useFakeTimers({ toFake: ['Date'] }); vi.setSystemTime(now); });

it('mounts the real Runs list with synthetic dungeons and preserves the initial hash', async () => {
  window.history.replaceState(null, '', '/?preview=runs#/runs');
  render(RunsPreview);
  const main = screen.getByRole('main');
  expect(await within(main).findAllByRole('link', { name: /The Shatters/ })).toHaveLength(2);
  expect(main.textContent).toContain('Lost Halls');
  expect(main.textContent).toContain('Abyss of Demons');
  expect(location.hash).toBe('#/runs');
});

it('opens Runs when no initial hash is supplied', async () => {
  window.history.replaceState(null, '', '/?preview=runs');
  render(RunsPreview);
  await within(screen.getByRole('main')).findByRole('link', { name: /Lost Halls/ });
  expect(location.hash).toBe('#/runs');
});

it.each([
  ['#/runs/preview-run-0', 'Run'],
  ['#/runs/preview-run-0/fights/preview-run-0-fight-0', 'Fight'],
  ['#/runs/preview-run-0/fights/preview-run-0-fight-0/players/1', 'Player'],
] as const)('retains initial %s through preview startup with a focusable route heading', async (hash, title) => {
  vi.useFakeTimers();
  window.history.replaceState(null, '', `/?preview=runs${hash}`);
  render(RunsPreview);
  await vi.advanceTimersByTimeAsync(20_000);
  expect(location.hash).toBe(hash);
  const main = screen.getByRole('main');
  const heading = within(main).getByRole('heading', { name: title, level: 1 });
  expect(heading.classList.contains('sr-only')).toBe(true);
  expect(heading.getAttribute('tabindex')).toBe('-1');
  heading.focus();
  expect(document.activeElement).toBe(heading);
  expect(main.textContent).toContain('TestMage');
  expect(main.textContent).not.toMatch(/Not Found|Storage Error/);
});

it.each(['default', 'stress'] as const)('serves linked, internally consistent %s stories and honest missing rows', async variant => {
  const preview = createRunsPreview(variant, now);
  try {
    const page = await preview.client.listRuns({ ...defaultFilters(), limit: 100 });
    expect(page.status).toBe('ready');
    if (page.status !== 'ready') throw new Error('Expected synthetic history');
    expect(page.value.rows).toHaveLength(variant === 'stress' ? 60 : 15);
    expect(page.value.rows.some(run => run.outcome === 'died')).toBe(true);
    expect(page.value.rows.some(run => run.outcome === 'left')).toBe(true);
    expect(page.value.rows.some(run => run.ended === null && run.outcome === null)).toBe(true);
    expect(page.value.rows.some(run => !run.complete_capture && run.issues.length > 0)).toBe(true);
    for (const run of page.value.rows) {
      const result = await preview.client.getRun(run.id);
      if (result.status !== 'ready' || !result.value) throw new Error('Expected synthetic story');
      const story = result.value;
      expect(story.run).toEqual(run);
      expect(story.boss_fights?.map(row => row.id)).toEqual(story.fights.filter(row => row.boss_type !== null).map(row => row.id));
      expect(story.boss_fights_limited).toBe(false); expect(story.events_limited).toBe(false);
      expect(story.events?.filter(event => event.kind.type === 'player_left')).toHaveLength(story.fights.length ? 1 : 0);
      expect(story.events?.some(event => event.kind.type === 'player_death')).toBe(run.died_at !== null);
      const runIndex = page.value.rows.findIndex(row => row.id === run.id);
      expect(story.older_run_id).toBe(page.value.rows[runIndex + 1]?.id ?? null);
      expect(story.newer_run_id).toBe(page.value.rows[runIndex - 1]?.id ?? null);
      const index = Number(run.id.split('-').at(-1));
      const bosses = story.fights.filter(fight => fight.target_kind === 'boss');
      // Standard runs carry the full story; preview-run-9 onward are default-variant edge cases.
      const edge = variant === 'default' && index >= 9 ? index - 9 : null;
      expect(bosses).toHaveLength(edge !== null ? 0 : variant === 'stress' ? 64 : 4);
      expect(story.fights.filter(fight => fight.target_kind === 'ordinary'))
        .toHaveLength(edge === null ? 9 : [3, 0, 0, 4, 4, 4][edge]);
      expect(story.fights).toHaveLength(edge === null ? bosses.length + 9 : [3, 3, 0, 4, 4, 4][edge]);
      expect(story.drops).toHaveLength(edge === 2 ? 0 : variant === 'stress' ? (index + 5) % 6 : [5, 2, 1, 1, 4, 0, 3, 2, 1, 0, 3, 0, 2, 4, 5][index]);
      if (edge === 0) expect(story.fights.every(fight => fight.target_kind === 'ordinary' && fight.target_type !== null)).toBe(true);
      if (edge === 1) expect(story.fights.every(fight => fight.target_kind == null && fight.target_type === null &&
        fight.target_name === null && fight.boss_type === null)).toBe(true);
      if (edge === 2) {
        expect(story.totals).toBeNull();
        expect(story.timeline.map(entry => entry.kind)).toEqual(['entered', 'completion', 'exit']);
      }
      if (edge === 3) {
        expect(story.character).toBeNull();
        expect(run.character_id).toBeNull();
        expect(run.issues).toContain('missing_identity');
        expect(run.complete_capture).toBe(false);
        expect(story.totals?.local).toBeNull();
        expect(story.fights.every(fight => fight.local_damage === null && fight.local_rank === null)).toBe(true);
      }
      if (edge === 4) {
        expect(story.drops.some(drop => !drop.contents_known)).toBe(true);
        expect(story.drops.some(drop => drop.contents_known)).toBe(true);
        expect(story.drops.filter(drop => !drop.contents_known).every(drop => drop.items.length === 0 && drop.contents.length === 0)).toBe(true);
      }
      if (edge === 5) {
        expect(clearAndDeath(run)).toBe(true);
        expect(story.timeline.some(entry => entry.kind === 'death')).toBe(true);
      }
      if (variant === 'default' && index === 4) expect(story.timeline.find(entry => entry.kind === 'player_left')?.explanation).toBe('DemoAlly left');
      if (variant === 'stress' && index === 0) {
        const name = story.timeline.find(entry => entry.kind === 'player_left')?.explanation;
        expect(name).toMatch(/^DemoAllyWithAnExceptionallyLongSyntheticName4 left$/);
      }
      if (story.totals) expect(story.totals.total_damage).toBe(story.fights.reduce((sum, fight) => sum + fight.total_damage, 0));
      else expect(edge).toBe(2);
      expect(story.timeline.every(entry => BigInt(entry.at) >= BigInt(run.started) &&
        (run.ended === null || BigInt(entry.at) <= BigInt(run.ended)))).toBe(true);
      expect(story.timeline.some(entry => entry.kind === 'completion')).toBe(run.completed_at !== null);
      expect(story.timeline.some(entry => entry.kind === 'exit')).toBe(run.ended !== null);
      const strip = await preview.client.getRunFightStrip(run.id, null);
      if (strip.status !== 'ready') throw new Error('Expected synthetic strip');
      expect(strip.value.map(row => row.fight_id)).toEqual(bosses.map(row => row.id));
      expect(strip.value.every(row => row.damage !== null && row.rank !== null)).toBe(true);
      const missingPlayer = await preview.client.getRunFightStrip(run.id, 'missing-player');
      if (missingPlayer.status !== 'ready') throw new Error('Expected synthetic strip');
      expect(missingPlayer.value.every(row => row.damage === null && row.rank === null && row.share === null)).toBe(true);
      if (story.fights.length === 0) continue;
      const fight = await preview.client.getFight((bosses[0] ?? story.fights[0]).id);
      if (fight.status !== 'ready' || !fight.value) throw new Error('Expected synthetic fight');
      expect(fight.value.run_id).toBe(run.id);
      expect(fight.value.players).toHaveLength(variant === 'stress' ? 85 : 7);
      expect(fight.value.players[0].sources).toHaveLength(2);
      expect(fight.value.buckets.reduce((sum, bucket) => sum + bucket.total, 0)).toBeCloseTo(fight.value.total_damage);
      expect(await preview.client.getPlayerBreakdown(fight.value.id, 1)).toEqual({ status: 'ready', value: fight.value.players[0] });
      expect(await preview.client.getPlayerBreakdown(fight.value.id, -99)).toEqual({ status: 'ready', value: null });
    }
    expect(await preview.client.getRun('missing')).toEqual({ status: 'ready', value: null });
    expect(await preview.client.getFight('missing')).toEqual({ status: 'ready', value: null });
  } finally { preview.dispose(); }
});

it('filters and pages the synthetic run list', async () => {
  const preview = createRunsPreview('default', now);
  try {
    const first = await preview.client.listRuns({ ...defaultFilters(), limit: 2 });
    if (first.status !== 'ready') throw new Error('Expected synthetic page');
    expect(first.value.rows).toHaveLength(2);
    expect(first.value.next).not.toBeNull();
    const second = await preview.client.listRuns({ ...defaultFilters(), limit: 2, cursor: first.value.next });
    if (second.status !== 'ready') throw new Error('Expected synthetic page');
    expect(second.value.rows[0].id).not.toBe(first.value.rows[0].id);
    const died = await preview.client.listRuns({ ...defaultFilters(), outcome: 'died' });
    if (died.status !== 'ready') throw new Error('Expected synthetic page');
    expect(died.value.rows.map(run => run.outcome)).toEqual(['died']);
  } finally { preview.dispose(); }
});

it('returns independent JSON DTOs from the preview transport', async () => {
  const preview = createRunsPreview('default', now);
  try {
    const first = await preview.client.getFight('preview-run-0-fight-0');
    if (first.status !== 'ready' || !first.value) throw new Error('Expected synthetic fight');
    const expected = structuredClone(first.value);
    first.value.players[0].name = 'Changed';
    first.value.players[0].sources[0].enchant.ids.push(999);
    first.value.buckets[0].total = -1;
    expect(await preview.client.getFight(first.value.id)).toEqual({ status: 'ready', value: expected });
  } finally { preview.dispose(); }
});

it.each(['default', 'stress'] as const)('uses mockup enchant and tier shapes in the %s player preview', async variant => {
  const preview = createRunsPreview(variant);
  try {
    const result = await preview.client.getFight('preview-run-0-fight-2');
    if (result.status !== 'ready' || !result.value) throw new Error('Expected synthetic fight');
    const [local, ...others] = result.value.players;
    expect(local.sources.map(source => ({ kind: source.kind, item: source.item_type, enchant: source.enchant }))).toEqual([
      { kind: 'weapon', item: 200, enchant: { state: 'recorded', slots: 4, applied: 4, ids: [1, 2, 3, 4] } },
      { kind: 'ability', item: 201, enchant: { state: 'recorded_empty', slots: 0, applied: 0, ids: [] } },
    ]);
    const visuals = await preview.client.getObjectVisuals([200, 201]);
    expect(visuals.objects.find(row => row.object_type === 200)?.item_tier).toBe('UT');
    expect(visuals.objects.find(row => row.object_type === 201)?.item_tier).toBe('ST');
    const counts = new Set(others.flatMap(player => player.sources.map(source => source.enchant.applied)));
    expect(counts).toEqual(new Set([null, 0, 1, 2, 3, 4]));
    expect(await preview.client.getPlayerBreakdown(result.value.id, local.object_id)).toEqual({ status: 'ready', value: local });
  } finally { preview.dispose(); }
});

it.each(['default', 'stress'] as const)('supplies consistent row details and scoped summary in %s preview', async variant => {
  const preview = createRunsPreview(variant, now);
  try {
    const reply = await preview.client.listRuns({ ...defaultFilters(), limit: 100 });
    if (reply.status !== 'ready') throw new Error('Expected synthetic page');
    const { rows, details, summary } = reply.value;
    expect(details?.map(detail => detail.run_id)).toEqual(rows.map(run => run.id));
    expect(summary?.runs).toBe(rows.length);
    expect(summary?.cleared).toBe(rows.filter(run => run.outcome === 'cleared').length);
    expect(summary?.unknown).toBe(rows.filter(run => run.outcome === 'unknown').length);
    expect(summary?.characters).toHaveLength(3);
    for (const detail of details ?? []) {
      const story = await preview.client.getRun(detail.run_id);
      if (story.status !== 'ready' || !story.value) throw new Error('Expected story');
      expect(detail.character_name).toBe(story.value.character?.name ?? null);
      expect(detail.local_share).toBe(story.value.totals?.local?.share ?? null);
      expect(detail.fame_delta).toBe(story.value.totals?.fame_delta ?? null);
      expect(detail.observed_drop_count).toBe(story.value.drops.length);
      expect(detail.loot_preview.length).toBeLessThanOrEqual(3);
      expect(detail.loot_preview.every(bag => story.value!.drops.some(drop => drop.id === bag.drop_id))).toBe(true);
    }
    const character = summary!.characters[0];
    const scoped = await preview.client.listRuns({ ...defaultFilters(), character_id: character.id, outcome: 'cleared',
      range: { start: rows.at(-1)!.started, end: String(BigInt(rows[0].started) + 1n) } });
    if (scoped.status !== 'ready') throw new Error('Expected scoped page');
    expect(scoped.value.rows.every(run => run.character_id === character.id && run.outcome === 'cleared')).toBe(true);
    expect(scoped.value.summary?.runs).toBe(rows.filter(run => run.character_id === character.id).length);
    expect(scoped.value.summary?.total_runs_all_time).toBe(rows.length);
    const empty = await preview.client.listRuns({ ...defaultFilters(), range: { start: '1', end: '2' } });
    if (empty.status !== 'ready') throw new Error('Expected empty page');
    expect(empty.value.rows).toEqual([]); expect(empty.value.summary?.fame_delta).toBeNull();
    const first = await preview.client.listRuns({ ...defaultFilters(), limit: 2 });
    if (first.status !== 'ready') throw new Error('Expected page');
    const second = await preview.client.listRuns({ ...defaultFilters(), cursor: first.value.next });
    if (second.status !== 'ready') throw new Error('Expected next page');
    expect(second.value.summary).toBeNull();
  } finally { preview.dispose(); }
});


it.each(['default', 'stress'] as const)('varies local days and gameplay metrics in %s preview', async variant => {
  const preview = createRunsPreview(variant, now);
  try {
    const reply = await preview.client.listRuns({ ...defaultFilters(), limit: 100 });
    if (reply.status !== 'ready') throw new Error('Expected synthetic page');
    const { rows, details = [] } = reply.value;
    const groups = dayGroups(rows, new Map(details!.map(detail => [detail.run_id, detail])), now);
    expect(groups.map(group => group.label)).toEqual(expect.arrayContaining(['Today', 'Yesterday', 'Tue, Sep 29']));
    expect(groups[0].rows.every(run => Number(BigInt(run.started) / 1000n) < now.getTime())).toBe(true);
    for (const field of ['clear_micros', 'local_share', 'fame_delta', 'class_id'] as const) {
      expect(new Set(details!.map(detail => detail[field]).filter(value => value !== null)).size).toBeGreaterThanOrEqual(3);
    }
    expect(details!.some(detail => vsBest(detail)?.text.startsWith('+'))).toBe(true);
    const pbs = details!.filter(detail => detail.is_personal_best);
    expect(new Set(pbs.map(detail => detail.clear_micros)).size).toBeGreaterThan(1);
    expect(pbs.some(detail => vsBest(detail)?.text.startsWith('−'))).toBe(true);
    for (const detail of pbs) {
      if (detail.previous_best_clear_micros == null) expect(vsBest(detail)).toBeNull();
      else expect(vsBest(detail)).toEqual(expect.objectContaining({ tone: 'good' }));
    }
    if (variant === 'default') {
      expect(vsBest(details!.find(detail => detail.run_id === 'preview-run-0'))).toEqual({ text: '−0:28', tone: 'good' });
      expect(vsBest(details!.find(detail => detail.run_id === 'preview-run-1'))).toBeNull();
    }
    expect(new Set(details!.map(detail => detail.observed_drop_count))).toEqual(new Set([0, 1, 2, 3, 4, 5]));
    if (variant === 'stress') expect(rows).toHaveLength(60);
  } finally { preview.dispose(); }
});

it('pages 60 stress runs through the real list and retains the total', async () => {
  window.history.replaceState(null, '', '/?preview=runs&variant=stress#/runs');
  render(RunsPreview, { variant: 'stress' });
  const main = within(screen.getByRole('main'));
  await main.findByText('1–25 of 60');
  expect(screen.getByRole('main').querySelectorAll('a[data-row-id]')).toHaveLength(25);
  await fireEvent.click(main.getByRole('button', { name: 'Next' }));
  await main.findByText('26–50 of 60');
  expect(screen.getByRole('main').querySelectorAll('a[data-row-id]')).toHaveLength(25);
  await fireEvent.click(main.getByRole('button', { name: 'Next' }));
  await main.findByText('51–60 of 60');
  expect(screen.getByRole('main').querySelectorAll('a[data-row-id]')).toHaveLength(10);
  expect(main.getByRole('button', { name: 'Next' }).getAttribute('aria-disabled')).toBe('true');
  await fireEvent.click(main.getByRole('button', { name: 'Previous' }));
  await main.findByText('26–50 of 60');
});

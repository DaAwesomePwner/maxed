import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import Evidence from '../../src/lib/runs/Evidence.svelte';
import BagList from '../../src/lib/home/v3/BagList.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { retainedBag } from '../runs-a4/loot-fixtures';
import { flush, mountRuns, ready, run, story, fight, player, source } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); setDebugMode(false); });
afterEach(() => { cleanup(); setDebugMode(false); vi.clearAllTimers(); vi.useRealTimers(); });

it('keeps outcome tooltips and estimated clear badges debug-only', async () => {
  render(Evidence, { estimated: true, evidence: [{ at: '9000000', evidence: {
    completion_counter: { before: 1, after: 2, delay_micros: 1000000 },
  } }] });
  await flush();
  expect(screen.queryByRole('button', { name: 'Outcome Evidence' })).toBeNull();
  expect(screen.queryByText(/Estimated clear time/i)).toBeNull();
  setDebugMode(true); await flush();
  expect(screen.getByRole('button', { name: 'Outcome Evidence' })).toBeTruthy();
  expect(screen.getByText(/Estimated clear time/i)).toBeTruthy();
  setDebugMode(false); await flush();
  expect(screen.queryByRole('button')).toBeNull();
});

it('keeps loot caveats and removal tooltips debug-only', async () => {
  const page = render(BagList, { drops: [retainedBag({ contents_known: false, source_estimated: true,
    source_reason: 'Estimated source; partial capture', issues: ['missing_stats'] })],
    visuals: emptyHomeVisuals(), scroll: true, resolveSpriteUrl: () => null, clock: () => '—' });
  await flush();
  const loot = page.container;
  expect(loot.textContent).not.toMatch(/estimated|partial|unseen|observed|unavailable|missing/i);
  expect(loot.querySelector('[data-debug-note]')).toBeNull();
  expect(loot.querySelectorAll('.item')).toHaveLength(3);
  setDebugMode(true); await flush();
  expect(loot.textContent).toMatch(/Source estimated/);
  expect(loot.textContent).toMatch(/Some stats are unavailable/i);
});

it.each(['#/runs', '#/runs/run-1', '#/runs/run-1/fights/fight-1', '#/runs/run-1/fights/fight-1/players/7'])
('hides caveats in production at %s', async hash => {
  const selected = player({ sources: [source({ estimated: true, reasons: ['Estimated reconstruction; missing stats'] })] });
  mountRuns({ hash, queries: {
    getRun: async () => ready(story({ run: run({ complete_capture: false, issues: ['missing_stats'], party_size_estimated: true }),
      timeline: [{ id: 'gap', at: '2000000', kind: 'capture_issue', related_id: null, explanation: 'Partial capture; estimated timing' }] })),
    getFight: async () => ready(fight({ complete_capture: false, issues: ['missing_stats'], players: [selected],
      kill: { at: '8000000', evidence: 'hp_zero_removed', provisional: true } })),
    getPlayerBreakdown: async () => ready(selected),
  } });
  await flush();
  const view = document.querySelector('.runs, .run-story, .fight, .player')!;
  expect(view).toBeTruthy();
  expect(view.querySelector('[data-debug-note]')).toBeNull();
  let estimate: HTMLElement | null = null;
  if (hash === '#/runs/run-1/fights/fight-1') {
    // Fight v3 retains quiet uncertainty tags while hiding evidence prose.
    estimate = screen.getByText('~100% est.', { selector: '.tag' });
    expect(estimate.getAttribute('title')).toBe("Estimated share of this item's damage");
    expect(screen.getByText('partial', { selector: '.tag' })).toBeTruthy();
    expect(view.textContent).not.toMatch(/provisional|partial capture|reconstruction|observed|unverified|coverage/i);
  } else if (hash === '#/runs/run-1/fights/fight-1/players/7') {
    // Shared player cards retain missing-data copy and quiet estimate tags.
    expect(screen.getByText('partial', { selector: '.player-tag' })).toBeTruthy();
    expect(screen.getByText('est.')).toBeTruthy();
    expect(screen.getByText('No boss fights observed')).toBeTruthy();
    expect(view.textContent).not.toMatch(/estimated|provisional|partial capture|reconstruction|unverified|coverage/i);
  } else expect(view.textContent).not.toMatch(/estimated|provisional|partial capture|reconstruction|observed|unverified|coverage/i);
  for (const node of view.querySelectorAll('[title], [aria-label], .sr-only')) {
    // The item's estimated share is intentional production guidance, checked above.
    if (node === estimate) continue;
    expect(`${node.getAttribute('title') ?? ''} ${node.getAttribute('aria-label') ?? ''} ${node.classList.contains('sr-only') ? node.textContent : ''}`)
      .not.toMatch(/estimated|provisional|partial|unavailable because|coverage/i);
  }
});

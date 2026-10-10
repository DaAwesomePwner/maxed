import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import RecentRuns from '../../src/lib/home/RecentRuns.svelte';
import type { DropView } from '../../src/lib/bindings/DropView';
import { homeVisualTypes } from '../../src/lib/home/visuals';
import { timestamp } from '../../src/lib/runs/story';
import { homeSummary } from './fixtures';
import { flush } from '../runs/fixtures';
import { retainedBag } from '../runs-a4/loot-fixtures';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

function mount(drop: DropView) {
  const summary = homeSummary();
  return render(RecentRuns, { runs: summary.recent_runs, details: [{ ...summary.recent_details[0], loot: [drop] }] });
}

async function revealContents(container: HTMLElement) {
  for (const summary of container.querySelectorAll('details:not([open]) > summary')) await fireEvent.click(summary);
  for (const tooltip of container.querySelectorAll('.tooltip-root')) await fireEvent.pointerEnter(tooltip);
  await flush();
}

it('recent shows retained items with short taken labels and no nested controls', async () => {
  const page = mount(retainedBag()); await flush();
  expect(screen.queryByText('Rank unavailable; unranked preview')).toBeNull();
  await revealContents(page.container);
  expect(screen.getByText('Items')).toBeTruthy();
  expect(page.container.querySelectorAll('[data-item-key]')).toHaveLength(3);
  for (const at of ['2000000', '3000000']) {
    const note = screen.getByText(`Taken · ${timestamp(at)}`);
    expect(note.tagName).toBe('SPAN'); expect(note.getAttribute('title')).toBeNull();
    expect(note.closest('button, .badge, [data-rarity]')).toBeNull();
  }
  for (const link of page.container.querySelectorAll('a')) expect(link.querySelector('a, button, input, summary, [tabindex]')).toBeNull();
});

it('recent does not conflate unknown, partial, observed empty, or previously removed contents', async () => {
  let page = mount(retainedBag({ contents_known: false, items: [], contents: [] })); await flush();
  await revealContents(page.container);
  expect(page.container.querySelector('.contents')?.textContent).toContain('\u2014'); expect(screen.queryByText('Empty')).toBeNull();
  page.unmount();
  page = mount(retainedBag({ issues: ['missing_stats'] })); await flush();
  await revealContents(page.container);
  expect(screen.queryByText(/Partially observed contents/)).toBeNull(); expect(screen.getByText(`Taken · ${timestamp('2000000')}`)).toBeTruthy();
  page.unmount();
  page = mount(retainedBag({ items: [], contents: [] })); await flush();
  await revealContents(page.container);
  expect(screen.getByText('Empty')).toBeTruthy(); expect(screen.queryByText('Contents unavailable')).toBeNull();
  page.unmount();
  const removed = retainedBag(); removed.contents.forEach(item => item.taken_observed_at ??= '4000000');
  page = mount(removed); await flush();
  await revealContents(page.container);
  expect(screen.queryByText(/Current contents are unavailable; an item may have reappeared/)).toBeNull();
  expect(screen.queryByText('Empty')).toBeNull();
});

it('includes retained item visuals even when current contents cannot be confirmed', () => {
  const types = homeVisualTypes(null, homeSummary({ best_drop: retainedBag({ contents_known: false }), recent_runs: [], recent_details: [] }));
  expect(types).toContain(444); expect(types).toContain(555);
});

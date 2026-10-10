import { setDebugMode } from '../../src/lib/debug/debug';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import LootContents from '../../src/lib/home/LootContents.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { timestamp } from '../../src/lib/runs/story';
import { flush } from '../runs/fixtures';
import { retainedBag } from './loot-fixtures';

beforeEach(() => { setDebugMode(true); vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { setDebugMode(false); cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('shows retained appearances and plain taken times, keyed by slot and item type across revisions', async () => {
  const drop = retainedBag(), visuals = emptyHomeVisuals(), onWindow = vi.fn();
  const page = render(LootContents, { drop, visuals, unknownText: 'Contents unseen' }); await flush();
  const loot = page.container, scope = within(loot);
  expect(scope.getByText('Items')).toBeTruthy();
  expect(loot.querySelectorAll('[data-item-key]')).toHaveLength(3);
  expect(scope.getByText(`Taken \u00b7 ${timestamp('2000000')}`)).toBeTruthy();
  const item = loot.querySelector('[data-item-key="0:444"]');
  const changed = retainedBag(); changed.contents[0].quantity = 3;
  await page.rerender({ drop: changed, visuals }); await flush();
  expect(loot.querySelector('[data-item-key="0:444"]')).toBe(item); expect(item?.textContent).toContain('\u00d7 3');
  expect(scope.queryAllByRole('link')).toHaveLength(0);
  expect(scope.getByText(`Taken \u00b7 ${timestamp('2000000')}`).tagName).toBe('SPAN');
});

it('keeps unseen, partial, observed empty and historically removed contents distinct', async () => {
  const removed = retainedBag(); removed.contents.forEach(item => item.taken_observed_at ??= '4000000');
  const cases = [
    [retainedBag({ contents_known: false, contents: [], items: [] }), 'Contents unseen'],
    [retainedBag({ issues: ['missing_stats'] }), 'Partially observed contents'],
    [retainedBag({ contents: [], items: [] }), 'Empty'],
    [removed, 'Current contents are unavailable'],
  ] as const;
  for (const [drop, text] of cases) {
    const page = render(LootContents, { drop, visuals: emptyHomeVisuals(), unknownText: 'Contents unseen' }); await flush();
    expect(page.container.textContent).toContain(text); page.unmount();
  }
});

it('retains observed appearances when a later partial projection cannot confirm contents', async () => {
  render(LootContents, { drop: retainedBag({ contents_known: false, issues: ['missing_stats'] }), visuals: emptyHomeVisuals(), unknownText: 'Contents unseen' }); await flush();
  expect(screen.getByText('Contents unseen')).toBeTruthy();
  expect(screen.getByText('Items')).toBeTruthy();
  expect(screen.getByText(`Taken \u00b7 ${timestamp('2000000')}`)).toBeTruthy();
  expect(screen.queryByText('Observed empty contents')).toBeNull();
});

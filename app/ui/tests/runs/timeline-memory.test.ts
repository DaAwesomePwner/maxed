import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as derivation from '../../src/lib/runs/run-screen';
import { advance, click, flush, largeStory, mountRuns, ready, region, saved } from './fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it.each([4, 64])('shares one marker derivation across tab and focus changes (%i fights)', async count => {
  const value = largeStory(count, true);
  const timeline = vi.spyOn(derivation, 'runMarkers');
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } });
  await flush();
  expect(timeline).toHaveBeenCalledTimes(1);
  const first = region('Timeline').getAllByRole('link').map(node => node.textContent);
  await fireEvent.click(screen.getAllByRole('tab')[1]); await flush();
  await fireEvent.click(screen.getByRole('tab', { name: 'Whole run' })); await flush();
  region('Timeline').getAllByRole('link')[0].focus();
  await fireEvent.scroll(screen.getByRole('main'), { target: { scrollTop: 123 } });
  await flush();
  expect(timeline).toHaveBeenCalledTimes(1);
  const replacement = structuredClone(value);
  replacement.run.dungeon = 'Refreshed saved Story';
  page.queries.getRun.mockResolvedValue(ready(replacement));
  page.event({ run_updated: replacement.run }, '13');
  page.setStore(saved('13')); await advance();
  expect(timeline).toHaveBeenCalledTimes(2);
  expect(timeline.mock.calls.at(-1)?.[0].run.dungeon).toBe('Refreshed saved Story');
  expect(region('Timeline').getAllByRole('link').map(node => node.textContent)).toEqual(first);
});

it('keeps every saved bag in a keyboard-scrollable list and reveals a far-down timeline selection', async () => {
  const value = largeStory(1);
  const seed = value.drops[0];
  value.drops = Array.from({ length: 128 }, (_, i) => ({ ...seed, id: `scroll-bag-${i}`, at: String(i === 0 ? 2000000 : 5000000 + i) }));
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } });
  await flush();
  const list = page.container.querySelector<HTMLElement>('.bag-rows')!;
  // The inline overflow contract overrides the shared non-scroll shorthand;
  // jsdom cannot verify actual scrollHeight or browser wheel scrolling.
  expect(list.style.overflowY).toBe('auto');
  expect(list.tabIndex).toBe(0);
  expect(region('Loot this run').getByRole('region', { name: 'All bags' })).toBe(list);
  const rows = list.querySelectorAll<HTMLElement>('[data-drop-id]');
  expect(rows).toHaveLength(127);
  const tally = page.container.querySelector('.loot-tally')!;
  expect(list.contains(tally)).toBe(false);
  expect(list.compareDocumentPosition(tally) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  const last = rows[126];
  const scrollIntoView = vi.fn();
  Object.defineProperty(last, 'scrollIntoView', { value: scrollIntoView, configurable: true });
  const focus = vi.spyOn(last, 'focus');
  await fireEvent.click(page.container.querySelector(`[data-marker-id="drop:${last.dataset.dropId}"]`)!);
  await flush();
  expect(last.classList.contains('selected')).toBe(true);
  expect(document.activeElement).toBe(last);
  expect(focus).toHaveBeenCalledWith({ preventScroll: true });
  expect(scrollIntoView).toHaveBeenCalledWith({ block: 'nearest' });
});

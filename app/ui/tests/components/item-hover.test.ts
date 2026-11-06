import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import ItemHoverCard from '../../src/lib/components/ItemHoverCard.svelte';
import { closeItemHover, itemHoverParams } from '../../src/lib/components/item-hover';
import type { ItemDetailLookup } from '../../src/lib/bridge/item-details';
import ItemHoverHarness from './fixtures/ItemHoverHarness.svelte';
import { lookup } from './fixtures/item-lookup';

const matches = Element.prototype.matches;
function focusVisible(value: boolean | Error) {
  vi.spyOn(Element.prototype, 'matches').mockImplementation(function (this: Element, selector) {
    if (selector !== ':focus-visible') return matches.call(this, selector);
    if (value instanceof Error) throw value;
    return value;
  });
}
beforeEach(() => { vi.useFakeTimers(); focusVisible(true); });
afterEach(() => { closeItemHover(); vi.restoreAllMocks(); });
function setup(getItemDetails = vi.fn(async (_type: number, _ids: number[]) => lookup)) {
  render(ItemHoverCard, { client: { getItemDetails } });
  const fixture = render(ItemHoverHarness);
  return { ...fixture, getItemDetails, a: screen.getByRole('link', { name: 'Item A' }), b: screen.getByRole('link', { name: 'Item B' }) };
}
async function advance(ms: number) { await vi.advanceTimersByTimeAsync(ms); await tick(); }
it('opens after the delay', async () => {
  const { a, getItemDetails } = setup();
  await fireEvent.pointerEnter(a);
  expect(getItemDetails).toHaveBeenCalledWith(10, []);
  await advance(249);
  expect(screen.queryByRole('tooltip')).toBeNull();
  await advance(1);
  expect(screen.getByRole('tooltip').textContent).toContain('Synthetic Staff');
});
it('keyboard focus opens immediately', async () => {
  const { a } = setup();
  await fireEvent.focusIn(a);
  expect(screen.getByRole('tooltip')).toBeTruthy();
});
it.each([false, new Error('Unsupported selector')])('ignores focus without supported focus-visible matching (%s)', async value => {
  focusVisible(value);
  const { a, getItemDetails } = setup();
  await fireEvent.focusIn(a); await advance(250);
  expect(screen.queryByRole('tooltip')).toBeNull();
  expect(a.getAttribute('aria-describedby')).toBe('existing');
  expect(getItemDetails).not.toHaveBeenCalled();
});
it('warm swap', async () => {
  const { a, b } = setup();
  await fireEvent.pointerEnter(a); await advance(250);
  await fireEvent.pointerLeave(a); await advance(100); await fireEvent.pointerEnter(b);
  expect(screen.getByRole('tooltip')).toBeTruthy();
  expect(b.getAttribute('aria-describedby')).toContain('item-hover-card');
  await fireEvent.pointerLeave(b); await advance(301); await fireEvent.pointerEnter(a);
  expect(screen.queryByRole('tooltip')).toBeNull();
  await advance(249); expect(screen.queryByRole('tooltip')).toBeNull();
  await advance(1); expect(screen.getByRole('tooltip')).toBeTruthy();
});
it('Escape closes', async () => {
  const { a } = setup(); await fireEvent.focusIn(a);
  await fireEvent.keyDown(window, { key: 'Escape' });
  expect(screen.queryByRole('tooltip')).toBeNull();
});
it('window blur closes the card and removes its description', async () => {
  const { a } = setup(); await fireEvent.pointerEnter(a); await advance(250);
  await fireEvent.blur(window);
  expect(screen.queryByRole('tooltip')).toBeNull();
  expect(a.getAttribute('aria-describedby')).toBe('existing');
});
it('window blur cancels a pending open', async () => {
  const { a } = setup(); await fireEvent.pointerEnter(a); await advance(100);
  await fireEvent.blur(window); await advance(250);
  expect(screen.queryByRole('tooltip')).toBeNull();
  expect(a.getAttribute('aria-describedby')).toBe('existing');
});
it('focus moves inside the window do not dismiss a hovered card', async () => {
  const { a } = setup();
  const first = document.createElement('button'), second = document.createElement('button');
  document.body.append(first, second);
  first.focus(); await fireEvent.pointerEnter(a); await advance(250);
  second.focus(); await tick();
  expect(document.activeElement).toBe(second);
  expect(screen.getByRole('tooltip')).toBeTruthy();
  expect(a.getAttribute('aria-describedby')).toBe('existing item-hover-card');
  first.remove(); second.remove();
});
it('aria-describedby is set only while open', async () => {
  const { a } = setup(); await fireEvent.pointerEnter(a);
  expect(a.getAttribute('aria-describedby')).toBe('existing');
  await advance(250);
  expect(a.getAttribute('aria-describedby')).toBe('existing item-hover-card');
  await fireEvent.pointerLeave(a);
  expect(a.getAttribute('aria-describedby')).toBe('existing');
});
it('ignores a stale lookup', async () => {
  let resolveA!: (result: ItemDetailLookup) => void;
  const getItemDetails = vi.fn((type: number, _ids: number[]) => type === 10 ? new Promise<ItemDetailLookup>(resolve => { resolveA = resolve; }) : Promise.resolve({ ...lookup, item: { ...lookup.item, object_type: 11, name: 'Synthetic Wand' } }));
  const { a, b } = setup(getItemDetails);
  await fireEvent.pointerEnter(a); await advance(250);
  await fireEvent.pointerLeave(a); await fireEvent.pointerEnter(b);
  expect(screen.getByRole('tooltip').textContent).toContain('Synthetic Wand');
  resolveA(lookup); await advance(0);
  expect(screen.getByRole('tooltip').textContent).toContain('Synthetic Wand');
  expect(screen.getByRole('tooltip').textContent).not.toContain('Synthetic Staff');
});
it('follows param updates', async () => {
  const { a, rerender, getItemDetails } = setup(); await fireEvent.focusIn(a);
  await rerender({ suffix: '×3' });
  expect(screen.getByRole('tooltip').textContent).toContain('UT Staff · ×3');
  expect(getItemDetails).toHaveBeenCalledOnce();
});
it('closes when the anchor is destroyed', async () => {
  const { a, unmount } = setup(); await fireEvent.focusIn(a); unmount(); await tick();
  expect(screen.queryByRole('tooltip')).toBeNull();
});
it('sets data-item-hover', () => {
  const { a } = setup();
  expect(a.getAttribute('data-item-hover')).toBe('10');
  expect(itemHoverParams(a)).toEqual({ type: 10, name: 'Fallback A', suffix: '' });
});
it('renders pending rows and then catalog details with rarity before the suffix', async () => {
  let resolve!: (result: ItemDetailLookup) => void;
  const { a, rerender, getItemDetails } = setup(vi.fn((_type: number, _ids: number[]) => new Promise<ItemDetailLookup>(done => { resolve = done; })));
  await rerender({ first: { type: 10, name: 'Pending Staff', tier: 'UT', enchant: { state: 'recorded', slots: 1, applied: 1, ids: [1] } }, suffix: '×3' });
  await fireEvent.focusIn(a);
  expect(getItemDetails).toHaveBeenCalledWith(10, [1]);
  expect(screen.getByRole('tooltip').textContent).toContain('Enchant #1');
  expect(screen.queryByText('not in catalog')).toBeNull();
  resolve(lookup); await advance(0);
  expect(screen.getByRole('tooltip').querySelector('.meta')?.textContent).toBe('UT Staff · Uncommon · ×3');
  expect(screen.getByText('Adds damage')).toBeTruthy();
  expect(screen.getByRole('tooltip').textContent).not.toContain('Enchant #1');
});
it('refetches updated lookup inputs on the same anchor', async () => {
  const { a, rerender, getItemDetails } = setup();
  await fireEvent.focusIn(a);
  await rerender({ first: { type: 11, enchant: { state: 'recorded', slots: 1, applied: 1, ids: [2] } } });
  expect(getItemDetails).toHaveBeenLastCalledWith(11, [2]);
  expect(getItemDetails).toHaveBeenCalledTimes(2);
  expect(a.getAttribute('data-item-hover')).toBe('11');
  expect(screen.getByText('not in catalog')).toBeTruthy();
});
it('failed lookup retries on the next hover', async () => {
  const getItemDetails = vi.fn(async (_type: number, _ids: number[]) => lookup).mockRejectedValueOnce(new Error('Unavailable'));
  const { a } = setup(getItemDetails);
  await fireEvent.focusIn(a); await advance(0);
  expect(screen.getByText('Details unavailable')).toBeTruthy();
  expect(screen.getByRole('tooltip').textContent).toContain('Fallback A');
  await fireEvent.focusOut(a); await fireEvent.focusIn(a);
  expect(getItemDetails).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('tooltip').textContent).toContain('Synthetic Staff');
  expect(screen.queryByText('Details unavailable')).toBeNull();
});
it('cancels pending opens when the pointer leaves', async () => {
  const { a } = setup();
  await fireEvent.pointerEnter(a); await advance(100); await fireEvent.pointerLeave(a); await advance(250);
  expect(screen.queryByRole('tooltip')).toBeNull();
  expect(a.getAttribute('aria-describedby')).toBe('existing');
});
it('a cancelled pending hover does not warm the next item', async () => {
  const getItemDetails = vi.fn(async (type: number, _ids: number[]) => ({ ...lookup, item: { ...lookup.item, object_type: type, name: type === 11 ? 'Synthetic Wand' : 'Synthetic Staff' } }));
  const { a, b } = setup(getItemDetails);
  await fireEvent.pointerEnter(a); await advance(100); await fireEvent.pointerLeave(a);
  await fireEvent.pointerEnter(b); await advance(249);
  expect(screen.queryByRole('tooltip')).toBeNull();
  await advance(1);
  expect(screen.getByRole('tooltip').textContent).toContain('Synthetic Wand');
});

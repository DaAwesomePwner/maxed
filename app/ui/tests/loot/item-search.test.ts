import { closeItemHover, itemHover, itemHoverParams, itemHoverState } from '../../src/lib/components/item-hover';
import { get } from 'svelte/store';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import ItemSearch from '../../src/lib/loot/ItemSearch.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot, deferred } from '../home/fixtures';
import { flush } from '../runs/fixtures';
const matches = [{ object_type: 101, name: 'Invented sword', tier: 'UT' }, { object_type: 102, name: 'Invented ring', tier: 'ST' }];
function mount() {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const search = vi.spyOn(client, 'searchItems').mockResolvedValue(matches), onPick = vi.fn();
  const view = render(ItemSearch, { client, placeholder: 'Find an item', onPick });
  return { ...view, client, search, onPick, input: screen.getByRole('combobox') };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); closeItemHover(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
async function searchResults() {
  const page = mount();
  await fireEvent.input(page.input, { target: { value: 'invented' } }); await vi.advanceTimersByTimeAsync(250); await flush();
  return page;
}
it('debounces typing for 250ms and cancels obsolete timers', async () => {
  const page = mount();
  await fireEvent.input(page.input, { target: { value: 'swo' } }); await vi.advanceTimersByTimeAsync(200);
  await fireEvent.input(page.input, { target: { value: 'sword' } }); await vi.advanceTimersByTimeAsync(249);
  expect(page.search).not.toHaveBeenCalled(); await vi.advanceTimersByTimeAsync(1); await flush();
  expect(page.search).toHaveBeenCalledExactlyOnceWith('sword', 10);
  const option = screen.getAllByRole('option')[0];
  expect(option.hasAttribute('data-item-hover')).toBe(false);
  expect(itemHoverParams(option)).toBeUndefined();
  expect(option.querySelector('[title]')).toBeNull();
  expect(option.hasAttribute('title')).toBe(false);
  for (const anchor of [option.querySelector('.tile')!, screen.getByText(matches[0].name)]) {
    expect(anchor.getAttribute('data-item-hover')).toBe('101');
    expect(itemHoverParams(anchor)).toEqual({ type: 101, name: matches[0].name, tier: 'UT' });
  }
});
it('moves up and down and picks the active result with Enter', async () => {
  const page = mount(); await fireEvent.input(page.input, { target: { value: 'invented' } }); await vi.advanceTimersByTimeAsync(250); await flush();
  await fireEvent.keyDown(page.input, { key: 'ArrowUp' }); expect(screen.getAllByRole('option')[1].getAttribute('aria-selected')).toBe('true');
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await fireEvent.keyDown(page.input, { key: 'Enter' });
  expect(page.onPick).toHaveBeenCalledExactlyOnceWith(matches[0]); expect(screen.queryByRole('listbox')).toBeNull();
});
it('Escape clears text and ignores an in-flight reply', async () => {
  const page = mount(), pending = deferred<typeof matches>(); page.search.mockReturnValue(pending.promise);
  await fireEvent.input(page.input, { target: { value: 'sword' } }); await vi.advanceTimersByTimeAsync(250);
  await fireEvent.keyDown(page.input, { key: 'Escape' }); pending.resolve(matches); await flush();
  expect((page.input as HTMLInputElement).value).toBe(''); expect(screen.queryByRole('listbox')).toBeNull();
});
it('reports failures and retries with Enter', async () => {
  const page = mount(); page.search.mockRejectedValueOnce(new Error('Synthetic failure'));
  await fireEvent.input(page.input, { target: { value: 'sword' } }); await vi.advanceTimersByTimeAsync(250); await flush();
  expect(screen.getByText('Item search unavailable. Press Enter to retry.')).toBeTruthy();
  await fireEvent.keyDown(page.input, { key: 'Enter' }); await flush(); expect(screen.getAllByRole('option')).toHaveLength(2);
});
it('validates character bounds without querying', async () => {
  const page = mount();
  for (const value of ['', 'x'.repeat(65)]) { await fireEvent.input(page.input, { target: { value } }); await vi.advanceTimersByTimeAsync(250); }
  expect(page.search).not.toHaveBeenCalled(); expect(screen.getByText('Enter 1–64 characters to search.')).toBeTruthy();
});
it('opens the keyboard-active option after 250ms', async () => {
  const page = await searchResults(), option = screen.getAllByRole('option')[0];
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(249);
  expect(get(itemHoverState)?.open).toBe(false);
  expect(option.hasAttribute('aria-describedby')).toBe(false);
  await vi.advanceTimersByTimeAsync(1);
  expect(get(itemHoverState)).toMatchObject({ anchor: option, open: true, params: { type: 101, name: matches[0].name, tier: 'UT' } });
  expect(page.input.getAttribute('aria-activedescendant')).toBe(option.id);
  expect(option.getAttribute('aria-describedby')).toBe('item-hover-card');
});
it('moves an open card to the next keyboard-active option immediately', async () => {
  const page = await searchResults(), options = screen.getAllByRole('option');
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(250);
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' });
  expect(get(itemHoverState)).toMatchObject({ anchor: options[1], open: true, params: { type: 102 } });
  expect(options[0].hasAttribute('aria-describedby')).toBe(false);
  expect(options[1].getAttribute('aria-describedby')).toBe('item-hover-card');
});
it('rapid arrows restart the delay for the final option', async () => {
  const page = await searchResults(), options = screen.getAllByRole('option');
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(200);
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(249);
  expect(get(itemHoverState)?.open).toBe(false);
  expect(options[0].hasAttribute('aria-describedby')).toBe(false);
  expect(options[1].hasAttribute('aria-describedby')).toBe(false);
  await vi.advanceTimersByTimeAsync(1);
  expect(get(itemHoverState)).toMatchObject({ anchor: options[1], open: true, params: { type: 102 } });
});
it('opens a keyboard-active option immediately during the warm window', async () => {
  const page = await searchResults();
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(250);
  await fireEvent.blur(page.input); await vi.advanceTimersByTimeAsync(100);
  await fireEvent.keyDown(page.input, { key: 'ArrowUp' });
  expect(get(itemHoverState)).toMatchObject({ anchor: screen.getAllByRole('option')[1], open: true });
});
it.each(['blur', 'Escape', 'typing', 'Enter', 'choose', 'destroy'])('closes the keyboard card on %s', async trigger => {
  const page = await searchResults(), option = screen.getAllByRole('option')[0];
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(250);
  if (trigger === 'blur') await fireEvent.blur(page.input);
  else if (trigger === 'typing') await fireEvent.input(page.input, { target: { value: 'ring' } });
  else if (trigger === 'choose') await fireEvent.click(option);
  else if (trigger === 'destroy') page.unmount();
  else await fireEvent.keyDown(page.input, { key: trigger });
  expect(get(itemHoverState)).toBeNull();
  expect(option.hasAttribute('aria-describedby')).toBe(false);
});
it('cancels a pending keyboard card on input blur', async () => {
  const page = await searchResults();
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(100);
  await fireEvent.blur(page.input); await vi.advanceTimersByTimeAsync(250);
  expect(get(itemHoverState)).toBeNull();
});
it('pointer hover takes over from the keyboard-active option', async () => {
  const page = await searchResults(), options = screen.getAllByRole('option');
  await fireEvent.keyDown(page.input, { key: 'ArrowDown' }); await vi.advanceTimersByTimeAsync(250);
  const anchor = options[1].querySelector<HTMLElement>('.tile')!;
  await fireEvent.pointerEnter(anchor);
  expect(get(itemHoverState)).toMatchObject({ anchor, open: true, params: { type: 102 } });
  expect(options[0].hasAttribute('aria-describedby')).toBe(false);
});
it('clearing search leaves a card on another surface open', async () => {
  const page = await searchResults(), anchor = document.createElement('span');
  document.body.append(anchor);
  const action = itemHover(anchor, { type: 999 });
  await fireEvent.pointerEnter(anchor); await vi.advanceTimersByTimeAsync(250);
  await fireEvent.input(page.input, { target: { value: 'ring' } });
  expect(get(itemHoverState)).toMatchObject({ anchor, open: true, params: { type: 999 } });
  expect(anchor.getAttribute('aria-describedby')).toBe('item-hover-card');
  action.destroy?.(); anchor.remove();
});

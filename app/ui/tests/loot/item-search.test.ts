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
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
it('debounces typing for 250ms and cancels obsolete timers', async () => {
  const page = mount();
  await fireEvent.input(page.input, { target: { value: 'swo' } }); await vi.advanceTimersByTimeAsync(200);
  await fireEvent.input(page.input, { target: { value: 'sword' } }); await vi.advanceTimersByTimeAsync(249);
  expect(page.search).not.toHaveBeenCalled(); await vi.advanceTimersByTimeAsync(1); await flush();
  expect(page.search).toHaveBeenCalledExactlyOnceWith('sword', 10);
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

import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, tick } from 'svelte';
import { createLogsList, releaseLogsRows } from '../../src/lib/logs/list.svelte';
import { effectRoot, reactive } from './helper-state.svelte';

const disposals: Array<() => void> = [];
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); document.body.replaceChildren(); });
function setup(innerScroll = false) {
  const input = reactive({ required: '1', namespace: 'a', pendingCount: 0, cursor: null as number | null,
    next: 1 as number | null, rows: [] as unknown[], available: false, enabled: true, entryKey: 'one' });
  const main = document.createElement('main');
  main.innerHTML = '<h1 tabindex="-1">Logs</h1><div><div class="scroll"></div></div>';
  document.body.append(main);
  const region = main.children[1] as HTMLElement, table = region.firstElementChild as HTMLElement;
  const persist = vi.fn(), onReady = vi.fn(), onStateChange = vi.fn();
  let list!: ReturnType<typeof createLogsList<number>>;
  disposals.push(effectRoot(() => {
    list = createLogsList<number>({ previous: [], scrollTop: 0,
      required: () => input.required, namespace: () => input.namespace, pendingCount: () => input.pendingCount,
      cursor: () => input.cursor, setCursor: value => { input.cursor = value; }, next: () => input.next,
      region: () => region, rows: () => input.rows, persist,
      available: () => input.available, enabled: () => input.enabled, entryKey: () => input.entryKey,
      onReady, onStateChange, innerScroll, ...(innerScroll ? { removedMessage: 'Alert removed. Heading focused.' } : {}),
    });
  }));
  flushSync();
  return { input, main, region, table, list, persist, onReady, onStateChange };
}

it.each([false, true])('defers away from the head, shows only matching data, and resets on namespace changes (inner=%s)', inner => {
  const { input, main, table, list } = setup(inner);
  input.required = '2'; flushSync();
  expect(list.appliedRequired).toBe('2');
  const scroller = inner ? table : main;
  scroller.scrollTop = 50; // Deliberately do not dispatch a scroll event.
  input.required = '3'; flushSync();
  expect(list.appliedRequired).toBe('2'); expect(list.hasNew).toBe(false);
  input.pendingCount = 2; flushSync(); expect(list.hasNew).toBe(true);
  list.showNew(); flushSync();
  expect(list.appliedRequired).toBe('3'); expect(list.hasNew).toBe(false);
  scroller.scrollTop = 0; input.cursor = 7; input.required = '4'; flushSync();
  expect(list.appliedRequired).toBe('3'); expect(list.hasNew).toBe(true);
  input.namespace = 'b'; flushSync();
  expect(list.appliedRequired).toBe('4'); expect(list.hasNew).toBe(false);
});

it('caps the cursor stack at 100 and guards Older, Newer and Latest while paging', () => {
  const { input, list, persist } = setup();
  list.back(); list.first(); expect(persist).not.toHaveBeenCalled();
  for (let i = 1; i <= 105; i++) { input.next = i; list.paging = false; list.next(); }
  expect(list.previous).toHaveLength(100); expect(list.previous[0]).toBe(5); expect(input.cursor).toBe(105);
  list.next(); list.back(); list.first(); expect(persist).toHaveBeenCalledTimes(105);
  list.paging = false; list.back(); expect(input.cursor).toBe(104); expect(list.previous).toHaveLength(99);
  list.paging = false; list.first(); expect(input.cursor).toBeNull(); expect(list.previous).toEqual([]);
  list.paging = false; input.next = null; list.next(); expect(list.paging).toBe(false);
});

it('resets paging and retries only an unchanged applied filter', () => {
  const { list, persist } = setup();
  list.next();
  const commit = vi.fn(), retry = vi.fn();
  list.apply({ query: 'Vexlin' }, { query: '' }, commit, retry);
  expect(list.previous).toEqual([]); expect(list.paging).toBe(false);
  expect(commit).toHaveBeenCalledOnce(); expect(persist).toHaveBeenCalledTimes(2); expect(retry).not.toHaveBeenCalled();
  list.apply({ query: 'Vexlin' }, { query: 'Vexlin' }, commit, retry);
  expect(retry).toHaveBeenCalledOnce();
});

it.each([false, true])('restores row focus or falls back to the heading with the view announcement (inner=%s)', async inner => {
  const { input, main, region, list, onStateChange } = setup(inner);
  const row = document.createElement('button'); row.dataset.rowId = 'synthetic'; region.append(row); row.focus();
  input.rows = [1]; flushSync();
  const replacement = row.cloneNode() as HTMLElement; row.replaceWith(replacement);
  await tick();
  expect(document.activeElement).toBe(replacement); expect(onStateChange).not.toHaveBeenCalled();
  main.scrollTop = 100;
  Object.defineProperties(main, { scrollHeight: { value: 80 }, clientHeight: { value: 50 } });
  input.rows = []; flushSync(); replacement.remove(); await tick();
  expect(document.activeElement).toBe(main.querySelector('h1'));
  expect(list.missingRow).toBe(inner ? 'Alert removed. Heading focused.' : 'Row Removed');
  expect(onStateChange).toHaveBeenLastCalledWith({ focusedRowId: null, scrollTop: 30 });
  list.persist({}, false); expect(list.missingRow).toBe(''); expect(main.scrollTop).toBe(30);
});

it('resets both scroll containers only when requested by persistence', () => {
  const { main, table, list, onStateChange } = setup(true);
  main.scrollTop = 60; table.scrollTop = 30;
  list.persist({ focusedRowId: 'synthetic' }, false);
  expect(onStateChange).toHaveBeenLastCalledWith({ focusedRowId: 'synthetic' }); expect(main.scrollTop).toBe(60);
  list.persist({ focusedRowId: 'synthetic' });
  expect(main.scrollTop).toBe(0); expect(table.scrollTop).toBe(0);
  expect(onStateChange).toHaveBeenLastCalledWith({ focusedRowId: null, scrollTop: 0 });
});

it('releases only once per entry after readiness and cancels a pending release on disposal', async () => {
  const { input, onReady } = setup();
  input.available = true; input.enabled = false; flushSync(); await tick(); expect(onReady).not.toHaveBeenCalled();
  input.enabled = true; flushSync(); await tick(); expect(onReady).toHaveBeenCalledExactlyOnceWith('one');
  input.available = false; flushSync(); input.available = true; flushSync(); await tick(); expect(onReady).toHaveBeenCalledOnce();
  input.entryKey = 'two'; flushSync(); await tick(); expect(onReady).toHaveBeenLastCalledWith('two');
  const canceled = vi.fn();
  const dispose = effectRoot(() => releaseLogsRows({ ready: () => true, enabled: () => true, entryKey: () => 'disposed', onReady: canceled }));
  flushSync(); dispose(); await tick(); expect(canceled).not.toHaveBeenCalled();
});

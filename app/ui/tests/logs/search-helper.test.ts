import { afterEach, expect, it, vi } from 'vitest';
import { flushSync } from 'svelte';
import { createLogsSearch, validLogsQuery } from '../../src/lib/logs/search.svelte';
import { effectRoot, reactive } from './helper-state.svelte';

const disposals: Array<() => void> = [];
afterEach(() => disposals.splice(0).forEach(dispose => dispose()));
function setup() {
  vi.useFakeTimers();
  const filter = reactive({ query: '' });
  const apply = vi.fn((text: string) => { filter.query = search.accept(text); });
  let search!: ReturnType<typeof createLogsSearch>;
  const dispose = effectRoot(() => { search = createLogsSearch({ query: () => filter.query, apply }); });
  disposals.push(dispose); flushSync();
  return { filter, search, apply, dispose };
}
it('debounces the latest text, trims the applied search, and keeps matching input whitespace', () => {
  const { filter, search, apply } = setup();
  search.query = 'Vex'; flushSync(); vi.advanceTimersByTime(249); expect(apply).not.toHaveBeenCalled();
  search.query = ' Vexlin '; flushSync(); vi.advanceTimersByTime(249); expect(apply).not.toHaveBeenCalled();
  vi.advanceTimersByTime(1); flushSync();
  expect(apply).toHaveBeenCalledExactlyOnceWith(' Vexlin '); expect(filter.query).toBe('Vexlin'); expect(search.query).toBe(' Vexlin ');
  vi.advanceTimersByTime(500); expect(apply).toHaveBeenCalledOnce();
});
it('preserves pending text on unrelated apply and writes back an explicit changed search', () => {
  const { filter, search } = setup();
  search.query = ' Quenby '; flushSync();
  search.accept(filter.query); expect(search.query).toBe(' Quenby ');
  expect(search.accept(' Vexlin ')).toBe('Vexlin'); expect(search.query).toBe('Vexlin');
});
it('cancels pending work on disposal', () => {
  const { search, apply, dispose } = setup(); search.query = 'Vexlin'; flushSync(); dispose();
  vi.advanceTimersByTime(250); expect(apply).not.toHaveBeenCalled();
});
it('enforces Unicode character, UTF-8 byte and NUL bounds', () => {
  expect(validLogsQuery('')).toBe(true); expect(validLogsQuery('😀'.repeat(128))).toBe(true);
  expect(validLogsQuery('x'.repeat(129))).toBe(false); expect(validLogsQuery('😀'.repeat(129))).toBe(false);
  expect(validLogsQuery('Vex\u0000lin')).toBe(false);
});

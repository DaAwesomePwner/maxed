import { get } from 'svelte/store';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createRecapPresentation } from '../../src/lib/home/v4/recap-presentation';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

it('counts exactly fifteen seconds of active Home time, preserving the recap', () => {
  const state = createRecapPresentation();
  state.remember('run-a');
  vi.advanceTimersByTime(60_000);
  expect(get(state).banner).toBe(true);
  state.setActive(true);
  vi.advanceTimersByTime(7_000);
  state.setActive(false);
  vi.advanceTimersByTime(60_000);
  state.setActive(true);
  vi.advanceTimersByTime(7_999);
  expect(get(state).banner).toBe(true);
  vi.advanceTimersByTime(1);
  expect(get(state).banner).toBe(false);
  state.open();
  expect(get(state).open).toBe(true);
});

it.each(['setHovered', 'setFocused'] as const)('pauses while %s and does not renew on summary refinement', method => {
  const state = createRecapPresentation();
  state.setActive(true); state.remember('run-a');
  vi.advanceTimersByTime(5_000);
  state[method](true);
  vi.advanceTimersByTime(60_000);
  state.remember('run-a');
  expect(get(state).banner).toBe(true);
  state[method](false);
  vi.advanceTimersByTime(9_999);
  expect(get(state).banner).toBe(true);
  vi.advanceTimersByTime(1);
  expect(get(state).banner).toBe(false);
});

it('hide and Back retain access, replacement starts a fresh banner, reset clears access', () => {
  const state = createRecapPresentation();
  state.setActive(true); state.remember('run-a'); state.hide();
  state.remember('run-a');
  expect(get(state).banner).toBe(false);
  state.open(); expect(get(state).open).toBe(true);
  state.back(); expect(get(state).open).toBe(false);
  state.open(); expect(get(state).open).toBe(true);
  state.remember('visit-b'); expect(get(state)).toEqual({ banner: true, open: false });
  vi.advanceTimersByTime(14_999); expect(get(state).banner).toBe(true);
  state.reset(); state.open();
  expect(get(state)).toEqual({ banner: false, open: false });
  expect(vi.getTimerCount()).toBe(0);
});

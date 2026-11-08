import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { emptyListState, normalizeListState } from '../../src/lib/navigation/state';
import { main, microtasks, mountPhase2 } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('phase2_checkpoint_legacy_state_and_optional_props', () => {
  for (const value of [emptyListState(), normalizeListState({}), normalizeListState(JSON.parse('{"scrollTop":0}'))]) {
    expect(value).toMatchObject({ alertsPosition: null, fightPosition: null });
    expect(normalizeListState(JSON.parse(JSON.stringify(value)))).toEqual(value);
    expect(value.filters.limit).toBe(25);
  }
  const input = { ...emptyListState(), alertsPosition: {
    filter: { kind: null, range: null, query: 'literal %_', cursor: null, limit: 50 },
    previous: Array.from({ length: 130 }, () => null), window: 12,
  }, fightPosition: { playerWindow: 3, selectedPlayerId: 'player-17', sourceWindow: 4 } };
  const normalized = normalizeListState(input);
  expect(normalized).toMatchObject({ alertsPosition: { window: 12, previous: Array(100).fill(null) },
    fightPosition: input.fightPosition });
  expect(normalizeListState(JSON.parse(JSON.stringify(normalized)))).toEqual(normalized);
});

it('checkpoint freezes the shared dense window sizes', async () => {
  const path = '../../src/lib/phase2/contracts.ts';
  const constants: Record<string, unknown> = await import(/* @vite-ignore */ path);
  expect(constants).toMatchObject({ DENSE_STORY_ROWS: 3, DENSE_ALERT_ROWS: 4 });
});

it.each([-1, 1.2, Infinity, NaN, '2'])('phase2_navigation_windows_and_bounds rejects malformed windows (%s)', value => {
  const normalized = normalizeListState({ ...emptyListState(), ...JSON.parse(JSON.stringify({ fightPosition: { playerWindow: value, sourceWindow: value, selectedPlayerId: '\u0000' },
    alertsPosition: { filter: { kind: null, range: null, query: '', cursor: null, limit: 50 }, previous: [], window: value },
  })) });
  expect(normalized).toMatchObject({ fightPosition: { playerWindow: 0, sourceWindow: 0, selectedPlayerId: null } });
  expect(normalized).toMatchObject({ alertsPosition: { window: 0 } });
});

it.each([
  { query: 'x'.repeat(129) }, { query: '\ud83d\ude80'.repeat(129) }, { kind: 'invented' },
  { range: { from: '-1', until: '2' } }, { cursor: { at: 'not-a-timestamp', id: 'bad' } },
])('checkpoint does not persist unsafe alert filters: %j', patch => {
  const result = normalizeListState({ ...emptyListState(), ...JSON.parse(JSON.stringify({ alertsPosition: {
    filter: { kind: null, range: null, query: '', cursor: null, limit: 50, ...patch }, previous: [], window: 0,
  } })) });
  expect(result).toMatchObject({ alertsPosition: null });
});

it('phase2_alert_stub_rows_ready_before_deadline', async () => {
  const page = mountPhase2({ hash: '#/settings' }); await microtasks();
  const began = Date.now();
  await fireEvent.click(main().getByRole('link', { name: 'Alert history' }));
  await microtasks();
  expect(page.browser.adapter.hash).toBe('#/logs/alerts');
  expect(document.activeElement).toBe(main().getByRole('heading', { level: 1, name: 'Logs' }));
  expect(Date.now()).toBe(began);
  expect(screen.queryByText(/previous row is no longer available/i)).toBeNull();
  // Rapid traversal cannot focus the obsolete Alerts entry after its tick.
  page.browser.adapter.go(-1);
  page.browser.adapter.go(1);
  page.browser.adapter.go(-1);
  await microtasks();
  expect(page.browser.adapter.hash).toBe('#/settings');
  expect(document.activeElement).toBe(main().getByRole('heading', { level: 1, name: 'Settings' }));
  expect(Date.now()).toBe(began);
  page.unmount(); await microtasks();
  expect(page.browser.listeners).toBe(0);
});

import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, beforeEach, expect, it } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import FameSparkline from '../../src/lib/home/FameSparkline.svelte';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import { homeSnapshot, homeSummary } from './fixtures';

beforeEach(() => setDebugMode(false));
afterEach(() => setDebugMode(false));

it('keeps chart caveats out of accessible text even in debug mode', async () => {
  const page = render(FameSparkline, {
    points: [{ at: '1000000', character_id: 'a', fame: 42 }],
    period: 'Session', partial: true, sampled: true,
  });
  const chart = screen.getByRole('img', { name: 'Fame over Time Session Fame' });
  expect(page.container.textContent).not.toMatch(/partial|sampled|baseline/i);
  setDebugMode(true); await tick();
  expect(page.container.querySelector('[data-debug-note]')?.textContent).toMatch(/Partial fame coverage/);
  expect(chart.textContent).not.toMatch(/partial|sampled|baseline/i);
  expect(chart.querySelectorAll('circle')).toHaveLength(1);
});

it('keeps session values and short titles while gating history and coverage details', async () => {
  const page = render(NexusHome, {
    snapshot: homeSnapshot(), choice: 'session', retry: () => {},
    history: { phase: 'error', summary: homeSummary({ fame_delta: 25, fame_missing_baselines: 1, fame_sampled: true }),
      message: 'History details: partial coverage', retryAvailable: true },
  });
  expect(screen.getByRole('heading', { name: homeSnapshot().map! })).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Best bag' })).toBeTruthy();
  expect(screen.getByText("Couldn't load. Retry.")).toBeTruthy();
  expect(screen.getByText('+25')).toBeTruthy();
  expect(page.container.querySelector('[data-debug-note]')).toBeNull();
  expect(page.container.textContent).not.toMatch(/partial|sampled|estimated|baseline/i);
  setDebugMode(true); await tick();
  expect(screen.getByText('History details: partial coverage')).toBeTruthy();
  expect(page.container.textContent).toContain('Partial known-character change');
  setDebugMode(false); await tick();
  expect(screen.getByText('+25')).toBeTruthy();
  expect(page.container.querySelector('[data-debug-note]')).toBeNull();
});

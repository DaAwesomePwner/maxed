import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import StatPanel from '../../src/lib/shell/StatPanel.svelte';
import HeroCard from '../../src/lib/shell/HeroCard.svelte';
import LiveStatusCard from '../../src/lib/shell/LiveStatusCard.svelte';
import { railView } from '../../src/lib/shell/rail';
import fixture from '../fixtures/t0-live';

afterEach(() => setDebugMode(false));

it('keeps missing stats and reasons out of production text and accessible names', async () => {
  const { container } = render(StatPanel);
  expect(screen.getByRole('region', { name: 'Stats' })).toBeTruthy();
  expect(container.textContent).not.toMatch(/observed|unavailable|provisional/i);
  expect(container.querySelector('[title]')).toBeNull();
  const hp = screen.getByRole('meter', { name: 'HP' });
  expect(hp.getAttribute('aria-valuetext')).toBe('—/— (—)');
  expect(hp.getAttribute('aria-valuenow')).toBe('0');
  setDebugMode(true); await tick();
  expect(screen.getByText('Stats unavailable · not observed')).toBeTruthy();
  expect(screen.getByTitle('HP fill unavailable: current and a positive total are required')).toBeTruthy();
  expect(hp.getAttribute('aria-label')).toBe('HP');
  expect(hp.getAttribute('aria-valuetext')).toBe('—/— (—)');
  setDebugMode(false); await tick();
  expect(container.querySelector('[data-debug-note], [title]')).toBeNull();
});

it('retains old DTO values and fills without trusting their unevidenced cap claims', async () => {
  const { container } = render(StatPanel, { panel: {
    lines: [{ stat_id: 0, current: 50, total: 100, boost: 0 }, { stat_id: 20, total: 75, boost: 10, at_cap: true }],
    conditions: [], breakdown_provisional: true,
  } });
  expect(container.querySelector('[data-stat-id="20"] dd')?.textContent).toBe('75 (+10)');
  expect(container.querySelector('[data-stat-id="20"]')?.getAttribute('data-tone')).toBe('boosted');
  expect(container.querySelector<HTMLElement>('.fill')?.style.width).toBe('50%');
  expect(container.textContent).not.toMatch(/observed|provisional/i);
  setDebugMode(true); await tick();
  expect(screen.getByText('Observed boost · provisional split')).toBeTruthy();
  expect(screen.getByRole('meter', { name: 'HP' }).getAttribute('aria-valuetext')).toBe('50/100');
  expect(container.querySelector<HTMLElement>('.fill')?.style.width).toBe('50%');
  expect(container.querySelector('[data-stat-id="20"] dd')?.textContent).toBe('75 (+10)');
});

it('keeps identity caveats and capture diagnostics out of the default rail', () => {
  const view = railView(fixture, [], true);
  const hero = render(HeroCard, { view });
  expect(hero.container.textContent).not.toMatch(/unverified|last seen|observed/i);
  expect(screen.getByRole('list', { name: 'Equipment' })).toBeTruthy();
  expect(hero.container.querySelector('[title*="unavailable"]')).toBeNull();
  const live = render(LiveStatusCard, {
    view, capture: { ...fixture.capture, unknown_packets: 10, dropped_frames: 2 },
    busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn(),
  });
  expect(live.container.textContent).not.toMatch(/observed|observation|unknown packets|dropped frames/i);
});

it('shows cap unavailability with debug off and never guesses pots or maxed tone', () => {
  const { container } = render(StatPanel, { panel: {
    lines: [{ stat_id: 20, total: 81, base: 75, cap: 75, boost: 6, exalt: 2, at_cap: null,
      split_evidence: { placement: 'provisional_inside_boost', cap_eligible: false,
        reasons: ['unverified_placement'] } }],
    conditions: [], breakdown_provisional: true,
  } });
  expect(container.querySelector('[data-stat-id="20"] dd')?.firstChild?.textContent).toBe('81 (+6)');
  expect(container.querySelector('[data-tone="max"], .at-cap')).toBeNull();
  expect(screen.getByText(/Cap and pots unavailable: exalt placement unverified/)).toBeTruthy();
});

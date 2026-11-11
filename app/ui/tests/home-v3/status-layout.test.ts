import { render, screen } from '@testing-library/svelte';
import { expect, it, vi } from 'vitest';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import { homeSnapshot, homeSummary } from '../home/fixtures';
import type { HomeHistoryState } from '../../src/lib/home/summary';
import StatusOverlay from '../../src/lib/status/StatusOverlay.svelte';
import { createStatusRegistry, STATUS_REGISTRY } from '../../src/lib/status/registry';

it('keeps the ready panel structure and focus through stale and ready', async () => {
  const history: HomeHistoryState = { phase: 'ready', summary: homeSummary(), message: '', retryAvailable: true };
  const registry = createStatusRegistry();
  render(StatusOverlay, { registry });
  const page = render(NexusHome, { props: { snapshot: homeSnapshot(), history, choice: 'session', retry: vi.fn(), onScopeChange: vi.fn() },
    context: new Map([[STATUS_REGISTRY, registry]]) });
  const panel = page.container.querySelector('.nexus-home')!;
  const elements = [...panel.children];
  const focused = screen.getByRole('button', { name: 'Session' }); focused.focus();
  await page.rerender({ history: { ...history, phase: 'stale', message: 'Refreshing saved observations.' } });
  expect(panel.querySelector('.history-status')).toBeNull();
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toBe('Refreshing history');
  expect([...panel.children]).toEqual(elements);
  expect(document.activeElement).toBe(focused);
  await page.rerender({ history });
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toBe('');
  expect([...panel.children]).toEqual(elements);
  expect(document.activeElement).toBe(focused);
});
it('keeps retry-ready history and loot errors next to their panels', () => {
  const page = render(NexusHome, { snapshot: homeSnapshot(), choice: 'session', retry: vi.fn(),
    history: { phase: 'error', summary: homeSummary(), message: 'Load failed', retryAvailable: true },
    lootStatus: { phase: 'busy', value: null, message: 'Loot busy', retryAvailable: true } });
  expect(page.container.querySelector('.history-status')?.textContent).toContain("Couldn't load history");
  expect(screen.getByRole('button', { name: 'Retry loot' }).closest('.loot-status')?.textContent).toContain('Loot busy');
});

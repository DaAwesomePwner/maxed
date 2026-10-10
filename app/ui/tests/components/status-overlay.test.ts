import { readFileSync } from 'node:fs';
import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { get } from 'svelte/store';
import { expect, it } from 'vitest';
import StatusOverlay from '../../src/lib/status/StatusOverlay.svelte';
import BusyStatus from '../../src/lib/status/BusyStatus.svelte';
import { createStatusRegistry, STATUS_REGISTRY, historyStatus } from '../../src/lib/status/registry';

it('combines concurrent panels, releases only the finished owner and cleans up on unmount', async () => {
  const registry = createStatusRegistry();
  const context = new Map([[STATUS_REGISTRY, registry]]);
  const overlay = render(StatusOverlay, { registry });
  const region = screen.getByRole('status', { name: 'Background activity' });
  const first = render(BusyStatus, { props: { message: 'Refreshing' }, context });
  const second = render(BusyStatus, { props: { message: 'Loading loot' }, context });
  await tick();
  expect(region.textContent).toBe('Refreshing · Loading loot');
  expect(overlay.container.querySelectorAll('[role="status"]')).toHaveLength(1);
  expect(region.getAttribute('aria-live')).toBe('polite');
  expect(region.querySelector('button, a, input, [tabindex], [autofocus]')).toBeNull();
  await first.rerender({ message: null });
  expect(region.textContent).toBe('Loading loot');
  second.unmount(); await tick();
  expect(region.textContent).toBe('');
  expect(get(registry)).toBe('');
});
it('does not steal focus and uses fixed inert styling without motion', async () => {
  const registry = createStatusRegistry();
  render(StatusOverlay, { registry });
  const button = document.createElement('button'); document.body.append(button); button.focus();
  const release = registry.add('Loading'); await tick();
  expect(document.activeElement).toBe(button);
  release(); button.remove();
  const source = readFileSync('src/lib/status/StatusOverlay.svelte', 'utf8');
  expect(source).toMatch(/position:\s*fixed/);
  expect(source).toMatch(/bottom:/); expect(source).toMatch(/right:/);
  expect(source).toMatch(/pointer-events:\s*none/);
  expect(source).toMatch(/prefers-reduced-motion:\s*reduce/);
  expect(source).toMatch(/animation:\s*none/); expect(source).toMatch(/transition:\s*none/);
});
it('classifies automatic refresh separately from retry-ready failures', () => {
  for (const phase of ['loading', 'stale', 'busy']) {
    expect(historyStatus({ phase, retryAvailable: false }, 'Runs')).toMatch(/Loading|Refreshing/);
  }
  for (const phase of ['error', 'ready', 'unavailable', 'superseded']) expect(historyStatus({ phase, retryAvailable: true }, 'Runs')).toBeNull();
  expect(historyStatus({ phase: 'superseded', retryAvailable: false }, 'Runs')).toBeNull();
  expect(historyStatus({ phase: 'busy', retryAvailable: true }, 'Runs')).toBeNull();
  expect(historyStatus({ phase: 'stale', retryAvailable: true, message: 'Saving failed. Retry.' }, 'Runs')).toBeNull();
});

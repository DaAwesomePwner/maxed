import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { parseRoute, productionFeatures } from '../../src/lib/navigation/routes';
import CharacterRail from '../../src/lib/shell/CharacterRail.svelte';
import { railView } from '../../src/lib/shell/rail';

afterEach(cleanup);
it('enables the loot feed in production', () => {
  expect(productionFeatures.lootBrowsing).toBe(true);
  expect(parseRoute('#/loot')).toEqual({ kind: 'loot', view: 'feed' });
});
it('shows the production Loot link between Runs and Logs', () => {
  render(CharacterRail, { route: { kind: 'home' }, features: productionFeatures, view: railView(null), capture: null,
    preview: false, busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
  const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
  expect(nav.getByRole('link', { name: 'Loot' }).getAttribute('href')).toBe('#/loot');
  const labels = nav.getAllByRole('link').map(link => link.textContent?.trim());
  expect(labels.slice(labels.indexOf('Runs'), labels.indexOf('Logs') + 1)).toEqual(['Runs', 'Loot', 'Logs']);
});

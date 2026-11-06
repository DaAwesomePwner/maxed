import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { breadcrumbs, parseRoute, productionFeatures, routeHref, type Route } from '../../src/lib/navigation/routes';
import CharacterRail from '../../src/lib/shell/CharacterRail.svelte';
import { railView } from '../../src/lib/shell/rail';

const on = { lootBrowsing: true, logs: true, stats: true, party: false, character: false, homeRedesign: false }, off = { ...on, stats: false };
const routes: Array<[string, Route]> = [
  ['#/stats', { kind: 'stats', view: 'maps' }],
  ['#/stats/sessions', { kind: 'stats', view: 'sessions' }],
  ['#/stats/sessions/s%201', { kind: 'stats', view: 'sessions', sessionId: 's 1' }],
  ['#/stats/dungeons', { kind: 'stats', view: 'dungeons' }],
];
afterEach(cleanup);
describe('stats routes', () => {
  it.each(routes)('parses and round-trips %s', (hash, route) => {
    expect(parseRoute(hash, on)).toEqual(route);
    expect(routeHref(route)).toBe(hash);
  });
  it.each(routes)('is not found with the flag off: %s', hash => expect(parseRoute(hash, off)).toEqual({ kind: 'not-found' }));
  it.each(['#/stats/maps', '#/stats/dungeons/x', '#/stats/sessions/a/b'])('rejects %s', hash => {
    expect(parseRoute(hash, on)).toEqual({ kind: 'not-found' });
  });
  it('turns the flag on in production', () => expect(productionFeatures.stats).toBe(true));
  it('builds breadcrumbs', () => {
    expect(breadcrumbs({ kind: 'stats', view: 'maps' }).map(c => c.label)).toEqual(['Home', 'Stats']);
    expect(breadcrumbs({ kind: 'stats', view: 'dungeons' }).map(c => c.label)).toEqual(['Home', 'Stats', 'Dungeons']);
    expect(breadcrumbs({ kind: 'stats', view: 'sessions', sessionId: 's1' }).map(c => c.label))
      .toEqual(['Home', 'Stats', 'Sessions', 'Session']);
  });
  it.each([[on, true], [off, false], [productionFeatures, true]])('shows Stats in the rail only with the flag on', (features, shown) => {
    render(CharacterRail, { route: { kind: 'stats', view: 'maps' }, features, view: railView(null), capture: null,
      preview: true, busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
    const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
    const link = nav.queryByRole('link', { name: 'Stats' });
    expect(link !== null).toBe(shown);
    if (link) {
      expect(link.getAttribute('aria-current')).toBe('page');
      // After Logs.
      const names = nav.getAllByRole('link').map(a => a.textContent);
      expect(names.indexOf('Stats')).toBe(names.indexOf('Logs') + 1);
    }
  });
});

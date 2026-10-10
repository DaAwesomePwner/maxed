import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { breadcrumbs, parseRoute, routeHref, routeTitle, type Route } from '../../src/lib/navigation/routes';
import RouteFrame from '../../src/routes/RouteFrame.svelte';
import { createRouter } from '../../src/lib/navigation/history';
import { fakeHistory } from '../navigation/fake-history';
import CharacterRail from '../../src/lib/shell/CharacterRail.svelte';
import { railView } from '../../src/lib/shell/rail';

const on = { lootBrowsing: true, logs: false, stats: false }, off = { lootBrowsing: false, logs: false, stats: false };
const routes: Array<[string, Route]> = [
  ['#/loot/in/Lost%20Halls', { kind: 'loot', view: 'feed', seed: { place: 'Lost Halls' } }],
  ['#/loot/with/2000', { kind: 'loot', view: 'feed', seed: { item: 2000 } }],
  ['#/loot', { kind: 'loot', view: 'feed' }],
  ['#/loot/stats', { kind: 'loot', view: 'stats' }],
  ['#/loot/collection', { kind: 'loot', view: 'collection' }],
  ['#/loot/item/3180', { kind: 'item', type: 3180 }],
  ['#/loot/3180', { kind: 'item', type: 3180 }],
];
afterEach(cleanup);
describe('loot routes', () => {
  it.each(routes)('parses %s with the feature on', (hash, route) => expect(parseRoute(hash, on)).toEqual(route));
  it.each(routes)('hides %s with the feature off', hash => expect(parseRoute(hash, off)).toEqual({ kind: 'not-found' }));
  it.each(['#/loot/with/70000', '#/loot/item/65536', '#/loot/item/-1', '#/loot/nope', '#/loot/stats/x', '#/loot/collection/x', '#/loot/item', '#/loot/65536', '#/loot/item/1.5'])('rejects %s', hash => {
    expect(parseRoute(hash, on)).toEqual({ kind: 'not-found' });
  });
  it.each([0, 65535])('accepts u16 boundary %s', type => expect(parseRoute(`#/loot/item/${type}`, on)).toEqual({ kind: 'item', type }));
  it.each(routes)('round-trips and canonicalizes %s', (hash, route) => {
    expect(routeHref(route)).toBe(hash === '#/loot/3180' ? '#/loot/item/3180' : hash);
    expect(parseRoute(routeHref(route), on)).toEqual(route);
    expect(routeTitle(route)).toBe(route.kind === 'item' ? 'Item' : 'Loot');
  });
  it('keeps browsing disabled when explicitly opted out', () => expect(parseRoute('#/loot', off)).toEqual({ kind: 'not-found' }));
  it.each(routes)('keeps Loot highlighted in the rail for %s', (_hash, route) => {
    render(CharacterRail, { route, features: on, view: railView(null), capture: null, preview: true,
      busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
    const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
    expect(nav.getByRole('link', { name: 'Loot' }).getAttribute('aria-current')).toBe('page');
    expect(nav.getAllByRole('link').filter(link => link.hasAttribute('aria-current'))).toHaveLength(1);
  });
  it.each([
    ['#/loot', ['Home', 'Loot'], ['#/', '#/loot']],
    ['#/loot/in/Lost%20Halls', ['Home', 'Loot'], ['#/', '#/loot/in/Lost%20Halls']],
    ['#/loot/with/2000', ['Home', 'Loot'], ['#/', '#/loot/with/2000']],
    ['#/loot/stats', ['Home', 'Loot', 'Stats'], ['#/', '#/loot', '#/loot/stats']],
    ['#/loot/collection', ['Home', 'Loot', 'Collection'], ['#/', '#/loot', '#/loot/collection']],
    ['#/loot/item/7', ['Home', 'Loot', 'Item'], ['#/', '#/loot', '#/loot/item/7']],
  ])('builds breadcrumbs for %s', (hash, labels, hrefs) => {
    const crumbs = breadcrumbs(parseRoute(hash as string, on));
    expect(crumbs.map(crumb => crumb.label)).toEqual(labels);
    expect(crumbs.map(crumb => crumb.href)).toEqual(hrefs);
  });
  it.each(routes)('renders the tab and signals rows ready once for %s', async (hash, route) => {
    const onRowsReady = vi.fn();
    render(RouteFrame, { route, snapshot: null, entryKey: hash, onRowsReady });
    if (route.kind === 'item') {
      expect(screen.queryByRole('navigation', { name: 'Loot views' })).toBeNull();
    } else {
      const nav = within(screen.getByRole('navigation', { name: 'Loot views' }));
      const active = route.kind === 'loot' && route.view !== 'feed' ? route.view === 'stats' ? 'Stats' : 'Collection' : 'Feed';
      for (const [name, href] of [['Feed', '#/loot'], ['Stats', '#/loot/stats'], ['Collection', '#/loot/collection']]) {
        const link = nav.getByRole('link', { name });
        expect(link.getAttribute('href')).toBe(href);
        expect(link.getAttribute('aria-current')).toBe(name === active ? 'page' : null);
      }
    }
    expect(screen.getByText('Loot unavailable')).toBeTruthy();
    await waitFor(() => expect(onRowsReady).toHaveBeenCalledExactlyOnceWith(hash));
  });
});

it('shares the range on Loot tab navigation while Back restores each entry independently', () => {
  const router = createRouter(fakeHistory('#/loot').adapter);
  const range = { start: '1000000', end: '2000000' };
  router.save({ lootPosition: { range, feed: { filter: { min_rank: null, place: null, item_type: null,
    enchanted_only: false, range, cursor: null, limit: 50 }, previous: [] }, item: null, statsRange: null, collectionDungeon: null } });
  router.navigate({ kind: 'loot', view: 'stats' });
  expect(router.state.entry.lootPosition?.range).toEqual(range);
  router.save({ lootPosition: { ...router.state.entry.lootPosition!, range: null } });
  router.back(); expect(router.state.entry.lootPosition?.range).toEqual(range);
  router.forward(); expect(router.state.entry.lootPosition?.range).toBeNull();
  router.dispose();
});
it('updates the header immediately without requiring saved-state feedback', async () => {
  const onStateChange = vi.fn();
  render(RouteFrame, { route: { kind: 'loot', view: 'stats' }, snapshot: null, onStateChange });
  const today = screen.getByRole('button', { name: 'Today' });
  await fireEvent.click(today);
  expect(today.getAttribute('aria-pressed')).toBe('true');
  expect(onStateChange.mock.lastCall![0].lootPosition.range).not.toBeNull();
});

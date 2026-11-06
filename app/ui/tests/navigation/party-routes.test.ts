import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { breadcrumbs, parseRoute, productionFeatures, routeHref, routeTitle, type Route } from '../../src/lib/navigation/routes';
import CharacterRail from '../../src/lib/shell/CharacterRail.svelte';
import { railView } from '../../src/lib/shell/rail';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot } from '../home/fixtures';
import { fakeHistory } from './fake-history';
import { flush } from '../runs/fixtures';

const on = { ...productionFeatures, party: true };
const off = { ...on, party: false };
const routes: Array<[string, Route, string]> = [
  ['#/party', { kind: 'party-live' }, 'Live'],
  ['#/party/runs/synth%3Arun%25%2F%201', { kind: 'party-run', runId: 'synth:run%/ 1' }, 'Live'],
  ['#/party/players', { kind: 'party-players' }, 'Players'],
  ['#/party/players/synth%3Aalpha%25%2F%201', { kind: 'party-player', key: 'synth:alpha%/ 1' }, 'Players'],
  ['#/party/presets', { kind: 'party-presets' }, 'Presets'],
];

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

describe('Party routes', () => {
  it.each(routes)('round-trips %s and titles it Party', (hash, route) => {
    expect(routeHref(route)).toBe(hash);
    expect(parseRoute(hash, on)).toEqual(route);
    expect(routeTitle(route)).toBe('Party');
  });

  it.each(routes)('returns Home with the flag off: %s', hash => {
    expect(parseRoute(hash, off)).toEqual({ kind: 'home' });
  });

  it.each(routes)('parses %s with the production features', (hash, route) => {
    expect(parseRoute(hash)).toEqual(route);
  });

  it.each(['#/party/runs', '#/party/players/', '#/party/runs/a/b', '#/party/players/%', '#/party/presets/extra'])('rejects malformed enabled route %s', hash => {
    expect(parseRoute(hash, on)).toEqual({ kind: 'not-found' });
  });

  it('builds Party breadcrumbs with a page-supplied dungeon for saved runs', () => {
    for (const [, route] of routes.filter(([, route]) => route.kind !== 'party-run')) {
      expect(breadcrumbs(route).map(crumb => crumb.label)).toEqual(['Home', 'Party']);
    }
    expect(breadcrumbs({ kind: 'party-run', runId: 'synth-run' }, 'Synthetic Sanctum')).toEqual([
      { label: 'Home', href: '#/' }, { label: 'Party', href: '#/party' },
      { label: 'Synthetic Sanctum', href: '#/runs/synth-run' },
      { label: 'Party run', href: '#/party/runs/synth-run' },
    ]);
    expect(breadcrumbs({ kind: 'party-run', runId: 'synth-run' }).map(crumb => crumb.label)).toEqual(['Home', 'Party', 'Run', 'Party run']);
  });

  it.each(routes)('selects the Party rail section for %s', (_hash, route) => {
    render(CharacterRail, { route, features: on, view: railView(null), capture: null,
      preview: true, busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
    const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
    const party = nav.getByRole('link', { name: 'Party' });
    expect(party.getAttribute('href')).toBe('#/party');
    expect(party.getAttribute('aria-current')).toBe('page');
    expect(nav.getAllByRole('link').filter(link => link.hasAttribute('aria-current'))).toEqual([party]);
  });

  it('turns the flag on in production', () => expect(productionFeatures.party).toBe(true));

  it('shows Party in the production rail', () => {
    render(CharacterRail, { route: { kind: 'home' }, features: productionFeatures, view: railView(null), capture: null,
      preview: true, busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
    expect(screen.getByRole('link', { name: 'Party' }).getAttribute('href')).toBe('#/party');
  });

  it.each(routes)('mounts the real Shell with one Party heading and linked sub-views for %s', async (hash, _route, tab) => {
    render(Shell, { client: createClient(createFixtureTransport(homeSnapshot())), features: on,
      preview: true, historyAdapter: fakeHistory(hash).adapter });
    await flush();
    const nav = within(screen.getByRole('navigation', { name: 'Party views' }));
    expect(nav.getAllByRole('link').map(link => [link.textContent, link.getAttribute('href')])).toEqual([
      ['Live', '#/party'], ['Players', '#/party/players'], ['Presets', '#/party/presets'],
    ]);
    expect(nav.getByRole('link', { name: tab }).getAttribute('aria-current')).toBe('page');
    expect(screen.getAllByRole('heading', { level: 1 })).toEqual([screen.getByRole('heading', { name: 'Party', level: 1 })]);
  });
});

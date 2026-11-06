import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { breadcrumbs, parseRoute, productionFeatures, routeHref, routeTitle } from '../../src/lib/navigation/routes';
import CharacterRail from '../../src/lib/shell/CharacterRail.svelte';
import { railView } from '../../src/lib/shell/rail';

afterEach(cleanup);
const on = { ...productionFeatures, character: true };
it('round-trips Character with its title and breadcrumbs', () => {
  expect(parseRoute('#/character', on)).toEqual({ kind: 'character' });
  expect(routeHref({ kind: 'character' })).toBe('#/character');
  expect(routeTitle({ kind: 'character' })).toBe('Character');
  expect(breadcrumbs({ kind: 'character' })).toEqual([
    { label: 'Home', href: '#/' }, { label: 'Character', href: '#/character' },
  ]);
});
it('falls back to Home with the flag off', () => {
  expect(parseRoute('#/character', { ...productionFeatures, character: false })).toEqual({ kind: 'home' });
});
it('turns the flag on in production', () => expect(productionFeatures.character).toBe(true));
it('parses Character with the production features', () => {
  expect(parseRoute('#/character')).toEqual({ kind: 'character' });
});
it('shows Character in the production rail', () => {
  render(CharacterRail, { route: { kind: 'home' }, features: productionFeatures,
    view: railView(null), capture: null, preview: true, busy: false, error: '',
    onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
  expect(screen.getByRole('link', { name: 'Character' }).getAttribute('href')).toBe('#/character');
});
it.each([true, false])('places Character between Stats and Party only when enabled: %s', character => {
  render(CharacterRail, { route: parseRoute('#/character', { ...on, character }), features: { ...on, character },
    view: railView(null), capture: null, preview: true, busy: false, error: '',
    onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
  const nav = screen.getByRole('navigation', { name: 'Navigation' });
  const links = within(nav).getAllByRole('link').map(link => link.textContent);
  expect(links.slice(- (character ? 3 : 2))).toEqual(character ? ['Stats', 'Character', 'Party'] : ['Stats', 'Party']);
  expect(nav.contains(screen.getByRole('link', { name: 'Settings' }))).toBe(false);
  if (character) expect(within(nav).getByRole('link', { name: 'Character' }).getAttribute('aria-current')).toBe('page');
});

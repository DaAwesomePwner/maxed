import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/svelte';
import CharacterRail from '../../src/lib/shell/CharacterRail.svelte';
import Shell from '../../src/lib/shell/Shell.svelte';
import { productionFeatures } from '../../src/lib/navigation/routes';
import { railView } from '../../src/lib/shell/rail';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { fakeHistory } from '../navigation/fake-history';
import fixture from '../fixtures/t0-live';

const read = (path: string) => readFileSync(resolve(process.cwd(), path), 'utf8');
const componentCss = (name: string) => read(`src/lib/shell/${name}.svelte`).match(/<style>([\s\S]*?)<\/style>/)![1];
let stylesheet: HTMLStyleElement | undefined;
function applyLayoutStyles() {
  // jsdom has no geometry engine. Exercise the authored rules and DOM containment,
  // including legacy global CSS, rather than assert invented bounding rectangles.
  // Isolated scenes allow unscoped component rules to be applied deterministically.
  stylesheet = document.createElement('style');
  stylesheet.textContent = [read('src/lib/styles/tokens.css'), read('src/app.css').replace(/@import[^;]+;/g, ''),
    ...['HeroCard', 'LiveStatusCard', 'CharacterRail', 'Shell'].map(componentCss)].join('\n');
  document.head.append(stylesheet);
}
afterEach(() => { stylesheet?.remove(); stylesheet = undefined; });

describe('rail navigation cannot fall below the card scroller', () => {
  it.each([{ width: 1280, height: 800, railWidth: 240 }, { width: 853, height: 533, railWidth: 192 }])(
    'reserves nonshrinking navigation at $width \u00d7 $height', ({ height, railWidth }) => {
      applyLayoutStyles();
      const { container } = render(CharacterRail, {
        view: railView(fixture), capture: fixture.capture, route: { kind: 'home' },
        features: { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: productionFeatures.homeRedesign }, preview: true, busy: false, error: '',
        onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn(),
      });
      container.style.width = `${railWidth}px`; container.style.height = `${height}px`;
      const rail = screen.getByRole('complementary', { name: 'Character' });
      const cards = rail.querySelector<HTMLElement>('.rail-cards')!;
      const brand = rail.querySelector<HTMLElement>('.rail-top')!;
      const nav = screen.getByRole('navigation', { name: 'Navigation' });
      const settings = screen.getByRole('link', { name: 'Settings' });
      expect([...rail.children]).toEqual([brand, cards, nav, settings]);
      expect(getComputedStyle(rail).display).toBe('flex');
      expect(getComputedStyle(rail).flexDirection).toBe('column');
      expect(getComputedStyle(rail).overflow).toBe('hidden');
      expect(getComputedStyle(cards).minHeight).toBe('0');
      expect(getComputedStyle(cards).flexShrink).toBe('1');
      expect(getComputedStyle(cards).overflowY).toBe('auto');
      for (const fixed of [brand, nav, settings]) {
        expect(getComputedStyle(fixed).flexShrink).toBe('0');
        expect(fixed.parentElement).toBe(rail);
        expect(cards.contains(fixed)).toBe(false);
      }
      expect(getComputedStyle(settings).marginTop).toBe('auto');
      expect(within(nav).getAllByRole('link').map((link) => link.textContent)).toEqual(['Home', 'Runs', 'Loot', 'Alerts']);
      expect(cards.querySelectorAll('.compact-hero, .compact-live')).toHaveLength(2);
      for (const card of cards.children) expect(getComputedStyle(card).flexShrink).toBe('0');
      expect(cards.querySelectorAll('.toggle')).toHaveLength(1);
      expect(within(rail).getByRole('group', { name: 'History' }).closest('.rail-top')).toBe(brand);
    },
  );

  it('keeps content left-aligned with no old toolbar or breadcrumb row', async () => {
    applyLayoutStyles();
    render(Shell, { client: createClient(createFixtureTransport(fixture)), historyAdapter: fakeHistory().adapter });
    await screen.findByRole('heading', { name: 'Home', level: 1 });
    const main = screen.getByRole('main');
    expect(main.querySelector('.route-toolbar')).toBeNull();
    expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
    expect(getComputedStyle(main).display).toBe('flex');
    expect(getComputedStyle(main).flexDirection).toBe('column');
    expect(getComputedStyle(main).placeItems).toBe('normal');
    expect(main.querySelector('.home-content')).toBeTruthy();
    expect(main.querySelector('[data-home="v4"]')).toBeTruthy();
    expect(within(main).getByLabelText('Session HUD')).toBeTruthy();
  });
});

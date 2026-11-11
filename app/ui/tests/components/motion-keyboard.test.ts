import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { tick } from 'svelte';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import HeroCard from '../../src/lib/shell/HeroCard.svelte';
import LiveStatusCard from '../../src/lib/shell/LiveStatusCard.svelte';
import BannerHost from './fixtures/BannerHost.svelte';
import ContentHarness from './fixtures/ContentHarness.svelte';
import { railView } from '../../src/lib/shell/rail';
import fixture from '../fixtures/t0-live';
import { mockAnimations } from './animations';

afterEach(() => { vi.unstubAllGlobals(); });
const read = (path: string) => readFileSync(resolve(process.cwd(), `src/${path}`), 'utf8');

describe('motion and component keyboard regressions', () => {
  it('gives native and aria-disabled buttons the same dimming while preserving the focused ring', () => {
    const style = document.createElement('style');
    style.textContent = read('lib/styles/controls.css');
    const controls = document.createElement('div');
    controls.innerHTML = '<button disabled>Native unavailable</button><button aria-disabled="true">Focusable unavailable</button>';
    document.head.append(style); document.body.append(controls);
    try {
      for (const button of controls.querySelectorAll('button')) {
        expect(getComputedStyle(button).opacity).toBe('0.45');
        expect(getComputedStyle(button).cursor).toBe('default');
      }
      // Pin the authored focus rule; WebView2 painting remains an owner check.
      const rules = [...style.sheet!.cssRules].filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule);
      expect(rules.find((rule) => rule.selectorText === 'button[aria-disabled="true"]:focus-visible')?.style.opacity).toBe('1');
      expect(rules.find((rule) => rule.selectorText === 'button:focus-visible')?.style.outline).toBe('2px solid var(--focus-ring)');
      // Bare buttons must not force a height: explicitly sized buttons (timeline pips, inline triggers) keep their own.
      const bare = rules.find((rule) => rule.selectorText === 'button');
      expect(bare).toBeTruthy();
      expect(bare!.style.getPropertyValue('min-height')).toBe('');
      expect(bare!.style.getPropertyValue('height')).toBe('');
      // The input/select base reaches 32px through padding, so inputs with their own padding keep their own height.
      const inputs = rules.filter((rule) => rule.selectorText.startsWith(':where(input'));
      expect(inputs.length).toBeGreaterThan(0);
      for (const rule of inputs) {
        expect(rule.style.getPropertyValue('height')).toBe('');
        expect(rule.style.getPropertyValue('min-height')).toBe('');
      }
      expect(inputs[0].style.getPropertyValue('border')).toBe('1px solid var(--border-strong)');
    } finally { controls.remove(); style.remove(); }
  });

  it.each([false, true])('keeps only the functional capture clock and preserves hero/banner nodes on status updates (reduce=%s)', async (reduce) => {
    vi.useFakeTimers();
    mockAnimations();
    vi.stubGlobal('matchMedia', vi.fn((query: string): MediaQueryList => ({
      matches: query === '(prefers-reduced-motion: reduce)' && reduce, media: query, onchange: null,
      addListener: vi.fn(), removeListener: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn(), dispatchEvent: vi.fn(() => true),
    })));
    const raf = vi.fn(); vi.stubGlobal('requestAnimationFrame', raf);
    const capture = { ...fixture.capture, state: 'Live' as const };
    const props = { view: railView(fixture), capture, busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() };
    const hero = render(HeroCard, { view: props.view });
    const live = render(LiveStatusCard, props);
    const banner = render(BannerHost, { id: 'same-moment' });
    await tick();
    const heroNode = hero.container.querySelector('section');
    const bannerNode = banner.container.querySelector('.banner');
    const announce = screen.getByText('White bag: Synthetic drop');
    for (let i = 1; i <= 5; i++) {
      await live.rerender({ ...props, capture: { ...capture, unknown_packets: i } });
      await hero.rerender({ view: { ...props.view, name: `Updated name ${i}` } });
      await banner.rerender({ id: 'same-moment', context: `Updated context ${i}` });
      expect(hero.container.querySelector('section')).toBe(heroNode);
      expect(banner.container.querySelector('.banner')).toBe(bannerNode);
      expect(announce.textContent).toBe('White bag: Synthetic drop');
    }
    expect(vi.getTimerCount()).toBe(1); expect(raf).not.toHaveBeenCalled();
    await live.rerender({ capture: { ...capture, state: 'Stopped' } });
    expect(vi.getTimerCount()).toBe(0);
  });

  it('removes CSS motion including pseudo-element shimmer and keeps navigation scroll immediate', () => {
    // jsdom does not evaluate OS media preferences or paint pseudo-elements.
    // Pin the authored CSS contract; visual behavior requires WebView2 QA.
    const motion = read('lib/styles/motion.css');
    const reduced = motion.slice(motion.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(reduced).toMatch(/\*,\s*\*::before,\s*\*::after/);
    for (const property of ['animation', 'transition']) expect(reduced).toContain(`${property}: none !important`);
    expect(reduced).toContain('scroll-behavior: auto !important');
    // The celebration pseudo-element was removed; its reduced-motion rule must not return.
    const bannerSource = read('lib/components/Banner.svelte');
    expect(bannerSource).not.toContain('.celebrate::before');
    expect(bannerSource).not.toMatch(/::before|::after|prefers-reduced-motion/);
    expect(read('lib/navigation/state.ts')).not.toMatch(/behavior:\s*['"]smooth['"]|requestAnimationFrame/);
    const shell = read('lib/shell/Shell.svelte').replace(/\r\n/g, '\n');
    expect(shell).not.toMatch(/behavior:\s*['"]smooth['"]/);
    // The one-shot readiness receipt waits for paint, without animating UI.
    // Exempt exactly that effect; every other effect, handler and template path
    // must remain frame-free, including onMount's router subscription.
    const markerStartAnchor = '  $effect(() => {\n    if (!interactiveReady || !mounted || reportedWindows.has(window)) return;';
    const markerEndAnchor = '    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); };\n  });';
    const markerStart = shell.indexOf(markerStartAnchor);
    const markerEnd = shell.indexOf(markerEndAnchor);
    expect(markerStart).toBeGreaterThanOrEqual(0);
    expect(markerEnd).toBeGreaterThanOrEqual(0);
    expect(markerEnd).toBeGreaterThan(markerStart);
    const markerAfter = markerEnd + markerEndAnchor.length;
    expect(shell.slice(markerStart, markerAfter).match(/requestAnimationFrame/g)).toHaveLength(2);
    expect(shell.slice(0, markerStart) + shell.slice(markerAfter)).not.toContain('requestAnimationFrame');
    for (const path of ['lib/shell/HeroCard.svelte', 'lib/shell/LiveStatusCard.svelte']) expect(read(path)).not.toMatch(/animation:|transition:|requestAnimationFrame/);
    expect(read('lib/shell/HeroCard.svelte')).not.toContain('setInterval');
    expect(read('lib/styles/tokens.css')).toMatch(/:where\(a, button, input, select, textarea, \[tabindex\]\):focus-visible\s*\{\s*outline: 2px solid var\(--focus-ring\)/);
  });

  it('opens evidence with keyboard focus and dismisses it with Escape without moving focus', async () => {
    render(ContentHarness);
    const user = userEvent.setup();
    const evidence = screen.getByRole('button', { name: 'Evidence' });
    await user.tab(); expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Retry capture' }));
    await user.tab(); expect(document.activeElement).toBe(screen.getByRole('link', { name: /The Shatters/ }));
    await user.tab(); expect(document.activeElement).toBe(evidence);
    expect(screen.getByRole('tooltip').id).toBe(evidence.getAttribute('aria-describedby'));
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('tooltip')).toBeNull(); expect(document.activeElement).toBe(evidence);
  });

  it('dismisses a banner using the keyboard', async () => {
    const onDismiss = vi.fn(); render(BannerHost, { onDismiss });
    const user = userEvent.setup(); await user.tab();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Dismiss White bag' }));
    await user.keyboard('{Enter}'); expect(onDismiss).toHaveBeenCalledOnce();
  });
});

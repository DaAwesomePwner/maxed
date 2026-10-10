import { readFileSync } from 'node:fs';
import { cleanup, render } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import BannerHost from '../../src/lib/components/BannerHost.svelte';
import { mockAnimations } from './animations';

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const moment = { id: 'motion', kind: 'key_popped' as const, title: 'Synthetic dungeon', context: '' };

it('animates from empty and back to empty, keeping the outgoing toast inert until its outro finishes', async () => {
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false })));
  const animations = mockAnimations();
  const page = render(BannerHost, { moment: null, onDismiss: () => {}, presentation: 'toast' });
  const events: string[] = [];
  for (const event of ['introstart', 'introend', 'outrostart', 'outroend']) {
    page.container.addEventListener(event, () => events.push(event), true);
  }
  await page.rerender({ moment });
  const element = page.container.querySelector<HTMLElement>('.moment')!;
  expect(element).not.toBeNull();
  expect(animations).toHaveLength(1);
  animations[0].animation.finish(); // Svelte's zero-delay frame.
  expect(events).toEqual(['introstart']);
  expect(animations[1].duration).toBe(180);
  expect(animations[1].frames[0].transform).toContain('-8px');
  animations[1].animation.finish();
  expect(events).toEqual(['introstart', 'introend']);
  await page.rerender({ moment: null, presentation: 'banner' });
  expect(page.container.querySelector('.moment')).toBe(element);
  expect(element.inert).toBe(true);
  expect(page.container.querySelector('.toast-overlay')).not.toBeNull();
  animations[2].animation.finish();
  expect(events.at(-1)).toBe('outrostart');
  // Svelte sets inert; browsers reflect it as an attribute (jsdom does not).
  expect(readFileSync('src/lib/components/BannerHost.svelte', 'utf8'))
    .toMatch(/\.moment:global\(\[inert\]\)\s*\{\s*pointer-events:\s*none/);
  expect(animations[3].duration).toBe(180);
  expect(animations[3].frames.at(-1)?.transform).toContain('-8px');
  animations[3].animation.finish();
  expect(events.at(-1)).toBe('outroend');
  expect(page.container.querySelector('.moment')).toBeNull();
});

it('uses zero duration for both transitions with reduced motion', async () => {
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: true })));
  const animations = mockAnimations();
  const page = render(BannerHost, { moment: null, onDismiss: () => {} });
  await page.rerender({ moment });
  expect(page.container.querySelector('.moment')).not.toBeNull();
  await page.rerender({ moment: null });
  expect(page.container.querySelector('.moment')).toBeNull();
  expect(animations).toHaveLength(0);
});

it('reserves a fixed scrollbar clearance without adding route flow space', () => {
  const source = readFileSync('src/lib/components/BannerHost.svelte', 'utf8');
  expect(source).toMatch(/\.alert-overlay\s*\{[^}]*position:\s*absolute/);
  expect(source).toMatch(/\.alert-overlay\s*\{[^}]*right:\s*20px/);
  expect(source).toMatch(/\.toast-overlay\s*\{[^}]*width:\s*min\(24rem,\s*calc\(100% - 20px\)\)/);
});

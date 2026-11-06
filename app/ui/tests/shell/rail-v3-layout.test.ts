import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
const read = (file: string) => readFileSync(`src/lib/${file}`, 'utf8');
it('reserves fixed tracks, readable tabular numbers and a 12rem narrow rail', () => {
  const bar = read('components/VitalBar.svelte');
  const stats = read('shell/StatPanel.svelte');
  const hero = read('shell/HeroCard.svelte');
  const shell = read('shell/Shell.svelte');
  expect(bar).toMatch(/width: 100%; min-width: 0; height: 21px/);
  expect(bar).toContain('font-variant-numeric: tabular-nums');
  expect(bar).toContain('white-space: nowrap');
  expect(stats).toContain('grid-template-columns: repeat(2, minmax(0, 1fr))');
  expect(stats).toContain('overflow-wrap: anywhere');
  expect(stats).toContain('font-variant-numeric: tabular-nums');
  expect(hero).toContain('grid-template-columns: 60px minmax(0, 1fr)');
  expect(shell).toMatch(/@media \(max-width: 900px\).*grid-template-columns: 12rem minmax\(0, 1fr\)/s);
});

it('uses mockup text sizes at full width and readable minimums in the narrow rail', () => {
  const hero = read('shell/HeroCard.svelte');
  const [wide, narrow] = hero.split('@media (max-width: 900px)');
  const heroRule = (css: string) => css.match(/\.compact-hero\s*\{([^}]+)\}/)![1];
  for (const [property, full, compact] of [
    ['--rail-vital-size', 12, 10],
    ['--rail-label-size', 12, 10],
    ['--rail-stat-size', 14, 11],
  ] as const) {
    expect(heroRule(wide)).toContain(`${property}: ${full}px`);
    expect(heroRule(narrow)).toContain(`${property}: ${compact}px`);
  }
  const bar = read('components/VitalBar.svelte');
  expect(bar).toContain('font: 700 var(--rail-vital-size,');
  expect(bar).toContain('font-size: var(--rail-label-size,');
  const stats = read('shell/StatPanel.svelte');
  expect(stats).toContain('font: 700 var(--rail-stat-size, 14px)');
  expect(stats).toMatch(/@media \(max-width: 900px\)\s*\{\s*\.stat-panel \.stat\s*\{\s*padding-inline: 1px/);
});

it('sizes stat values to their grid when a classic scrollbar consumes rail width', () => {
  // jsdom cannot measure container queries. Assert the authored responsive contract;
  // the browser acceptance gate checks actual fit with debug disclosures open.
  const stats = read('shell/StatPanel.svelte');
  const grid = stats.match(/\.stat-grid\s*\{([^}]+)\}/)![1];
  expect(grid).toContain('container-type: inline-size');
  expect(grid).toContain('container-name: rail-stats');
  // The 197px grid at 1100px also needs the first step for long boosted stats.
  for (const [width, size] of [[212, 13], [184, 12], [172, 11]]) {
    const query = new RegExp(`@container rail-stats \\(max-width: ${width}px\\)\\s*\\{\\s*dd\\s*\\{([^}]+)\\}`);
    expect(stats.match(query)?.[1]).toContain(`font-size: min(var(--rail-stat-size, 14px), ${size}px)`);
  }
  // The narrow rail's readable minimum is 11px; no container step goes below it.
  expect(stats).not.toMatch(/14px\), (?:[1-9]|10)px\)/);
  const value = stats.match(/\n  dd\s*\{([^}]+)\}/)![1];
  expect(value).toContain('overflow-wrap: anywhere');
  expect(value).not.toContain('white-space: nowrap');
  expect(value).not.toMatch(/overflow:|text-overflow:|line-clamp:/);
});

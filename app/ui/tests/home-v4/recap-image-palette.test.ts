import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';
import { recapImageColors, recapImageFont } from '../../src/lib/home/v4/recap-image';

// The canvas cannot read CSS custom properties, so the shareable PNG copies the
// contract values. Pin each copy to its token so the two cannot drift apart.
const tokens = readFileSync('src/lib/styles/tokens.css', 'utf8');
function token(name: string): string {
  const value = tokens.match(new RegExp(`^\\s*${name}:\\s*([^;]+);`, 'm'))?.[1].trim();
  if (!value) throw new Error(`Missing token ${name}`);
  return value;
}

it.each([
  ['background', '--surface-canvas'], ['panel', '--surface-panel'], ['border', '--surface-hover'],
  ['text', '--text-primary'], ['secondary', '--text-secondary'], ['muted', '--text-muted'],
  ['gold', '--gold'], ['bar', '--bar-party-muted'],
  ['green', '--success'], ['red', '--danger'], ['warning', '--warning'],
] as const)('recap_image_%s_matches_%s', (key, name) => {
  expect(recapImageColors[key].toLowerCase()).toBe(token(name).toLowerCase());
});

it('recap_image_palette_has_no_unpinned_colours', () => {
  expect(Object.keys(recapImageColors).sort()).toEqual(
    ['background', 'bar', 'border', 'gold', 'green', 'muted', 'panel', 'red', 'secondary', 'text', 'warning']);
});

it('recap_image_font_matches_the_ui_font', () => {
  expect(recapImageFont).toBe(token('--font-ui'));
});

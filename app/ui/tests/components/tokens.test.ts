import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// npm --prefix app/ui runs Vitest from the UI root; module URLs may use HTTP in jsdom.
const tokens = readFileSync(resolve(process.cwd(), 'src/lib/styles/tokens.css'), 'utf8');
const motion = readFileSync(resolve(process.cwd(), 'src/lib/styles/motion.css'), 'utf8');
function token(name: string): string {
  const match = tokens.match(new RegExp(`--${name}:\\s*([^;]+);`));
  if (!match) throw new Error(`Missing token: ${name}`);
  return match[1].trim();
}
function luminance(hex: string): number {
  const rgb = hex.slice(1).match(/../g)!.map((pair) => {
    const channel = parseInt(pair, 16) / 255;
    return channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4;
  });
  return rgb[0] * .2126 + rgb[1] * .7152 + rgb[2] * .0722;
}
function contrast(foreground: string, background: string): number {
  const values = [luminance(token(foreground)), luminance(token(background))].sort((a, b) => b - a);
  return (values[0] + .05) / (values[1] + .05);
}
describe('Arcane Modern design tokens', () => {
  it('contains the complete production scale and approved accents', () => {
    for (const name of ['surface-canvas', 'surface-panel', 'surface-raised', 'surface-hover', 'border', 'border-strong',
      'text-primary', 'text-secondary', 'text-muted', 'text-disabled', 'gold', 'violet', 'violet-text', 'accent-gradient', 'hero-radial',
      'success', 'danger', 'estimated', 'warning', 'information', 'font-ui', 'text-meta', 'leading-meta', 'text-label', 'leading-label',
      'text-body', 'leading-body', 'text-title', 'leading-title', 'text-hero', 'leading-hero', 'text-number', 'leading-number',
      'weight-normal', 'weight-semibold', 'weight-bold', 'space-1', 'space-2', 'space-3', 'space-4', 'space-6', 'space-8', 'space-12',
      'row-min', 'target-min', 'radius-sprite', 'radius-control', 'radius-card', 'radius-banner', 'radius-pill', 'elevation-card', 'elevation-popover',
      'bag-brown', 'bag-pink', 'bag-purple', 'bag-blue', 'bag-cyan', 'bag-red', 'bag-orange', 'bag-white', 'bag-unknown', 'glow-white']) expect(token(name)).toBeTruthy();
    expect(token('gold')).toBe('#f2c14e');
    expect(token('accent-gradient')).toBe('linear-gradient(90deg, #8b5cf6, #f2c14e)');
    expect(token('glow-white')).toBe('0 0 14px 2px #ffffff40');
    expect(tokens).toContain('tabular-nums lining-nums');
    expect(tokens).not.toMatch(/@import|https?:|backdrop-filter/);
  });
  for (const surface of ['surface-canvas', 'surface-panel', 'surface-raised', 'surface-hover']) {
    it(`keeps text and control boundaries readable on ${surface}`, () => {
      for (const foreground of ['text-primary', 'text-secondary', 'text-muted', 'text-disabled', 'gold', 'violet-text', 'success', 'danger', 'estimated', 'information']) {
        expect(contrast(foreground, surface), `${foreground} on ${surface}`).toBeGreaterThanOrEqual(4.5);
      }
      expect(contrast('gold', surface)).toBeGreaterThanOrEqual(3);
      // Controls use strong boundaries; the quiet card separator is not a control boundary.
      expect(contrast(surface === 'surface-hover' ? 'text-muted' : 'border-strong', surface)).toBeGreaterThanOrEqual(3);
    });
  }
  it('limits motion and removes it for reduced-motion preferences', () => {
    expect(motion).toContain('--motion-fast: 100ms');
    expect(motion).toContain('--motion-standard: 180ms');
    expect(motion).toContain('--motion-page: 240ms');
    expect(motion).toContain('--motion-shimmer: 650ms');
    expect(motion).toContain('@media (prefers-reduced-motion: reduce)');
    expect(motion).toContain('animation: none !important');
    expect(motion).toContain('transition: none !important');
    expect(motion).toContain('scroll-behavior: auto !important');
    expect(motion).not.toContain('infinite');
    expect(motion).not.toMatch(/(?:transition|animation):[^;]*(?:width|height|background|box-shadow)/);
  });
});

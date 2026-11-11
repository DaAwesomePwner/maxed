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
// Follows var() aliases to the authored value, e.g. --estimated -> --warning -> hex.
function resolved(name: string): string {
  const value = token(name);
  const alias = value.match(/^var\(--([a-z0-9-]+)\)$/);
  return alias ? resolved(alias[1]) : value;
}
function luminance(hex: string): number {
  const rgb = hex.slice(1).match(/../g)!.map((pair) => {
    const channel = parseInt(pair, 16) / 255;
    return channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4;
  });
  return rgb[0] * .2126 + rgb[1] * .7152 + rgb[2] * .0722;
}
function contrast(foreground: string, background: string): number {
  const values = [luminance(resolved(foreground)), luminance(resolved(background))].sort((a, b) => b - a);
  return (values[0] + .05) / (values[1] + .05);
}
describe('unslop design tokens', () => {
  it('contains the complete production scale, graphite surfaces and gold accent', () => {
    for (const name of ['surface-canvas', 'surface-page', 'surface-panel', 'surface-raised', 'surface-hover', 'surface-selected', 'surface-inset',
      'hairline', 'hairline-strong', 'border', 'border-strong',
      'text-primary', 'text-secondary', 'text-muted', 'text-disabled', 'gold', 'accent', 'accent-ink', 'accent-soft',
      'bar-party', 'bar-party-muted', 'bar-you', 'bar-track', 'chart-grid', 'chart-label', 'chart-party', 'chart-you',
      'success', 'danger', 'estimated', 'warning', 'information', 'tier-ut', 'tier-st',
      'ch-public', 'ch-guild', 'ch-party', 'ch-whisper', 'ch-npc', 'stat-maxed', 'stat-boosted',
      'font-ui', 'font-mono', 'text-meta', 'leading-meta', 'text-caption', 'leading-caption', 'text-label', 'leading-label',
      'text-body', 'leading-body', 'text-title', 'leading-title', 'text-hero', 'leading-hero', 'text-number', 'leading-number',
      'weight-normal', 'weight-medium', 'weight-semibold', 'space-1', 'space-2', 'space-3', 'space-4', 'space-5', 'space-6', 'space-8', 'space-10', 'space-12',
      'row-min', 'target-min', 'radius-sprite', 'radius-tag', 'radius-control', 'radius-card', 'radius-banner', 'radius-pill', 'elevation-card', 'elevation-popover',
      'focus-ring', 'bag-brown', 'bag-pink', 'bag-purple', 'bag-blue', 'bag-cyan', 'bag-red', 'bag-orange', 'bag-white', 'bag-unknown']) expect(token(name)).toBeTruthy();
    expect(token('gold')).toBe('#f2c14e');
    expect(token('accent')).toBe('var(--gold)');
    expect(token('surface-page')).toBe('#141415');
    expect(token('border')).toBe('var(--hairline)');
    expect(token('focus-ring')).toBe('var(--text-primary)');
    expect(token('elevation-card')).toBe('none');
    expect(token('leading-hero')).toBe('2.25rem');
    expect(token('radius-control')).toBe('.5rem');
    expect(token('stat-maxed')).toBe('#ffe04a');
    expect(token('stat-boosted')).toBe('#5cff5c');
    expect(tokens).toContain('tabular-nums lining-nums');
    expect(tokens).not.toMatch(/@import|https?:|backdrop-filter|gradient\(|8b5cf6|c4a7ff/i);
    expect(tokens).not.toMatch(/--weight-[a-z]+:\s*(?:650|700|800|900)\b/);
  });
  it('keeps deprecated aliases resolving to the contract values during migration', () => {
    expect(token('violet')).toBe('var(--bar-party)');
    expect(token('violet-text')).toBe('var(--text-primary)');
    expect(token('accent-gradient')).toBe('none');
    expect(token('hero-radial')).toBe('var(--surface-panel)');
    expect(token('glow-white')).toBe('none');
    expect(token('weight-bold')).toBe('600');
    expect(motion).toContain('--motion-shimmer: 0ms');
  });
  for (const surface of ['surface-canvas', 'surface-page', 'surface-panel', 'surface-raised', 'surface-hover', 'surface-selected', 'surface-inset']) {
    it(`keeps text and control boundaries readable on ${surface}`, () => {
      // Disabled text is intentionally below 4.5:1 and never carries required information.
      for (const foreground of ['text-primary', 'text-secondary', 'text-muted', 'gold', 'violet-text', 'success', 'danger', 'estimated', 'warning', 'information']) {
        expect(contrast(foreground, surface), `${foreground} on ${surface}`).toBeGreaterThanOrEqual(4.5);
      }
      expect(contrast('gold', surface)).toBeGreaterThanOrEqual(3);
      // Controls use strong boundaries; the quiet card separator is not a control boundary.
      expect(contrast(surface === 'surface-hover' ? 'text-muted' : 'border-strong', surface)).toBeGreaterThanOrEqual(3);
    });
  }
  it('limits motion and removes it for reduced-motion preferences', () => {
    expect(motion).toContain('--motion-fast: 120ms');
    expect(motion).toContain('--motion-standard: 180ms');
    expect(motion).toContain('--motion-page: 320ms');
    expect(motion).toContain('--motion-ease: cubic-bezier(.22,1,.36,1)');
    expect(motion).not.toContain('maxed-shimmer');
    expect(motion).toContain('@media (prefers-reduced-motion: reduce)');
    expect(motion).toContain('animation: none !important');
    expect(motion).toContain('transition: none !important');
    expect(motion).toContain('scroll-behavior: auto !important');
    expect(motion).not.toContain('infinite');
    expect(motion).not.toMatch(/(?:transition|animation):[^;]*(?:width|height|background|box-shadow)/);
  });
});

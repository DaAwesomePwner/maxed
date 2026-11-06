import { render, screen } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import StatPanel from '../../src/lib/shell/StatPanel.svelte';
import type { StatLine } from '../../src/lib/bindings/StatLine';
import type { ExaltPlacement } from '../../src/lib/bindings/ExaltPlacement';
import { setDebugMode } from '../../src/lib/debug/debug';
import { statBoost, statExalt, statPotsLeft } from '../../src/lib/shell/vitals';

const line = (stat_id: number, exalt: number | null, placement: ExaltPlacement = 'provisional_inside_boost'): StatLine => ({
  stat_id, total: 100, current: 50, boost: 20, exalt, base: 80, cap: 85, at_cap: false,
  split_evidence: { placement, cap_eligible: true, reasons: [] },
});
const panel = (lines: StatLine[]) => ({ lines, conditions: [], breakdown_provisional: false });
afterEach(() => setDebugMode(false));

it('shows DTO bonuses directly: level two is +10 HP/MP and +2 for all six core stats', () => {
  const ids = [0, 3, 20, 21, 22, 28, 26, 27];
  const labels = ['HP', 'MP', 'ATT', 'DEF', 'SPD', 'DEX', 'VIT', 'WIS'];
  const { container } = render(StatPanel, { panel: panel(ids.map(id => line(id, id < 4 ? 10 : 2))) });
  ids.forEach((id, index) => {
    const value = id < 4 ? 10 : 2;
    const row = container.querySelector(`[data-stat-id="${id}"]`)!;
    expect(row.querySelector('.exalt')?.textContent).toBe(`Exalt +${value}`);
    expect(screen.getByLabelText(`${labels[index]} exalt bonus +${value}`)).toBeTruthy();
    expect(screen.getByTitle(`${labels[index]} exalt bonus +${value}`)).toBeTruthy();
    expect(row.textContent).toContain('(+20)');
    expect(row.textContent).not.toContain('|');
    expect(statPotsLeft(line(id, value))).toBeNull();
  });
  expect(screen.getByRole('meter', { name: 'HP' }).getAttribute('aria-valuetext')).toBe('50/100 (+20)');
});

it.each([null, 0, undefined])('omits an exalt marker for %s without inventing zero', exalt => {
  const { container } = render(StatPanel, { panel: panel([0, 3, 20, 21, 22, 28, 26, 27].map(id => ({ ...line(id, null), exalt }))) });
  expect(container.querySelector('.exalt')).toBeNull();
  expect(container.querySelector('[aria-label*="exalt bonus"], [title*="exalt bonus"]')).toBeNull();
});

it.each(['verified_inside_base', 'verified_inside_boost'] as const)('splits %s without changing engine cap/pots decisions or observed tones', placement => {
  const lines = [0, 3, 20].map(id => line(id, id < 4 ? 10 : 2, placement));
  lines[2].base = 85; lines[2].at_cap = true;
  const { container } = render(StatPanel, { panel: panel(lines) });
  for (const entry of lines) {
    const boost = placement === 'verified_inside_boost' ? 20 - entry.exalt! : 20;
    const row = container.querySelector(`[data-stat-id="${entry.stat_id}"]`)!;
    expect(row.textContent).toContain(`(+${boost})`);
    expect(row.querySelector('.exalt')?.textContent).toBe(`Exalt +${entry.exalt}`);
    expect(statPotsLeft(entry)).toBe(entry.stat_id === 20 ? 0 : 1);
  }
  expect(screen.getByRole('meter', { name: 'HP' }).getAttribute('aria-valuetext')).toContain('|1');
  expect(container.querySelector('[data-stat-id="20"]')?.getAttribute('data-tone')).toBe('max');
});

it('retains the raw boost with unresolved, missing or contradictory placement evidence', () => {
  const entry = line(20, 2, 'verified_inside_boost');
  expect(statBoost({ ...entry, split_evidence: undefined })).toBe(20);
  expect(statBoost(line(20, 2, 'unresolved'))).toBe(20);
  expect(statBoost({ ...entry, boost: null })).toBeNull();
  expect(statBoost({ ...entry, split_evidence: { ...entry.split_evidence!, reasons: ['negative_other'] } })).toBe(20);
  expect(statBoost({ ...entry, exalt: null })).toBe(20);
  expect(statBoost({ ...entry, exalt: 0 })).toBe(20);
  expect(statExalt({ ...entry, exalt: -1 })).toBeNull();
});

it('keeps the provisional debug note even if only the line reports unresolved placement', () => {
  setDebugMode(true);
  render(StatPanel, { panel: panel([line(20, 2, 'unresolved')]) });
  expect(screen.getByText('Observed boost · provisional split')).toBeTruthy();
});

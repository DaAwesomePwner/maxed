import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { tierClass, bestItem, rangeLabel, shortWhen, clockWhen, whiteRate } from '../../src/lib/loot/loot-format';
import LootHeader from '../../src/lib/loot/LootHeader.svelte';
import RangeSwitch from '../../src/lib/loot/RangeSwitch.svelte';
import { dateRange } from '../../src/lib/runs/filters';
import { presetRange } from '../../src/lib/runs/runs-list';
import { injectChicagoDate } from '../runs/fixtures';
import type { DropItem } from '../../src/lib/bindings/DropItem';

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
it('maps tier classes and ranks items by tier then slot without mutating them', () => {
  expect(['UT', 'ST', 'T12', null, undefined].map(tierClass)).toEqual(['ut', 'st', '', '', '']);
  const tiers = ['T5', 'T12', 'ST', 'UT', 'UT', null];
  const items: DropItem[] = tiers.map((_, slot) => ({ slot, item_type: slot, quantity: 1, enchant: { state: 'missing', slots: null, applied: null, ids: [] }, taken_observed_at: null }));
  const tier = (type: number) => tiers[type];
  expect(bestItem(items, tier)?.slot).toBe(3);
  expect(bestItem(items.slice(0, 3), tier)?.slot).toBe(2);
  expect(bestItem(items.slice(0, 2), tier)?.slot).toBe(1);
  expect(bestItem([items[5], items[0]], tier)?.slot).toBe(0);
  expect(bestItem([items[4], items[3]], tier)?.slot).toBe(3);
  expect(bestItem([], tier)).toBeNull();
});
it('formats uncapped white rates and unknown denominators', () => {
  expect(whiteRate(3, 41)).toBe('7.3%'); expect(whiteRate(0, 12)).toBe('0.0%');
  expect(whiteRate(2, 0)).toBe('—'); expect(whiteRate(1, null)).toBe('—'); expect(whiteRate(3, 2)).toBe('150.0%');
});
it('formats local dates across a month boundary', () => {
  injectChicagoDate();
  const now = new Date('2026-10-02T23:00:00-05:00');
  const at = (date: string) => String(new Date(date).getTime() * 1000);
  expect(rangeLabel(dateRange('2026-09-20', '2026-10-02')!)).toBe('Sep 20 – Oct 2');
  expect(shortWhen(at('2026-10-02T19:38:00-05:00'), now)).toBe('Today');
  expect(shortWhen(at('2026-10-01T21:02:00-05:00'), now)).toBe('Yesterday');
  expect(shortWhen(at('2026-09-29T20:00:00-05:00'), now)).toBe('Sep 29');
  expect(clockWhen(at('2026-10-02T19:38:00-05:00'), now)).toContain('today,');
});
it('marks just the active Loot tab', () => {
  render(LootHeader, { view: 'stats', subline: null });
  expect(screen.getByRole('link', { name: 'Stats' }).getAttribute('aria-current')).toBe('page');
  expect(screen.getByRole('link', { name: 'Feed' }).hasAttribute('aria-current')).toBe(false);
});
it('applies presets with the injected clock', async () => {
  const now = () => new Date(2026, 9, 2), onChange = vi.fn();
  render(RangeSwitch, { range: null, now, onChange });
  await fireEvent.click(screen.getByRole('button', { name: '7 days' }));
  expect(onChange).toHaveBeenCalledWith(presetRange('7d', now()));
});
it('validates custom dates inline and restores focus on Escape', async () => {
  const onChange = vi.fn(); render(RangeSwitch, { range: null, onChange });
  const custom = screen.getByRole('button', { name: 'Custom…' }); await fireEvent.click(custom);
  await fireEvent.input(screen.getByLabelText('From'), { target: { value: '2026-10-02' } });
  await fireEvent.input(screen.getByLabelText('Through'), { target: { value: '2026-10-01' } });
  await fireEvent.submit(screen.getByRole('form', { name: 'Custom date range' }));
  expect(screen.getByRole('alert').textContent).toBe('Invalid Date Range'); expect(onChange).not.toHaveBeenCalled();
  await fireEvent.keyDown(window, { key: 'Escape' }); expect(document.activeElement).toBe(custom);
  expect(screen.queryByRole('form')).toBeNull();
});
it('labels saved custom ranges and applies inclusive Through dates', async () => {
  const range = dateRange('2026-09-20', '2026-10-02')!, onChange = vi.fn();
  render(RangeSwitch, { range, now: () => new Date(2026, 10, 1), onChange });
  const custom = screen.getByRole('button', { name: 'Sep 20 – Oct 2' });
  expect(custom.getAttribute('aria-pressed')).toBe('true'); await fireEvent.click(custom);
  await fireEvent.submit(screen.getByRole('form', { name: 'Custom date range' }));
  expect(onChange).toHaveBeenCalledWith(range);
});

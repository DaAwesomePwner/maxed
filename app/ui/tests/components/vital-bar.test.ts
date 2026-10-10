import { render, screen } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import VitalBar from '../../src/lib/components/VitalBar.svelte';

it('retains one full-width track while two-digit values become four-digit values', async () => {
  const page = render(VitalBar, { kind: 'hp', label: 'HP', text: '12/99', fill: 12, tone: 'plain' });
  const bar = screen.getByRole('meter', { name: 'HP' });
  expect(bar.getAttribute('aria-valuetext')).toBe('12/99');
  await page.rerender({ kind: 'hp', label: 'HP', text: '1250/1385|27 (+310)', fill: 90.25, tone: 'boosted' });
  expect(screen.getByRole('meter', { name: 'HP' })).toBe(bar);
  expect(bar.getAttribute('data-tone')).toBe('boosted');
  expect(bar.querySelector('.value')?.textContent).toBe('1250/1385|27 (+310)');
  expect(bar.getAttribute('aria-valuenow')).toBe('90.25');
});
it.each([[-20, '0'], [120, '100'], [Number.NaN, '0'], [Infinity, '0']])('bounds %s percent', (fill, expected) => {
  render(VitalBar, { kind: 'mp', label: 'MP', text: '—/— (—)', fill: Number(fill), tone: 'plain' });
  expect(screen.getByRole('meter').getAttribute('aria-valuenow')).toBe(expected);
  expect(screen.getByRole('meter').textContent).not.toMatch(/NaN|undefined|Infinity/);
});

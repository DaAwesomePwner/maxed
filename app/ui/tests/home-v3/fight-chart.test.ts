import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it } from 'vitest';
import FightChart from '../../src/lib/home/v3/FightChart.svelte';
import { setDebugMode } from '../../src/lib/debug/debug';
afterEach(() => setDebugMode(false));

it('uses actual durations and closes each area at its own gap', () => {
  const page = render(FightChart, { buckets: [
    { from: '0', until: '2000000', total: 200, local: 40 },
    { from: '3000000', until: '3500000', total: 150, local: null },
  ] });
  expect(screen.getByRole('img', { name: 'Party and your DPS over the fight' })).toBeTruthy();
  expect(page.container.querySelector('path.party')?.getAttribute('d')?.match(/M/g)).toHaveLength(2);
  expect(page.container.querySelector('path.area')?.getAttribute('d')?.match(/Z/g)).toHaveLength(2);
  expect(page.container.querySelector('path.local')?.getAttribute('d')?.match(/M/g)).toHaveLength(1);
  expect(page.container.textContent).toContain('300');
  expect(page.container.querySelector('.axis')?.textContent).toContain('0:03');
});
it('no buckets has a stable chart box; diagnostics appear only in debug', async () => {
  const page = render(FightChart, { buckets: [] });
  expect(page.container.querySelector('.fight-chart')).toBeTruthy();
  expect(page.container.textContent).toContain('No DPS');
  expect(page.container.textContent).not.toMatch(/unavailable|observed|sampled/i);
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('no positive-duration buckets');
});

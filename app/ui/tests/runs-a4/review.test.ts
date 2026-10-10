import { setDebugMode } from '../../src/lib/debug/debug';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flush, main, mountRuns, ready, run, story } from '../runs/fixtures';

beforeEach(() => { setDebugMode(true); vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { setDebugMode(false); cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

it.each([[-0.2, '20% faster'], [0, 'unchanged'], [0.2, '20% slower']])('describes comparison direction honestly (%s)', async (change, expected) => {
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ comparison: { previous_clears: 3, mean_micros: 10000000, change_fraction: Number(change) } })) } }); await flush();
  expect(main().getByText(text => text.includes(String(expected)))).toBeTruthy();
  if (change === 0) expect(main().queryByText(/0% faster/)).toBeNull();
});

it('separates multiple outcome evidence entries visibly inside the unchanged tooltip', async () => {
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ run: run({ evidence: [
    { at: '9000000', evidence: 'victory' }, { at: '10000000', evidence: 'observed_exit' },
  ] }) })) } }); await flush();
  const details = main().getByText('Run evidence').closest('details')!;
  expect(details.querySelectorAll('p')[0].textContent).toMatch(/Victory observed/);
  expect(details.querySelectorAll('p')[1].textContent).toMatch(/Exit observed/);

});

it('retains wrapping and reduced-motion CSS constraints without claiming browser layout acceptance', () => {
  const css = readFileSync(resolve(process.cwd(), 'src/lib/styles/motion.css'), 'utf8');
  expect(css).toMatch(/prefers-reduced-motion:\s*reduce/);
  expect(css).toMatch(/animation:\s*none !important/); expect(css).toMatch(/scroll-behavior:\s*auto !important/);
  for (const name of ['Runs', 'RunStory', 'Fight', 'PlayerBreakdown']) {
    const route = readFileSync(resolve(process.cwd(), `src/routes/${name}.svelte`), 'utf8')
      + (name === 'RunStory' ? readFileSync(resolve(process.cwd(), 'src/lib/runs/run-screen.css'), 'utf8') : '');
    expect(route).toContain('min-width: 0'); expect(route).toContain('overflow-wrap: anywhere');
    expect(route).toContain('overflow-anchor: none');
    expect(route).not.toMatch(/behavior:\s*['"]smooth|animation\s*:|requestAnimationFrame/);
  }
});

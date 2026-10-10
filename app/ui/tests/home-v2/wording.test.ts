import { render } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import DungeonTimeline from '../../src/lib/home/v2/DungeonTimeline.svelte';
import RealmHome from '../../src/lib/home/RealmHome.svelte';
import { dungeonTimeline } from '../../src/lib/home/dungeon';
import { fight, homeSnapshot, realmSnapshot } from './fixtures';

afterEach(() => setDebugMode(false));

const caveats = /observed|unavailable|estimated|partial|provisional|coverage|inferred/i;
function expectClean(container: HTMLElement) {
  expect(container.querySelector('[data-debug-note]')).toBeNull();
  expect(container.textContent).not.toMatch(caveats);
  for (const element of container.querySelectorAll('[aria-label], [title], svg title, svg desc')) {
    expect([element.getAttribute('aria-label'), element.getAttribute('title'),
      ['title', 'desc'].includes(element.tagName) ? element.textContent : ''].join(' ')).not.toMatch(caveats);
  }
}

it('keeps timeline caveats out of marker titles and accessible names', async () => {
  const snapshot = homeSnapshot();
  const boss = fight('wording', { complete_capture: false, issues: ['stream_gap'] });
  const page = render(DungeonTimeline, { run: snapshot.run!, at: snapshot.at,
    markers: dungeonTimeline([boss], [], []), partial: true });
  expectClean(page.container);
  expect(page.container.querySelector('[data-marker-id]')).toBeTruthy();
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('Partial timeline.');
  const marker = page.container.querySelector('[data-marker-id]')!;
  expect(marker.getAttribute('aria-label')).not.toMatch(caveats);
  expect(document.getElementById(marker.getAttribute('aria-describedby')!)?.textContent).not.toMatch(caveats);
});

it('keeps Realm recovery, population and visit caveats in debug', async () => {
  const page = render(RealmHome, { snapshot: realmSnapshot(), stale: true });
  expectClean(page.container);
  expect(page.container.querySelector('[role="status"]')?.textContent).toBe('Reconnecting');
  expect(page.container.textContent).toContain('No Loot');
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('Estimated population');
  expect(page.container.textContent).toContain('Capture is recovering; values are frozen.');
  expect(page.container.textContent).toContain('Visit observations unavailable until the next observed entry');
});

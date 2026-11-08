import { cleanup, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import { homeSnapshot } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';

afterEach(cleanup);

it.each(['realm', 'dungeon'] as const)('explicit v3 rollback renders %s and returns to Nexus', async area_kind => {
  const snapshot = homeSnapshot(area_kind === 'realm' ? { area_kind, run: null, map: 'Realm' } : {});
  const page = mountHome(snapshot, undefined, '#/', { homeRedesign: false });
  await flushHomeRender();
  expect(page.container.querySelector('[data-home="v3"]')).toBeTruthy();
  expect(page.container.querySelector(`.${area_kind}-home`)).toBeTruthy();
  expect(page.container.querySelector('[data-home="v4"]')).toBeNull();
  const home = within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Home' });
  home.focus();
  page.transport.publishSnapshot({ ...snapshot, sequence: String(BigInt(snapshot.sequence) + 1n), area_kind: 'nexus', run: null });
  await flushHomeRender();
  expect(page.container.querySelector('.nexus-home')).toBeTruthy();
  expect(document.activeElement).toBe(home);
  expect(page.browser.adapter.hash).toBe('#/');
});

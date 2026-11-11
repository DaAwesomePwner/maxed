import { readFileSync } from 'node:fs';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { alert, flush, mountPhase2 } from './fixtures';
import BannerHost from '../../src/lib/components/BannerHost.svelte';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';

const sprite: SpriteRef = { key: 'synthetic-portal', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 };
beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

it.each(['banner', 'toast'] as const)('%s stays outside the scrolling route flow and keeps its action in the overlay', async presentation => {
  const page = mountPhase2(); await flush();
  const main = screen.getByRole('main');
  const first = main.firstElementChild as HTMLElement;
  const offset = first.offsetTop;
  page.event({ alert_fired: alert({ presentation }) }, '13'); await flush();
  const dismiss = screen.getByRole('button', { name: /dismiss/i });
  const overlay = dismiss.closest('.alert-overlay');
  expect(overlay).not.toBeNull();
  expect(overlay?.parentElement).toBe(main.parentElement);
  expect(main.contains(overlay)).toBe(false);
  expect(main.firstElementChild).toBe(first);
  expect(first.offsetTop).toBe(offset);
  expect(overlay?.classList.contains('toast-overlay')).toBe(presentation === 'toast');
  expect(dismiss.closest('.banner')?.classList.contains('toast')).toBe(presentation === 'toast');
  expect(overlay?.querySelector('a[href="#/alerts"]')?.textContent).toBe('Alerts');
  expect(screen.getByRole('complementary', { name: 'Character' }).contains(overlay)).toBe(false);
  await fireEvent.click(dismiss); await flush();
  expect(main.firstElementChild).toBe(first);
  expect(first.offsetTop).toBe(offset);
  expect(overlay?.classList.contains('toast-overlay')).toBe(presentation === 'toast');
  // jsdom has no layout engine: pin the positioning contract as well as DOM ownership.
  const host = readFileSync('src/lib/components/BannerHost.svelte', 'utf8');
  const shell = readFileSync('src/lib/shell/Shell.svelte', 'utf8');
  expect(host).toMatch(/\.alert-overlay\s*\{[^}]*position:\s*absolute/);
  expect(host).toMatch(/\.alert-overlay\s*\{[^}]*top:\s*0/);
  expect(shell).toMatch(/\.content-pane\s*\{[^}]*position:\s*relative/);
});

it.each(['banner', 'toast'] as const)('shows a known Key popped portal in a %s', async presentation => {
  const page = mountPhase2({ configure: client => {
    client.getDungeonPortalType = vi.fn(async () => 1234);
    client.getObjectVisuals = vi.fn(async () => ({ client_fingerprint: 'a'.repeat(64), objects: [
      { object_type: 1234, name: 'Synthetic portal', sprite, bag: null },
    ] }));
  } }); await flush();
  page.event({ alert_fired: alert({ kind: 'key_popped', presentation,
    details: { type: 'key_popped', dungeon: 'Synthetic dungeon' } }) }, '13'); await flush();
  const banner = screen.getByRole('region', { name: 'Key popped' });
  expect(banner.querySelector('img')?.getAttribute('src')).toContain('/mapObjects.png');
  expect(banner.querySelector('[data-fallback="true"]')).toBeNull();
  expect(banner.querySelector('a')?.textContent).toBe('View');
  expect(banner.querySelector('a')?.getAttribute('href')).toBe('#/alerts');
  expect(page.client.getDungeonPortalType).toHaveBeenCalledWith('Synthetic dungeon');
});

it.each(['unknown', 'missing catalog', 'missing sprite'] as const)('omits the portal slot for %s', async missing => {
  const page = mountPhase2({ configure: client => {
    client.getDungeonPortalType = vi.fn(async () => {
      if (missing === 'missing catalog') throw new Error('No catalog');
      return missing === 'unknown' ? null : 1234;
    });
    client.getObjectVisuals = vi.fn(async () => ({ client_fingerprint: 'a'.repeat(64), objects: [
      { object_type: 1234, name: null, sprite: null, bag: null },
    ] }));
  } }); await flush();
  page.event({ alert_fired: alert({ kind: 'key_popped', presentation: 'toast',
    details: { type: 'key_popped', dungeon: 'Unknown dungeon' } }) }, '13'); await flush();
  const banner = screen.getByRole('region', { name: 'Key popped' });
  expect(banner.querySelector('[role="img"], .portal-icon, .fallback')).toBeNull();
});

it('keeps overflow access in the overlay after the visible alert is gone', async () => {
  const page = mountPhase2(); await flush();
  for (let index = 0; index < 8; index++) page.event({ alert_fired: alert({ id: `overflow-${index}` }) }, String(13 + index));
  await flush();
  const action = screen.getByRole('link', { name: 'More Alerts' });
  expect(action.closest('.alert-overlay')).not.toBeNull();
  for (let index = 0; index < 6; index++) {
    await fireEvent.click(screen.getByRole('button', { name: /dismiss/i })); await flush();
  }
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  expect(screen.getByRole('link', { name: 'More Alerts' })).toBe(action);
});

it('pauses on pointer hover and expires without stealing route focus', async () => {
  const page = mountPhase2(); await flush();
  const main = screen.getByRole('main'); main.focus();
  page.event({ alert_fired: alert() }, '13'); await flush();
  const moment = screen.getByRole('group', { name: 'Alert' });
  await fireEvent.pointerEnter(moment);
  await vi.advanceTimersByTimeAsync(9000);
  expect(screen.getByRole('button', { name: /dismiss/i })).toBeTruthy();
  await fireEvent.pointerLeave(moment);
  await vi.advanceTimersByTimeAsync(8000); await flush();
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  expect(document.activeElement).toBe(main);
});


it('returns focused dismissal to an explicit target', async () => {
  const target = document.createElement('button'); document.body.append(target);
  const dismissed = vi.fn();
  const page = render(BannerHost, { moment: { id: 'focus', kind: 'personal_best', title: 'Synthetic best', context: '' },
    onDismiss: dismissed, dismissFocusTarget: target, showAlertsAction: true });
  const dismiss = screen.getByRole('button', { name: /dismiss/i }); dismiss.focus();
  await fireEvent.click(dismiss);
  expect(document.activeElement).toBe(target); expect(dismissed).toHaveBeenCalledOnce();
  page.unmount(); target.remove();
});

it('keeps the overlay Alerts link focused after dismissing the final alert', async () => {
  const page = mountPhase2(); await flush();
  page.event({ alert_fired: alert() }, '13'); await flush();
  const dismiss = screen.getByRole('button', { name: /dismiss/i }); dismiss.focus();
  await fireEvent.click(dismiss); await flush();
  expect(document.activeElement).toBe(page.container.querySelector('.alert-overlay .alerts-action'));
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
});

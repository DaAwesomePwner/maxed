import { render } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import Home from '../../src/routes/Home.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { fittedSpriteScale } from '../../src/lib/bridge/visuals';
import { createDungeonPortal, type HomeVisuals } from '../../src/lib/home/visuals';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import { deferred, flush, flushHomeRender, homeSnapshot } from './fixtures';

const disposals: (() => void)[] = [];
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.restoreAllMocks(); });
const rect = (width: number, height = width): SpriteRef => ({ key: `portal:${width}x${height}`, sheet: 'mapObjects.png', x: 0, y: 0, width, height });
const portalVisuals = (sprite: SpriteRef | null): HomeVisuals => ({ client_fingerprint: 'a'.repeat(64), phase: 'ready',
  objects: [{ object_type: 900, name: 'Synthetic Portal', sprite, bag: null, item_tier: null }] });

it('fits sprites into the 40px header box at the largest whole-number scale', () => {
  expect([8, 10, 16, 20, 32, 40, 64].map(side => fittedSpriteScale(rect(side), 40))).toEqual([5, 4, 2, 2, 1, 1, 1]);
  expect(fittedSpriteScale(rect(16, 8), 40)).toBe(2);
  expect(fittedSpriteScale(rect(1), 40)).toBe(16);
  expect(fittedSpriteScale(null, 40)).toBe(5);
});

it('renders the mapped portal sprite in the dungeon header at the fitted size', async () => {
  const page = render(DungeonHome, { snapshot: homeSnapshot(), stale: false, visuals: portalVisuals(rect(16)), portalType: 900 });
  await flushHomeRender();
  const icon = page.container.querySelector('.run-title .run-icon [role="img"]') as HTMLElement;
  expect(icon.getAttribute('aria-label')).toBe('Synthetic Portal');
  expect(icon.dataset.fallback).toBe('false');
  expect([icon.style.width, icon.style.height]).toEqual(['40px', '40px']);
  const crop = () => icon.querySelector('.crop') as HTMLElement;
  expect([crop().style.width, crop().style.height]).toEqual(['32px', '32px']);
  expect((icon.querySelector('img') as HTMLImageElement).src).toBe(`http://maxed-sprite.localhost/${'a'.repeat(64)}/mapObjects.png/0-0-16-16.png`);
  expect(icon.querySelector('img')!.style.transform).toContain('scale(2)');
  await page.rerender({ snapshot: homeSnapshot(), stale: false, visuals: portalVisuals(rect(8)), portalType: 900 });
  expect([icon.style.width, icon.style.height]).toEqual(['40px', '40px']);
  expect([crop().style.width, crop().style.height]).toEqual(['40px', '40px']);
});

it('fits a trimmed portal sprite by its opaque bounds, not its padded rectangle', async () => {
  const sprite = { ...rect(16), trim: { x: 4, y: 4, width: 8, height: 8 } };
  const page = render(DungeonHome, { snapshot: homeSnapshot(), stale: false, visuals: portalVisuals(sprite), portalType: 900 });
  await flushHomeRender();
  const icon = page.container.querySelector('.run-title .run-icon [role="img"]') as HTMLElement;
  const crop = icon.querySelector('.crop') as HTMLElement;
  expect([icon.style.width, icon.style.height]).toEqual(['40px', '40px']);
  expect([crop.style.width, crop.style.height]).toEqual(['40px', '40px']);
  expect(icon.querySelector('img')!.style.transform).toBe('translate(-20px, -20px) scale(5)');
});

it.each([
  ['no mapping', null, rect(16)],
  ['a mapping the catalog cannot draw', 900, null],
  ['a mapping missing from the visual page', 901, rect(16)],
] as const)('keeps the placeholder icon for %s', async (_, portalType, sprite) => {
  const page = render(DungeonHome, { snapshot: homeSnapshot(), stale: false, visuals: portalVisuals(sprite), portalType });
  await flushHomeRender();
  const icon = page.container.querySelector('.run-title .run-icon [role="img"]') as HTMLElement;
  expect(icon.getAttribute('aria-label')).toBe(portalType === 900 ? 'Synthetic Portal' : 'Dungeon');
  expect(icon.dataset.fallback).toBe('true');
  expect(icon.querySelector('img')).toBeNull();
});

it('looks up the latest dungeon only, treats failures as unmapped and refreshes on asset changes', async () => {
  const slow = deferred<number | null>(), release = vi.fn();
  let invalidate = () => {};
  const getDungeonPortalType = vi.fn()
    .mockReturnValueOnce(slow.promise)
    .mockResolvedValueOnce(51)
    .mockRejectedValueOnce(new Error('Client assets are not available yet.'))
    .mockResolvedValueOnce(70000)
    .mockResolvedValue(52);
  const states: (number | null)[] = [];
  const loader = createDungeonPortal({ getDungeonPortalType, onVisualsChanged: callback => { invalidate = callback; return release; } }, next => states.push(next));
  disposals.push(loader.dispose);
  loader.update(null); await flush();
  expect(getDungeonPortalType).not.toHaveBeenCalled();
  loader.update('Synthetic Cave'); loader.update('Synthetic Lair');
  slow.resolve(50); await flush();
  expect(getDungeonPortalType.mock.calls).toEqual([['Synthetic Cave'], ['Synthetic Lair']]);
  expect(states.at(-1)).toBe(51);
  loader.update('Synthetic Lair'); await flush();
  expect(getDungeonPortalType).toHaveBeenCalledTimes(2);
  invalidate(); await flush();
  expect(states.at(-1)).toBeNull();
  invalidate(); await flush();
  expect(states.at(-1)).toBeNull();
  invalidate(); await flush();
  expect(states.at(-1)).toBe(52);
  expect(states).not.toContain(50);
  loader.dispose(); expect(release).toHaveBeenCalledOnce();
  invalidate(); await flush();
  expect(getDungeonPortalType).toHaveBeenCalledTimes(5);
});

it('Home requests the current dungeon portal and shows its sprite in the header', async () => {
  const snapshot = homeSnapshot();
  const client = createClient(createFixtureTransport(snapshot, portalVisuals(rect(8))));
  const lookup = vi.spyOn(client, 'getDungeonPortalType').mockResolvedValue(900);
  const visuals = vi.spyOn(client, 'getObjectVisuals');
  const page = render(Home, { snapshot, client }); await flushHomeRender();
  expect(lookup).toHaveBeenCalledWith(snapshot.run!.dungeon);
  expect(visuals.mock.calls.at(-1)![0]).toContain(900);
  const icon = page.container.querySelector('.v4-header [role="img"]') as HTMLElement;
  expect(icon.getAttribute('aria-label')).toBe('Synthetic Portal');
  expect(icon.querySelector('img')).not.toBeNull();
  expect(icon.style.width).toBe('40px');
});

import { cleanup, render } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import SpriteImage from '../../src/lib/components/SpriteImage.svelte';
import ItemTile from '../../src/lib/components/ItemTile.svelte';
import { spriteUrl } from '../../src/lib/bridge/visuals';

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
it('uses stable tile URLs across routes and changes URLs with the fingerprint without retaining atlas Images', () => {
  const image = vi.fn(); vi.stubGlobal('Image', image);
  const sprite = { key: 'synthetic', sheet: 'mapObjects.png', x: 17, y: 21, width: 8, height: 12 };
  for (const fingerprint of ['a'.repeat(64), 'a'.repeat(64), 'b'.repeat(64)]) {
    const url = spriteUrl(fingerprint, sprite)!;
    const page = render(SpriteImage, { sprite, sheetUrl: url, alt: 'Tile' });
    expect(page.container.querySelector('img')?.getAttribute('src')).toBe(`http://maxed-sprite.localhost/${fingerprint}/mapObjects.png/17-21-8-12.png`);
    expect(page.container.querySelector('img')?.style.transform).toBe('translate(0px, 0px) scale(4)');
    page.unmount();
  }
  expect(image).not.toHaveBeenCalled();
});
it('requests individual pip tiles without retaining a strip', () => {
  const image = vi.fn(); vi.stubGlobal('Image', image);
  const page = render(ItemTile, { sprite: null, resolveSpriteUrl: sprite => spriteUrl('c'.repeat(64), sprite), alt: 'Unknown item',
    enchant: { state: 'recorded', slots: 4, applied: 3, ids: [1, 2, 3] } });
  expect(page.container.querySelector('[data-pip="3"] img')?.getAttribute('src')).toBe(`http://maxed-sprite.localhost/${'c'.repeat(64)}/enchantPips.png/32-0-16-16.png`);
  expect(image).not.toHaveBeenCalled();
});

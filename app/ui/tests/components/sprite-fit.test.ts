import { fireEvent, render, screen } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import SpriteImage from '../../src/lib/components/SpriteImage.svelte';
const sprite = { key: 'test', sheet: 'characters.png', x: 8, y: 16, width: 8, height: 12 };
const sheetUrl = `http://maxed-sprite.localhost/${'a'.repeat(64)}/characters.png/8-16-8-12.png`;

it('centers an integer-scaled tile inside fit without changing visible geometry', () => {
  const { container } = render(SpriteImage, { sprite, sheetUrl, alt: 'Knight', fit: 60, scale: 2 });
  const frame = screen.getByRole('img', { name: 'Knight' });
  expect(frame.style.width).toBe('60px'); expect(frame.style.height).toBe('60px');
  const crop = container.querySelector<HTMLElement>('.crop')!;
  expect([crop.style.width, crop.style.height, crop.style.left, crop.style.top]).toEqual(['40px', '60px', '10px', '0px']);
  expect(container.querySelector('img')?.style.transform).toBe('translate(0px, 0px) scale(5)');
});
it('preserves scale-only sizing and does not cap fit at the legacy 16x limit', async () => {
  const page = render(SpriteImage, { sprite, sheetUrl, alt: 'Knight', scale: 2 });
  expect(screen.getByRole('img').style.width).toBe('16px');
  expect(screen.getByRole('img').style.height).toBe('24px');
  await page.rerender({ sprite: { ...sprite, width: 1, height: 1 }, sheetUrl, alt: 'Knight', fit: 32 });
  expect(page.container.querySelector('img')?.style.transform).toContain('scale(32)');
});
it('fits tight opaque bounds at integer scale and preserves scale-only padded art', async () => {
  const padded = { ...sprite, trim: { x: 2, y: 3, width: 4, height: 6 } };
  const page = render(SpriteImage, { sprite: padded, sheetUrl, alt: 'Trimmed', fit: 60 });
  const crop = () => page.container.querySelector<HTMLElement>('.crop')!;
  expect([crop().style.width, crop().style.height, crop().style.left, crop().style.top])
    .toEqual(['40px', '60px', '10px', '0px']);
  expect(page.container.querySelector('img')?.style.transform).toBe('translate(-20px, -30px) scale(10)');
  await page.rerender({ sprite: padded, sheetUrl, alt: 'Trimmed', scale: 2, fit: undefined });
  expect(screen.getByRole('img').style.width).toBe('16px');
  expect(screen.getByRole('img').style.height).toBe('24px');
  expect(page.container.querySelector('img')?.style.transform).toBe('translate(0px, 0px) scale(2)');
});
it.each([null, { x: 7, y: 0, width: 2, height: 2 }, { x: 0, y: 0, width: 0, height: 2 }])
  ('uses the full rectangle for absent or invalid trim %s', trim => {
    const page = render(SpriteImage, { sprite: { ...sprite, trim }, sheetUrl, alt: 'Fallback bounds', fit: 60 });
    expect(page.container.querySelector('img')?.style.transform).toBe('translate(0px, 0px) scale(5)');
  });
it('keeps a labelled fixed fallback for absent, rejected and failed sprites', async () => {
  const page = render(SpriteImage, { sprite: null, sheetUrl: null, alt: 'Unknown class', fit: 60 });
  const frame = screen.getByRole('img', { name: 'Unknown class' });
  expect(frame.dataset.fallback).toBe('true'); expect(frame.style.width).toBe('60px');
  await page.rerender({ sprite, sheetUrl: 'https://remote.invalid/atlas.png', alt: 'Unknown class', fit: 60 });
  expect(page.container.querySelector('img')).toBeNull();
  await page.rerender({ sprite, sheetUrl, alt: 'Unknown class', fit: 60 });
  await fireEvent.error(page.container.querySelector('img')!);
  expect(frame.dataset.fallback).toBe('true'); expect(frame.style.height).toBe('60px');
  await page.rerender({ sprite, sheetUrl: sheetUrl.replace('a'.repeat(64), 'b'.repeat(64)), alt: 'Unknown class', fit: 60 });
  expect(frame.dataset.fallback).toBe('false');
});

it('accepts only the tile dimensions on load, preserving pixel-art styles', async () => {
  const page = render(SpriteImage, { sprite, sheetUrl, alt: 'Tile', scale: 2 });
  const image = page.container.querySelector('img')!;
  expect(image.getAttribute('src')).toBe(sheetUrl);
  Object.defineProperties(image, { naturalWidth: { value: 8 }, naturalHeight: { value: 12 } });
  await fireEvent.load(image);
  expect(screen.getByRole('img').dataset.fallback).toBe('false');
});

it('leaves an empty frame pending load and only shows the glyph after failure', async () => {
  const page = render(SpriteImage, { sprite, sheetUrl, alt: 'Pending tile', fit: 60 });
  const frame = screen.getByRole('img');
  const image = page.container.querySelector('img')!;
  expect(frame.dataset.pending).toBe('true');
  expect(image.style.visibility).toBe('hidden');
  expect(page.container.querySelector('.fallback')).toBeNull();
  Object.defineProperties(image, { naturalWidth: { value: 8 }, naturalHeight: { value: 12 } });
  await fireEvent.load(image);
  expect(frame.dataset.pending).toBe('false');
  expect(image.style.visibility).toBe('visible');
  await page.rerender({ sprite, sheetUrl: sheetUrl.replace('a'.repeat(64), 'b'.repeat(64)), alt: 'Pending tile', fit: 60 });
  expect(frame.dataset.pending).toBe('true');
  expect(page.container.querySelector('.fallback')).toBeNull();
  await fireEvent.error(page.container.querySelector('img')!);
  expect(frame.dataset.pending).toBe('false');
  expect(page.container.querySelector('.fallback')?.textContent).toBe('?');
});

it('ignores a late load event from an unmounted source', async () => {
  const page = render(SpriteImage, { sprite, sheetUrl, alt: 'Tile' });
  const oldImage = page.container.querySelector('img')!;
  Object.defineProperties(oldImage, { naturalWidth: { value: 8 }, naturalHeight: { value: 12 } });
  await page.rerender({ sprite, sheetUrl: sheetUrl.replace('a'.repeat(64), 'b'.repeat(64)), alt: 'Tile' });
  const currentImage = page.container.querySelector('img')!;
  Object.defineProperties(currentImage, { naturalWidth: { value: 8 }, naturalHeight: { value: 12 } });
  await fireEvent.load(currentImage);
  await fireEvent.load(oldImage);
  expect(currentImage.style.visibility).toBe('visible');
  expect(screen.getByRole('img').dataset.pending).toBe('false');
});

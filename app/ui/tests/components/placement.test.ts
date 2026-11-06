import { expect, it } from 'vitest';
import { placeFixed, scrollBounds } from '../../src/lib/components/placement';

const anchor = new DOMRect(100, 100, 40, 40);
const card = { width: 200, height: 60 };
it('the card fits below', () => {
  expect(placeFixed(anchor, card, { start: 0, stop: 500 }, 500)).toEqual({ left: 100, top: 148, above: false });
});
it('the anchor is near the right edge', () => {
  expect(placeFixed(new DOMRect(450, 100, 40, 40), card, { start: 0, stop: 500 }, 500).left).toBe(292);
});
it('overflow at the bottom with room above', () => {
  expect(placeFixed(anchor, card, { start: 0, stop: 500 }, 200)).toEqual({ left: 100, top: 32, above: true });
});
it('neither fits: stays below', () => {
  expect(placeFixed(anchor, { width: 200, height: 150 }, { start: 0, stop: 500 }, 200)).toEqual({ left: 100, top: 148, above: false });
});
it('start greater than anchor.left', () => {
  expect(placeFixed(anchor, card, { start: 120, stop: 500 }, 500).left).toBe(128);
});
it('scroll bounds use the nearest scrolling ancestor', () => {
  const parent = document.createElement('div');
  parent.style.overflowX = 'auto';
  Object.defineProperty(parent, 'clientWidth', { value: 300 });
  parent.getBoundingClientRect = () => new DOMRect(50, 0, 300, 200);
  expect(scrollBounds(parent, 500)).toEqual({ start: 50, stop: 350 });
  expect(scrollBounds(null, 500)).toEqual({ start: 0, stop: 500 });
});

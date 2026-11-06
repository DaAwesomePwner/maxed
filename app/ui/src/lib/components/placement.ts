export function scrollBounds(from: Element | null, viewportWidth: number): { start: number; stop: number } {
  let start = 0, stop = viewportWidth;
  let parent = from;
  while (parent && parent !== document.body && !['auto', 'scroll'].includes(getComputedStyle(parent).overflowX)) parent = parent.parentElement;
  if (parent && parent !== document.body && parent.clientWidth > 0) {
    const edge = parent.getBoundingClientRect().left + parent.clientLeft;
    start = Math.max(start, edge);
    stop = Math.min(stop, edge + parent.clientWidth);
  }
  return { start, stop };
}

export function placeFixed(anchor: DOMRect, card: { width: number; height: number }, bounds: { start: number; stop: number }, viewportHeight: number, gap = 8): { left: number; top: number; above: boolean } {
  const above = anchor.bottom + gap + card.height > viewportHeight - gap && anchor.top >= card.height + gap;
  return {
    left: Math.max(bounds.start + gap, Math.min(anchor.left, bounds.stop - card.width - gap)),
    top: above ? anchor.top - gap - card.height : anchor.bottom + gap,
    above,
  };
}

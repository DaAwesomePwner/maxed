import { itemHoverParams } from '../../src/lib/components/item-hover';
import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import KpiStrip from '../../src/lib/home/v3/KpiStrip.svelte';
import BestBagCard from '../../src/lib/home/v3/BestBagCard.svelte';
import BagList from '../../src/lib/home/v3/BagList.svelte';
import LootTally from '../../src/lib/home/v3/LootTally.svelte';
import { bestDrop } from '../../src/lib/home/v3/loot-rank';
import { retainedBag } from '../runs-a4/loot-fixtures';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { setDebugMode } from '../../src/lib/debug/debug';

afterEach(() => { vi.unstubAllGlobals(); setDebugMode(false); });
const visuals = { ...emptyHomeVisuals(), objects: [
  { object_type: 444, name: 'A very long ST item name that must occupy at most two lines', item_tier: 'ST', sprite: null, bag: null },
] };
const resolveSpriteUrl = () => 'http://maxed-sprite.localhost/' + 'a'.repeat(64) + '/enchantPips.png';
it('keeps keyed KPI cells and the same columns while numbers grow; bounds progress', async () => {
  const cells = [{ key: 'runs', label: 'Runs', value: '12', sub: '10 cleared', progress: 400 },
    { key: 'fame', label: 'Fame', value: '99', tone: 'fame' as const, progress: Number.NaN }];
  const page = render(KpiStrip, { cells });
  const cell = page.container.querySelector('[data-kpi="runs"]');
  const columns = page.container.querySelector('dl')!.getAttribute('style');
  await page.rerender({ cells: cells.map(c => ({ ...c, value: '9,999' })) });
  expect(page.container.querySelector('[data-kpi="runs"]')).toBe(cell);
  expect(page.container.querySelector('dl')!.getAttribute('style')).toBe(columns);
  expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('100');
  expect(Array.from(cell!.children, child => child.tagName)).toEqual(['DT', 'DD']);
  expect(cell!.querySelector('dd .value')?.textContent).toBe('9,999');
  expect(cell!.querySelector('dd .sub')?.textContent).toBe('10 cleared');
  expect(cell!.querySelector('dd [role="progressbar"]')).toBeTruthy();
  expect(page.container.innerHTML).not.toMatch(/NaN|undefined/);
});
it('renders independent good and bad KPI sub-line tones', async () => {
  const cells = [{ key: 'time', label: 'Time', value: '1:00', sub: '-0:05', subTone: 'good' as const }];
  const page = render(KpiStrip, { cells });
  expect(page.container.querySelector('.sub')?.getAttribute('data-sub-tone')).toBe('good');
  await page.rerender({ cells: [{ ...cells[0], sub: '+0:05', subTone: 'bad' }] });
  expect(page.container.querySelector('.sub')?.getAttribute('data-sub-tone')).toBe('bad');
});
it('features an unknown-content bag on every shared-card surface', async () => {
  const drop = retainedBag({ contents_known: false, contents: [], items: [] });
  expect(bestDrop([drop], visuals)).toEqual({ drop, item: null });
  const page = render(BestBagCard, { best: bestDrop([drop], visuals), visuals, resolveSpriteUrl, meta: '8:14 PM' });
  expect(screen.queryByText('No Bags')).toBeNull();
  expect(screen.getByRole('img', { name: `Bag ${drop.bag_type}` })).toBeTruthy();
  expect(page.container.textContent).toContain(`Bag ${drop.bag_type}`);
  expect(page.container.textContent).not.toContain('Contents unknown');
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('Contents unknown');
});
it('glows once in tier colour for a new best bag and respects reduced motion', async () => {
  const animate = vi.fn(() => ({ cancel: vi.fn() }));
  vi.stubGlobal('getComputedStyle', () => ({ color: '#ffb347' }));
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  const first = retainedBag({ id: 'first' });
  const page = render(BestBagCard, { best: bestDrop([first], visuals), visuals, resolveSpriteUrl, meta: '—' });
  const node = page.container.querySelector<HTMLElement>('.best-card')!;
  Object.defineProperty(node, 'animate', { value: animate, configurable: true });
  await tick(); expect(animate).not.toHaveBeenCalled();
  const second = { ...first, id: 'second' };
  await page.rerender({ best: bestDrop([second], visuals) });
  expect(animate).toHaveBeenCalledExactlyOnceWith([
    { boxShadow: '0 0 22px 4px #ffb347' }, { boxShadow: '0 0 0 0 transparent' },
  ], { duration: 600, iterations: 1 });
  await page.rerender({ best: bestDrop([second], visuals), meta: 'updated clock' });
  expect(animate).toHaveBeenCalledOnce();
  vi.stubGlobal('matchMedia', () => ({ matches: true }));
  await page.rerender({ best: bestDrop([{ ...first, id: 'third' }], visuals) });
  expect(animate).toHaveBeenCalledOnce();
});
it('does not glow when a scope first loads or is switched, only on a new best within it', async () => {
  const animate = vi.fn(() => ({ cancel: vi.fn() }));
  vi.stubGlobal('getComputedStyle', () => ({ color: '#ffb347' }));
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  Object.defineProperty(HTMLElement.prototype, 'animate', { value: animate, configurable: true });
  try {
    const page = render(BestBagCard, { best: null, visuals, resolveSpriteUrl, meta: '—', scope: null });
    await page.rerender({ best: bestDrop([retainedBag({ id: 'loaded' })], visuals), scope: 'session' });
    await page.rerender({ best: bestDrop([retainedBag({ id: 'today' })], visuals), scope: 'today' });
    await page.rerender({ best: null, scope: null });
    await page.rerender({ best: bestDrop([retainedBag({ id: 'reloaded' })], visuals), scope: 'today' });
    expect(animate).not.toHaveBeenCalled();
    await page.rerender({ best: bestDrop([retainedBag({ id: 'new-in-today' })], visuals), scope: 'today' });
    expect(animate).toHaveBeenCalledOnce();
    // The first bag of a loaded scope is new too, although the card had nothing to show.
    await page.rerender({ best: null, scope: 'run' });
    await page.rerender({ best: bestDrop([retainedBag({ id: 'first-in-run' })], visuals), scope: 'run' });
    expect(animate).toHaveBeenCalledTimes(2);
  } finally { Reflect.deleteProperty(HTMLElement.prototype, 'animate'); }
});
it('features ST with tier styling, 16px pip, rarity, count and debug-only evidence', async () => {
  const drop = retainedBag({ source_estimated: true, issues: ['stream_gap'] });
  drop.contents = drop.contents.slice(0, 1);
  drop.items = [444];
  drop.contents[0].enchant = { state: 'recorded', applied: 4, slots: 4, ids: [1, 2, 3, 4] };
  const page = render(BestBagCard, { best: bestDrop([drop], visuals), visuals, resolveSpriteUrl, meta: 'Nexus · 8:14 PM' });
  expect(page.container.querySelector('.best-card')?.getAttribute('data-tier')).toBe('ST');
  const tile = page.container.querySelector('.item-box')!;
  expect(tile.getAttribute('data-item-hover')).toBe('444');
  expect(tile.hasAttribute('title')).toBe(false);
  expect(itemHoverParams(tile)).toEqual({ type: 444, enchant: drop.contents[0].enchant, name: visuals.objects[0].name, tier: 'ST' });
  expect(screen.getByText('Divine')).toBeTruthy();
  expect(screen.getByText('4 enchants')).toBeTruthy();
  expect(page.container.querySelector<HTMLElement>('[data-pip="4"]')?.style.width).toBe('16px');
  expect(page.container.querySelector('.name')?.classList.contains('clamp')).toBe(true);
  expect(page.container.textContent).not.toContain('Some traffic was missed');
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('Some traffic was missed');
  expect(page.container.textContent).toContain('Observed contents, not pickup or ownership.');
  // Accepted revisions replace DTOs; keep the previous published bag intact.
  const revised = { ...drop, contents: drop.contents.map(item => ({ ...item,
    enchant: { state: 'missing' as const, applied: null, slots: null, ids: [] } })) };
  await page.rerender({ best: bestDrop([revised], visuals), visuals, resolveSpriteUrl, meta: '—' });
  expect(page.container.querySelector('[data-pip]')).toBeNull();
  expect(page.container.textContent).not.toMatch(/Common|enchants/);
  await page.rerender({ best: null, visuals, resolveSpriteUrl, meta: '—' });
  expect(screen.getByText('No Bags')).toBeTruthy();
});
it('fills fixed rows, reserves overflow space, groups hidden bags and disconnects observer', async () => {
  let resize!: ResizeObserverCallback;
  const disconnect = vi.fn();
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe() {} disconnect = disconnect;
  });
  const drop = retainedBag({ issues: ['stream_gap'] });
  drop.contents[0].enchant = { state: 'recorded', applied: 2, slots: 2, ids: [1, 2] };
  drop.contents.push(...[2, 3, 4].map(slot => ({ ...drop.contents[0], slot })));
  const drops = Array.from({ length: 8 }, (_, i) => ({ ...drop, id: `bag-${i}`, bag_type: i < 3 ? 1280 : 1286, bag_rank: i < 3 ? 10 : 0 }));
  const page = render(BagList, { drops, visuals, resolveSpriteUrl, clock: () => '8:14 PM' });
  const sized = (height: number) => resize([{ contentRect: { height } } as ResizeObserverEntry], {} as ResizeObserver);
  sized(114); await tick();
  const tile = page.container.querySelector('.item')!;
  expect(tile.getAttribute('data-item-hover')).toBe('444');
  expect(tile.hasAttribute('title')).toBe(false);
  expect(itemHoverParams(tile)).toEqual({ type: 444, enchant: drop.contents[0].enchant, name: visuals.objects[0].name, tier: 'ST' });

  expect(page.container.querySelectorAll('[data-drop-id]')).toHaveLength(2);
  expect(screen.getByText('+6 more')).toBeTruthy();
  const clippedRows = page.container.querySelector('.bag-rows');
  const overflowTrigger = screen.getByRole('button', { name: /\+6 more/ });
  expect(clippedRows?.querySelectorAll('[data-drop-id]')).toHaveLength(2);
  expect(clippedRows!.contains(overflowTrigger)).toBe(false);
  expect(clippedRows!.contains(page.container.querySelector('[role="tooltip"]'))).toBe(false);
  expect(page.container.querySelectorAll('[data-overflow-bag]')).toHaveLength(2);
  expect(page.container.querySelector('[data-overflow-bag="1280"]')?.textContent).toContain('1');
  expect(page.container.querySelector('[data-overflow-bag="1286"]')?.textContent).toContain('5');
  expect(page.container.querySelector('[data-drop-id]')?.querySelectorAll('.item')).toHaveLength(4);
  expect(page.container.querySelector<HTMLElement>('[data-pip="2"]')?.style.width).toBe('12px');
  setDebugMode(true); await tick();
  expect(page.container.querySelectorAll('[data-drop-id]')).toHaveLength(2);
  expect(page.container.querySelector('.bag-list')?.querySelector('[data-debug-note]')).toBeNull();
  expect(page.container.querySelector('.bag-list-notes')?.textContent).toContain('Some traffic was missed');
  expect(page.container.querySelector('.bag-list-root')?.children).toHaveLength(2);
  sized(400); await tick();
  expect(page.container.querySelectorAll('[data-drop-id]')).toHaveLength(8);
  expect(screen.queryByText('+6 more')).toBeNull();
  page.unmount(); expect(disconnect).toHaveBeenCalledOnce();
});
it('renders explicit zero tallies and labelled missing art without fabricated values', () => {
  const tally = render(LootTally, { bags: 0, ut: 0, st: 0, items: 0 });
  expect(tally.container.querySelectorAll('dd')).toHaveLength(4);
  expect(tally.container.textContent).toContain('Items');
  const drop = retainedBag();
  const page = render(BestBagCard, { best: bestDrop([drop], { ...emptyHomeVisuals(), phase: 'ready' }), visuals: { ...emptyHomeVisuals(), phase: 'ready' }, resolveSpriteUrl: () => null, meta: '—' });
  expect(screen.getByRole('img', { name: 'Item 444' })).toBeTruthy();
  expect(page.container.innerHTML).not.toMatch(/NaN|undefined/);
});

import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ItemTile from '../../src/lib/components/ItemTile.svelte';
import HeroCard from '../../src/lib/shell/HeroCard.svelte';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import LootContents from '../../src/lib/home/LootContents.svelte';
import BagList from '../../src/lib/home/v3/BagList.svelte';
import type { EnchantView } from '../../src/lib/bindings/EnchantView';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import { railView } from '../../src/lib/shell/rail';
import { enchantCount, pipSize, pipSprite, rarityName } from '../../src/lib/styles/enchant';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { homeSnapshot, homeSummary } from '../home/fixtures';
import { retainedBag } from '../runs-a4/loot-fixtures';
import fixture from '../fixtures/t0-live';

afterEach(() => cleanup());

const enchant = (applied: number): EnchantView => applied === 0
  ? { state: 'recorded_empty', slots: 0, applied: 0, ids: [] }
  : { state: 'recorded', slots: applied, applied, ids: Array.from({ length: applied }, (_, i) => 10 + i) };
const item: SpriteRef = { key: 'lofiObj:1', sheet: 'characters.png', x: 8, y: 0, width: 8, height: 8 };
const resolve = (sprite: SpriteRef) => `/${sprite.sheet}`;
const names = ['Uncommon', 'Rare', 'Legendary', 'Divine'];

describe('enchant count to pip mapping', () => {
  it('counts recorded enchants and keeps missing or invalid evidence unknown', () => {
    expect(enchantCount(null)).toBeNull();
    expect(enchantCount({ state: 'missing', slots: null, applied: null, ids: [] })).toBeNull();
    expect(enchantCount({ state: 'invalid', slots: null, applied: null, ids: [] })).toBeNull();
    expect(enchantCount({ state: 'recorded', slots: 5, applied: 5, ids: [] })).toBeNull();
    for (const n of [0, 1, 2, 3, 4]) expect(enchantCount(enchant(n))).toBe(n);
  });
  it('names rarity Common through Divine and maps 1-4 to RarityIcon_1-4', () => {
    expect([0, 1, 2, 3, 4].map(rarityName)).toEqual(['Common', ...names]);
    expect(rarityName(null)).toBeNull();
    expect(pipSprite(0)).toBeNull();
    expect(pipSprite(null)).toBeNull();
    expect(pipSprite(5)).toBeNull();
    expect([1, 2, 3, 4].map(n => pipSprite(n))).toEqual([1, 2, 3, 4].map(n => ({
      key: `enchantPip:${n}`, sheet: 'enchantPips.png', x: (n - 1) * 16, y: 0, width: 16, height: 16,
    })));
  });
  it('sizes pips to the tile: 16px from 40px, 12px on small tiles', () => {
    expect([56, 40, 39, 30, 24, 16].map(pipSize)).toEqual([16, 16, 12, 12, 12, 8]);
  });
});

describe('ItemTile', () => {
  it.each([
    ['no enchant evidence', null],
    ['missing evidence', { state: 'missing', slots: null, applied: null, ids: [] } as EnchantView],
    ['zero enchants', enchant(0)],
  ])('shows no pip for %s', (_, value) => {
    const { container } = render(ItemTile, { sprite: item, resolveSpriteUrl: resolve, alt: 'Staff', enchant: value });
    expect(container.querySelector('[data-pip]')).toBeNull();
    expect(screen.getByRole('img', { name: 'Staff' })).toBeTruthy();
  });
  it.each([1, 2, 3, 4])('shows the game pip for %i enchants at the bottom-right', (n) => {
    const { container } = render(ItemTile, { sprite: item, resolveSpriteUrl: resolve, alt: 'Staff', scale: 5, enchant: enchant(n) });
    const pip = screen.getByRole('img', { name: `${names[n - 1]}, ${n} ${n === 1 ? 'enchant' : 'enchants'}` });
    expect(pip.dataset.pip).toBe(String(n));
    // 40px tile: the 16px art at 1x, individual RarityIcon_n tile.
    expect(pip.style.width).toBe('16px');
    const art = pip.querySelector('img')!;
    expect(art.getAttribute('src')).toBe(`${window.location.origin}/enchantPips.png`);
    expect(art.style.transform).toBe('scale(1)');
    expect(container.querySelectorAll('[data-pip]')).toHaveLength(1);
  });
  it('scales the pip down on small tiles', () => {
    render(ItemTile, { sprite: item, resolveSpriteUrl: resolve, alt: 'Ring', scale: 3, enchant: enchant(3) });
    const pip = screen.getByRole('img', { name: 'Legendary, 3 enchants' });
    expect(pip.style.width).toBe('12px');
    expect(pip.querySelector('img')!.style.transform).toBe('scale(0.75)');
  });
  it('falls back to rarity gems without pip art or when it fails to load', async () => {
    render(ItemTile, { sprite: item, alt: 'Armor', enchant: enchant(2) });
    const missing = screen.getByRole('img', { name: 'Rare, 2 enchants' });
    expect(missing.querySelector('img')).toBeNull();
    expect(missing.querySelectorAll('.gems i')).toHaveLength(2);
    cleanup();
    render(ItemTile, { sprite: item, resolveSpriteUrl: resolve, alt: 'Armor', enchant: enchant(4) });
    const pip = screen.getByRole('img', { name: 'Divine, 4 enchants' });
    await fireEvent.error(pip.querySelector('img')!);
    expect(pip.querySelector('img')).toBeNull();
    expect(pip.querySelectorAll('.gems i')).toHaveLength(4);
  });
  it('rejects incorrect pip tile dimensions', async () => {
    render(ItemTile, { sprite: item, resolveSpriteUrl: resolve, alt: 'Armor', enchant: enchant(4) });
    const art = screen.getByRole('img', { name: 'Divine, 4 enchants' }).querySelector('img')!;
    Object.defineProperties(art, { naturalWidth: { value: 32 }, naturalHeight: { value: 16 } });
    await fireEvent.load(art);
    expect(screen.getByRole('img', { name: 'Divine, 4 enchants' }).querySelectorAll('.gems i')).toHaveLength(4);
  });
});

describe('enchant pips on item tiles', () => {
  it('marks each rail equipment slot with its enchant count', () => {
    const snapshot: LiveSnapshot = structuredClone(fixture);
    snapshot.character!.equipment = [0, 1, 2, 3].map(slot => ({ slot, item_type: 200 + slot, enchant: enchant([3, 0, 2, 1][slot]) }));
    const { container } = render(HeroCard, { view: railView(snapshot), resolveSpriteUrl: resolve });
    const pips = [...container.querySelectorAll('[data-slot]')].map(slot => slot.querySelector<HTMLElement>('[data-pip]')?.dataset.pip ?? null);
    expect(pips).toEqual(['3', null, '2', '1']);
  });
  it.each(['contents', 'story'])('marks loot bag items in %s with their enchant count', async (surface) => {
    const drop = retainedBag({ contents: [0, 1, 2, 3, 4].map(n => ({ slot: n, item_type: 600 + n, quantity: 1, enchant: enchant(n), taken_observed_at: null })) });
    const visuals = { ...emptyHomeVisuals(), phase: 'ready' as const, client_fingerprint: 'a'.repeat(64) };
    const { container } = surface === 'contents' ? render(LootContents, { drop, visuals })
      : render(BagList, { drops: [drop], visuals, scroll: true, clock: () => '—', resolveSpriteUrl: () => null });
    for (const summary of container.querySelectorAll('details > summary')) await fireEvent.click(summary);
    const counts = (root: ParentNode) => [...root.querySelectorAll<HTMLElement>('[data-pip]')].map(pip => pip.dataset.pip).sort();
    expect(counts(container)).toEqual(surface === 'contents' ? ['1', '2', '3', '4'] : ['1', '2', '3']);
    if (surface === 'story') expect(screen.getByText('+1')).toBeTruthy();
  });
  it('shows the best bag item pip and its rarity name', () => {
    const drop = retainedBag({ contents: [{ slot: 0, item_type: 444, quantity: 1, enchant: enchant(4), taken_observed_at: null }] });
    render(NexusHome, { snapshot: homeSnapshot(), choice: 'session', retry: vi.fn(),
      visuals: { ...emptyHomeVisuals(), phase: 'ready', client_fingerprint: 'a'.repeat(64) },
      history: { phase: 'ready', summary: homeSummary({ best_drop: drop, recent_runs: [], recent_details: [] }), message: '', retryAvailable: true } });
    const card = screen.getByRole('heading', { name: 'Best bag' }).closest('section')!;
    const row = card.querySelector('.best-card')!;
    expect(row.querySelector<HTMLElement>('[data-pip]')?.dataset.pip).toBe('4');
    expect(row.querySelector('.rarity')?.textContent).toBe('Divine');
  });
  it('keeps an unenchanted best item free of a rarity tag and pip', () => {
    render(NexusHome, { snapshot: homeSnapshot(), choice: 'session', retry: vi.fn(),
      history: { phase: 'ready', summary: homeSummary({ best_drop: retainedBag(), recent_runs: [], recent_details: [] }), message: '', retryAvailable: true } });
    const row = screen.getByRole('heading', { name: 'Best bag' }).closest('section')!.querySelector('.best-card')!;
    expect(row.querySelector('[data-pip]')).toBeNull();
    expect(row.querySelector('.rarity')).toBeNull();
  });
});

it.each([[8, 8], [32, 12], [40, 16], [256, 16]])
  ('fits equipment to %spx and sizes its pip to %spx', (fit, pip) => {
    const { container } = render(ItemTile, { sprite: { ...item, height: 12 }, resolveSpriteUrl: resolve,
      alt: 'Fitted sword', fit, enchant: enchant(4) });
    const frame = screen.getByRole('img', { name: 'Fitted sword' });
    expect(frame.style.width).toBe(`${fit}px`); expect(frame.style.height).toBe(`${fit}px`);
    expect(screen.getByRole('img', { name: 'Divine, 4 enchants' }).style.width).toBe(`${pip}px`);
    expect(container.querySelector('[data-pip="4"]')).toBeTruthy();
  });
it.each([NaN, Infinity, -Infinity, -1, 0, 7, 32.5, 257])('ignores invalid fit %s everywhere', fit => {
  render(ItemTile, { sprite: item, resolveSpriteUrl: resolve, alt: 'Legacy sword', fit, scale: 2, enchant: enchant(4) });
  expect(screen.getByRole('img', { name: 'Legacy sword' }).style.width).toBe('16px');
  expect(screen.getByRole('img', { name: 'Legacy sword' }).style.height).toBe('16px');
  expect(screen.getByRole('img', { name: 'Divine, 4 enchants' }).style.width).toBe('8px');
});

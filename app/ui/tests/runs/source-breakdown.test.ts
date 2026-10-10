import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import { tick } from 'svelte';
import { setDebugMode } from '../../src/lib/debug/debug';
import type { EnchantView } from '../../src/lib/bindings/EnchantView';
import SourceBreakdown from '../../src/lib/runs/SourceBreakdown.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { player, source } from './fixtures';

afterEach(() => { cleanup(); setDebugMode(false); });

function renderSource(enchant: EnchantView, tier = 'UT') {
  render(SourceBreakdown, { player: player({ sources: [source({ enchant })] }),
    visuals: { ...emptyHomeVisuals(), objects: [
      { object_type: 1000, name: 'Synthetic source', item_tier: tier, sprite: null, bag: null },
    ] } });
  return screen.getByRole('rowheader', { name: /Synthetic source/ });
}

it.each([[1, 'Uncommon'], [2, 'Rare'], [3, 'Legendary'], [4, 'Divine']] as const)
('shows tier, pips and %s known enchants with their rarity', (count, rarity) => {
  const row = renderSource({ state: 'recorded', slots: count, applied: count, ids: Array.from({ length: count }, (_, i) => i + 1) });
  const scope = within(row), noun = count === 1 ? 'enchant' : 'enchants';
  expect(scope.getByText('UT')).toBeTruthy();
  expect(scope.getByText(`${rarity} · ${count} ${noun}`)).toBeTruthy();
  expect(scope.getByRole('img', { name: `${rarity}, ${count} ${noun}` }).getAttribute('data-pip')).toBe(String(count));
  const subline = scope.getByText('UT').parentElement!;
  expect(subline.textContent).toContain('•'.repeat(count));
  expect(subline.textContent).not.toContain('—');
});

it('shows ST no enchants only when zero enchants were recorded', () => {
  const row = renderSource({ state: 'recorded_empty', slots: 0, applied: 0, ids: [] }, 'ST');
  expect(within(row).getByText('ST').parentElement?.textContent?.replace(/\s+/g, ' ').trim()).toBe('ST no enchants');
  expect(row.querySelector('[data-pip]')).toBeNull();
});

it.each(['missing', 'invalid'] as const)('shows only the tier for %s enchant evidence', state => {
  const row = renderSource({ state, slots: null, applied: null, ids: [] });
  expect(within(row).getByText('UT').parentElement?.textContent?.trim()).toBe('UT');
  expect(within(row).queryByText('no enchants')).toBeNull();
  expect(row.querySelector('[data-pip]')).toBeNull();
});

it.each([0, 1, 2])('pluralizes the summary for %s sources', count => {
  render(SourceBreakdown, { player: player({ sources: Array.from({ length: count }, () => source()) }) });
  expect(screen.getByText(`${count} ${count === 1 ? 'source' : 'sources'} · 321 total`)).toBeTruthy();
});

it('labels debug evidence with the same names as the source rows', async () => {
  setDebugMode(true);
  const { container } = render(SourceBreakdown, { player: player({ sources: [
    source({ reasons: ['Synthetic reconstruction reason'] }),
    source({ item_type: 2000 }),
    source({ kind: 'unknown', item_type: null }),
  ] }), visuals: { ...emptyHomeVisuals(), objects: [
    { object_type: 1000, name: 'Synthetic blade', item_tier: 'UT', sprite: null, bag: null },
  ] } });
  for (const name of ['Synthetic blade', 'Item 2000', 'Unknown Damage']) {
    expect(screen.getByRole('rowheader', { name: new RegExp(name) })).toBeTruthy();
    expect(screen.getByText(`${name}: Enchant evidence: recorded_empty. Slots: 1 · Applied: 0 · IDs: —`)).toBeTruthy();
  }
  expect(screen.getByText('Synthetic blade: Synthetic reconstruction reason')).toBeTruthy();
  setDebugMode(false); await tick();
  expect(container.querySelector('[data-debug-note]')).toBeNull();
  expect(screen.queryByText('Source coverage')).toBeNull();
});

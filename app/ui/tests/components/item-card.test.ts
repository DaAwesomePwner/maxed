import { expect, it } from 'vitest';
import { itemCardModel } from '../../src/lib/components/item-card';
import type { EnchantView } from '../../src/lib/bindings/EnchantView';
import { lookup } from './fixtures/item-lookup';

const recorded = (ids = [1], applied = ids.length): EnchantView => ({ state: 'recorded', slots: applied, applied, ids });
it('catalog item has no section', () => {
  expect(itemCardModel({ type: 10 }, lookup, false)).toMatchObject({ section: { kind: 'none' }, meta: 'UT Staff', rarity: null });
});
it('recorded_empty is two lines', () => {
  expect(itemCardModel({ type: 10, enchant: { state: 'recorded_empty', slots: 0, applied: 0, ids: [] } }, lookup, false)).toMatchObject({ section: { kind: 'none' }, rarity: null });
});
it('recorded lists rows', () => {
  expect(itemCardModel({ type: 10, enchant: recorded() }, lookup, false)).toMatchObject({ meta: 'UT Staff', rarity: { name: 'Uncommon', level: 1 }, section: { kind: 'rows', rows: [{ id: 1, name: 'Keen Edge', description: 'Adds damage', unknown: false }] } });
});
it('unknown enchant stays visible', () => {
  expect(itemCardModel({ type: 10, enchant: recorded([2]) }, lookup, false).section).toMatchObject({ kind: 'rows', rows: [{ id: 2, name: 'Enchant #2', description: null, unknown: true }] });
});
it('pending lookup shows id rows without the unknown tag', () => {
  expect(itemCardModel({ type: 10, name: 'Pending Staff', tier: 'T12', enchant: recorded([2]) }, null, false)).toMatchObject({ name: 'Pending Staff', meta: 'T12', section: { kind: 'rows', rows: [{ id: 2, name: 'Enchant #2', description: null, unknown: false }] } });
});
it('recorded without ids shows no section', () => {
  expect(itemCardModel({ type: 10, enchant: recorded([], 2) }, lookup, false)).toMatchObject({ section: { kind: 'none' }, rarity: { name: 'Rare', level: 2 } });
});
it.each(['missing', null, 'invalid'] as const)('%s enchant shows its evidence message', state => {
  const enchant = state === null ? null : { state, slots: null, applied: null, ids: [] };
  expect(itemCardModel({ type: 10, enchant }, lookup, false).section).toEqual({ kind: 'message', text: state === 'invalid' ? 'Enchant data unreadable' : 'Enchants not observed' });
});
it('failed lookup falls back', () => {
  expect(itemCardModel({ type: 10, name: 'Fallback', tier: 'T12' }, null, true)).toMatchObject({ name: 'Fallback', meta: 'T12', section: { kind: 'message', text: 'Details unavailable' } });
});
it('slot fallbacks', () => {
  expect(itemCardModel({ type: 10 }, { ...lookup, item: { ...lookup.item, slot_label: null, slot_type: 40 } }, false).meta).toBe('UT Slot 40');
  expect(itemCardModel({ type: 10 }, { ...lookup, item: { ...lookup.item, slot_label: null, slot_type: null } }, false).meta).toBe('UT');
});
it('suffix, label, note', () => {
  expect(itemCardModel({ type: 10, enchant: recorded(), suffix: '×3', label: 'Best copy', note: 'Removal observed' }, lookup, false)).toMatchObject({ base: 'UT Staff', meta: 'UT Staff · ×3', section: { kind: 'rows', label: 'Best copy' }, note: 'Removal observed' });
});
it('name priority', () => {
  expect(itemCardModel({ type: 10, name: 'Fallback' }, lookup, false).name).toBe('Synthetic Staff');
  expect(itemCardModel({ type: 10, name: 'Fallback' }, { ...lookup, item: { ...lookup.item, name: null } }, false).name).toBe('Fallback');
  expect(itemCardModel({ type: 10 }, null, false).name).toBe('Item 10');
});

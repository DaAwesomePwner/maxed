import type { DropView } from '../../src/lib/bindings/DropView';
import { homeSnapshot } from '../home/fixtures';

/** Invented retained appearances, including two different items in one reused slot. */
export function retainedBag(overrides: Partial<DropView> = {}): DropView {
  const enchant = { state: 'recorded_empty' as const, slots: 1, applied: 0, ids: [] };
  return { ...homeSnapshot().drops[0], id: 'retained-bag', contents_known: true, issues: [], items: [444, 555, 444],
    contents: [
      { slot: 0, item_type: 444, quantity: 1, enchant, taken_observed_at: '2000000' },
      { slot: 0, item_type: 555, quantity: 2, enchant, taken_observed_at: '3000000' },
      { slot: 1, item_type: 444, quantity: 1, enchant, taken_observed_at: null },
    ], ...overrides };
}

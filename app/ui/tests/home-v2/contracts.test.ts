import { expect, expectTypeOf, it } from 'vitest';
import type { ObjectVisual } from '../../src/lib/bindings/ObjectVisual';
import type { DamageChartProps, HomeLiveWindow, HomeLootProps, HomeScreen, LootView, LootViewProjector, ObservedLootItem } from '../../src/lib/home/v2/contracts';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';

it('freezes additive native and shared renderer declarations without implementation imports', () => {
  expectTypeOf<HomeScreen>().toEqualTypeOf<'dungeon' | 'realm' | 'nexus'>();
  expectTypeOf<LootViewProjector>().toEqualTypeOf<(input: HomeLootProps) => LootView>();
  expectTypeOf<ObservedLootItem['tier']>().toEqualTypeOf<string | null>();
  expectTypeOf<HomeLiveWindow['visitStarted']>().toEqualTypeOf<string | null>();
  const oldVisual: ObjectVisual = { object_type: 1, name: null, sprite: null, bag: null };
  const newVisual: ObjectVisual = { ...oldVisual, item_tier: null };
  const loot: HomeLootProps = { drops: [], visuals: emptyHomeVisuals(), partial: true, reasons: ['Unavailable'], title: 'Observed loot' };
  const chart: DamageChartProps = { buckets: [], playerSeries: null, selectedPlayerId: null };
  expect(newVisual.item_tier).toBeNull();
  expect(loot.partial).toBe(true);
  expect(chart.playerSeries).toBeNull();
  expect(emptyHomeLiveWindow()).toMatchObject({ visitKey: null, partial: true, fights: [], drops: [], events: [] });
});

import type { LootFixtureQueries } from '../../../src/lib/bridge/fixture';
import type { LootDropRow } from '../../../src/lib/bindings/LootDropRow';
import type { DropItem } from '../../../src/lib/bindings/DropItem';
import type { DropCursor } from '../../../src/lib/bindings/DropCursor';
import type { DropPage } from '../../../src/lib/bindings/DropPage';
import type { TimeRange } from '../../../src/lib/bindings/TimeRange';
import type { LootSettings } from '../../../src/lib/bindings/LootSettings';
import type { VisualPage } from '../../../src/lib/bindings/VisualPage';

export const lootDungeons = ['Abbey', 'Burrow', 'Citadel', 'Delta', 'Estuary', 'Foundry'].map(name => `Invented ${name}`);
const base = BigInt(Date.UTC(2026, 8, 1)) * 1000n;
const day = 86_400_000_000n;
const at = (index: number, offset = 0n) => String(base + BigInt(index) * day + offset);
// Five bags per run, including six open runs; eight additional bags have no run.
export const lootRuns = Array.from({ length: 24 }, (_, index) => ({
  id: `invented-run-${index}`, dungeon: lootDungeons[index % 6], started: at(index),
  ended: index < 18 ? at(index, 3_600_000_000n) : null, cleared: index % 3 !== 0,
}));
const ranks = [null, 1, 3, 255];
const states = ['missing', 'invalid', 'recorded_empty', 'recorded'] as const;
const tier = (type: number) => ['UT', 'ST', 'T5'][type % 3];
function contents(index: number, timestamp: string): DropItem[] {
  if (index % 11 === 0 || index % 13 === 0) return [];
  return Array.from({ length: 3 }, (_, slot) => {
    const state = states[(index + slot) % 4];
    return {
      slot, item_type: index === 119 && slot === 2 ? 65000 : slot === 0 ? 100 : 100 + (index + slot) % 16,
      quantity: 1, enchant: { state, slots: state.startsWith('recorded') ? 2 : null,
        applied: state === 'recorded' ? 1 : state === 'recorded_empty' ? 0 : null, ids: state === 'recorded' ? [700 + index % 3] : [] },
      taken_observed_at: index % 9 === 0 && slot === 0 ? String(BigInt(timestamp) + 1n) : null,
    };
  });
}
export const lootBags: LootDropRow[] = Array.from({ length: 128 }, (_, index): LootDropRow => {
  const run = index < 120 ? lootRuns[Math.floor(index / 5)] : null;
  // Equal timestamps deliberately exercise the cursor's id tie-breaker.
  const timestamp = at(run ? Math.floor(index / 5) : 24, BigInt(Math.floor(index % 5 / 2) + 1) * 1_000_000n);
  const items = contents(index, timestamp);
  return { dungeon: run?.dungeon ?? null, drop: {
    id: `invented-bag-${String(index).padStart(3, '0')}`, session_id: 'invented-session', run_id: run?.id ?? null,
    at: timestamp, bag_type: 800 + index % 4, bag_rank: ranks[index % 4], items: items.map(item => item.item_type),
    contents: items, contents_known: index % 13 !== 0 && index % 7 !== 0, enchant: null,
    source_enemy: index % 10 === 0 ? null : 900 + index % 6, source_estimated: index % 10 !== 0 && index % 3 === 0,
    source_reason: index % 10 !== 0 && index % 3 === 0 ? 'Invented nearby encounter' : null,
    issues: index % 7 === 0 ? ['stream_gap'] : [],
  } };
}).sort((a, b) => Number(BigInt(b.drop.at) - BigInt(a.drop.at)) || b.drop.id.localeCompare(a.drop.id));

export const lootSettings: LootSettings = { chase: Array.from({ length: 20 }, (_, index) => ({
  item_type: 100 + index, dungeon: index % 2 === 0 ? null : lootDungeons[index % 6], pinned_at: at(0),
})) };
export const lootVisuals: VisualPage = { client_fingerprint: 'b'.repeat(64), objects: [
  ...Array.from({ length: 20 }, (_, index) => ({ object_type: 100 + index, name: `Invented Item ${100 + index}`,
    sprite: null, bag: null, item_tier: tier(100 + index) })),
  ...lootDungeons.map((_, index) => ({ object_type: 900 + index, name: `Invented Guardian ${index + 1}`, sprite: null, bag: null })),
  ...ranks.map((rank, index) => ({ object_type: 800 + index, name: `Invented Bag ${index + 1}`, sprite: null,
    bag: { object_type: 800 + index, name: `Invented Bag ${index + 1}`, base_type: 800 + index, is_white: rank === 255, rank, capture: true } })),
] };

function inRange(timestamp: string, range: TimeRange | null): boolean {
  return !range || (BigInt(timestamp) >= BigInt(range.start) && BigInt(timestamp) < BigInt(range.end));
}
function page(rows: LootDropRow[], cursor: DropCursor | null, limit: number): DropPage {
  const remaining = rows.filter(({ drop }) => !cursor || BigInt(drop.at) < BigInt(cursor.at)
    || (drop.at === cursor.at && drop.id < cursor.id));
  const selected = remaining.slice(0, Math.max(1, Math.min(50, limit)));
  const last = selected.at(-1)?.drop;
  return { rows: selected, next: remaining.length > selected.length && last ? { at: last.at, id: last.id } : null };
}
function groups<T>(rows: T[], key: (row: T) => string): T[][] {
  const result = new Map<string, T[]>();
  for (const row of rows) {
    const id = key(row), group = result.get(id) ?? [];
    group.push(row); result.set(id, group);
  }
  return [...result.values()];
}
function appearances(type: number, dungeon: string | null = null) {
  return lootBags.filter(row => dungeon === null || row.dungeon === dungeon)
    .flatMap(row => row.drop.contents.filter(item => item.item_type === type).map(item => ({ row, item })));
}
function bagCounts(rows: LootDropRow[]) {
  return groups(rows, row => String(row.drop.bag_rank)).map(group => ({ bag_rank: group[0].drop.bag_rank, count: group.length }))
    .sort((a, b) => (b.bag_rank ?? -1) - (a.bag_rank ?? -1));
}
const whites = (rows: LootDropRow[]) => rows.filter(row => row.drop.bag_rank === 255).length;
function dryRuns(dungeon: string | null, since: string) {
  return lootRuns.filter(run => run.ended !== null && (dungeon === null || run.dungeon === dungeon) && BigInt(run.started) > BigInt(since)).length;
}

export const lootQueries = {
  listDrops(filter) {
    const rows = lootBags.filter(({ drop, dungeon }) => (filter.min_rank === null || (drop.bag_rank !== null && drop.bag_rank >= filter.min_rank))
      && (!filter.place || (filter.place.kind === 'outside' ? drop.run_id === null : dungeon === filter.place.name))
      && (filter.item_type === null || drop.contents.some(item => item.item_type === filter.item_type))
      && (!filter.enchanted_only || drop.contents.some(item => item.enchant.state === 'recorded' && (item.enchant.applied ?? 0) > 0))
      && inRange(drop.at, filter.range));
    return page(rows, filter.cursor, filter.limit);
  },
  itemHistory(request) {
    const seen = appearances(request.item_type);
    const bags = lootBags.filter(row => row.drop.items.includes(request.item_type));
    return { item_type: request.item_type,
      totals: { appearances: seen.length, bags: bags.length, taken: seen.filter(({ item }) => item.taken_observed_at !== null).length,
        first_at: bags.at(-1)?.drop.at ?? null, last_at: bags[0]?.drop.at ?? null },
      enchant_variants: groups(seen, ({ item }) => JSON.stringify([item.enchant.state, item.enchant.applied, item.enchant.ids]))
        .map(group => ({ state: group[0].item.enchant.state, applied: group[0].item.enchant.applied, ids: group[0].item.enchant.ids, count: group.length })),
      by_dungeon: groups(seen, ({ row }) => JSON.stringify(row.dungeon))
        .map(group => ({ dungeon: group[0].row.dungeon, count: group.length, last_at: group[0].row.drop.at })),
      by_source: groups(seen, ({ row }) => JSON.stringify([row.drop.source_enemy, row.drop.source_estimated]))
        .map(group => ({ source_enemy: group[0].row.drop.source_enemy, estimated: group[0].row.drop.source_estimated, count: group.length })),
      bags: page(bags, request.cursor, request.limit),
    };
  },
  lootStats(filter) {
    const rows = lootBags.filter(row => inRange(row.drop.at, filter.range));
    const ended = lootRuns.filter(run => run.ended !== null && inRange(run.ended, filter.range));
    return {
      dungeons: lootDungeons.filter(dungeon => rows.some(row => row.dungeon === dungeon) || ended.some(run => run.dungeon === dungeon)).map(dungeon => {
        const bags = rows.filter(row => row.dungeon === dungeon), runs = ended.filter(run => run.dungeon === dungeon);
        const lastWhite = lootBags.find(row => row.dungeon === dungeon && row.drop.bag_rank === 255)?.drop.at;
        const best = [...bags].filter(row => row.drop.bag_rank !== null).sort((a, b) => b.drop.bag_rank! - a.drop.bag_rank!)[0]?.drop;
        return { dungeon, runs_ended: runs.length, clears: runs.filter(run => run.cleared).length, bags: bagCounts(bags), whites: whites(bags),
          runs_since_last_white: lastWhite ? dryRuns(dungeon, lastWhite) : null,
          best: best && best.bag_rank !== null ? { drop_id: best.id, run_id: best.run_id, bag_rank: best.bag_rank, at: best.at } : null };
      }),
      sources: groups(rows.filter(row => row.drop.source_enemy !== null), row => JSON.stringify([row.drop.source_enemy, row.dungeon])).map(bags => ({
        source_enemy: bags[0].drop.source_enemy!, dungeon: bags[0].dungeon, bags: bagCounts(bags), whites: whites(bags),
        runs_ended: bags[0].dungeon === null ? null : ended.filter(run => run.dungeon === bags[0].dungeon).length,
      })),
      totals: bagCounts(rows), whites: whites(rows),
    };
  },
  collection(request) {
    return { grid_dungeon: request.dungeon,
      chase: request.chase.map(item => {
        const seen = appearances(item.item_type, item.dungeon), last_at = seen[0]?.row.drop.at ?? null;
        return { item, seen: seen.length, last_at, dry_runs: dryRuns(item.dungeon, last_at ?? item.pinned_at) };
      }),
      grid: request.dungeon === null ? [] : lootVisuals.objects.filter(visual => ['UT', 'ST'].includes(visual.item_tier ?? '')).flatMap(visual => {
        const seen = appearances(visual.object_type, request.dungeon);
        return seen.length ? [{ item_type: visual.object_type, tier: visual.item_tier!, count: seen.length, last_at: seen[0].row.drop.at }] : [];
      }).sort((a, b) => (a.tier === b.tier ? b.count - a.count || a.item_type - b.item_type : a.tier === 'UT' ? -1 : 1)),
    };
  },
  searchItems(query, limit) {
    const text = query.trim().toLowerCase();
    if (!text || text.length > 64) throw new Error('Enter 1–64 characters');
    return lootVisuals.objects.filter(visual => visual.item_tier && visual.name?.toLowerCase().includes(text))
      .slice(0, Math.max(1, Math.min(25, limit)))
      .map(visual => ({ object_type: visual.object_type, name: visual.name!, tier: visual.item_tier ?? null }));
  },
} satisfies Required<Omit<LootFixtureQueries, 'getDropSummary'>>;

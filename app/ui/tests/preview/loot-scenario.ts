import type { DropFilter } from '../../src/lib/bindings/DropFilter';
import type { DropTotals } from '../../src/lib/bindings/DropTotals';
import type { Client } from '../../src/lib/bridge/client';
import type { LootDropRow } from '../../src/lib/bindings/LootDropRow';
import type { DropCursor } from '../../src/lib/bindings/DropCursor';
import type { DropPage } from '../../src/lib/bindings/DropPage';
import type { ChaseItem } from '../../src/lib/bindings/ChaseItem';
import type { EnchantVariant } from '../../src/lib/bindings/EnchantVariant';
import type { PlaceCount } from '../../src/lib/bindings/PlaceCount';
import type { SourceCount } from '../../src/lib/bindings/SourceCount';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { createHomePreview } from './scenario';

const marker = 'LOOT_SYNTHETIC_PREVIEW_ONLY';
export function createLootPreview(variant: 'default' | 'stress'): { client: Client; dispose(): void } {
  const preview = createHomePreview('nexus'), client = preview.client;
  const stress = variant === 'stress', now = new Date();
  const name = (prefix: string, index: number) => stress ? `${prefix} ${index} `.padEnd(64, 'x') : `${prefix} ${index}`;
  const dungeons = Array.from({ length: stress ? 128 : 18 }, (_, i) => name('Invented vault', i + 1));
  const catalog = Array.from({ length: stress ? 100 : 17 }, (_, i) => ({ object_type: 2000 + i, name: name('Synthetic relic', i + 1), tier: !stress && i >= 6 ? (i % 4 === 0 ? 'ST' : 'UT') : i % 3 === 0 ? 'UT' : i % 3 === 1 ? 'ST' : 'T12' }));
  const rows: LootDropRow[] = Array.from({ length: stress ? 1000 : 66 }, (_, i): LootDropRow => {
    const at = String(new Date(now.getFullYear(), now.getMonth(), now.getDate() - Math.floor(i / (stress ? 334 : 22)), 20, 59 - i % 22).getTime() * 1000);
    // Three observed bags give the default collection fourteen distinct UT/ST rows.
    const collectionTypes = !stress && i > 0 && i % 18 === 0 ? [2000, 2001, 2003, ...Array.from({ length: 11 }, (_, n) => 2006 + n)].slice((i / 18 - 1) * 5, i / 18 * 5) : null;
    const contents = i % 11 === 1 ? [] : Array.from({ length: collectionTypes?.length ?? 1 + i % 3 }, (_, slot) => ({ slot, item_type: collectionTypes?.[slot] ?? 2000 + (i + slot) % (stress ? 98 : 4), quantity: 1,
      enchant: { state: 'recorded' as const, slots: 3, applied: (i + slot) % 4, ids: Array.from({ length: (i + slot) % 4 }, (_, j) => j + 1) },
      taken_observed_at: i % 7 === 0 ? at : null }));
    return { dungeon: i % 13 === 0 ? null : dungeons[i % dungeons.length], drop: {
      id: `${marker}-${i}`, session_id: marker, run_id: i % 13 === 0 ? null : `synthetic-run-${i}`, at,
      bag_type: 400 + i % 5, bag_rank: i % 17 === 0 ? null : [255, 4, 2, 1, 0][i % 5], items: contents.map(item => item.item_type), contents,
      enchant: null, contents_known: i % 11 !== 1, source_enemy: i % 9 === 0 ? null : 300 + i % 6,
      source_estimated: i % 4 === 0, source_reason: i % 4 === 0 ? 'Synthetic proximity observation' : null,
      issues: i % 8 === 0 ? ['stream_gap'] : [],
    } };
  }).sort((a, b) => Number(BigInt(b.drop.at) - BigInt(a.drop.at)) || b.drop.id.localeCompare(a.drop.id));
  const ready = <T>(value: T): HistoryReply<T> => ({ status: 'ready', value: structuredClone(value) });
  const page = (matching: LootDropRow[], cursor: DropCursor | null, limit: number): DropPage => {
    const start = cursor ? matching.findIndex(row => row.drop.id === cursor.id) + 1 : 0;
    const selected = matching.slice(start, start + Math.min(50, Math.max(1, limit))), last = selected.at(-1);
    return { rows: selected, next: last && start + selected.length < matching.length ? { at: last.drop.at, id: last.drop.id } : null };
  };
  const matchingRows = (filter: DropFilter) => rows.filter(({ drop, dungeon }) =>
    (filter.min_rank === null || drop.bag_rank !== null && drop.bag_rank >= filter.min_rank)
    && (filter.place === null || (filter.place.kind === 'outside' ? dungeon === null : dungeon === filter.place.name))
    && (filter.item_type === null || drop.items.includes(filter.item_type))
    && (!filter.enchanted_only || drop.contents.some(item => (item.enchant.applied ?? 0) > 0))
    && (!filter.range || BigInt(drop.at) >= BigInt(filter.range.start) && BigInt(drop.at) < BigInt(filter.range.end)));
  client.listDrops = async filter => ready(page(matchingRows(filter), filter.cursor, filter.limit));
  client.getDropSummary = async request => {
    const matching = matchingRows(request.filter);
    const totals = (bags: LootDropRow[], range: DropFilter['range']): DropTotals => {
      const counted = request.include_items && bags.length <= 10000;
      const items = bags.flatMap(row => row.drop.contents);
      const whites = bags.filter(row => row.drop.bag_rank === 255), last = whites[0];
      const ended = rows.filter(row => row.drop.run_id !== null && (!range || BigInt(row.drop.at) >= BigInt(range.start) && BigInt(row.drop.at) < BigInt(range.end))
        && (request.filter.place?.kind !== 'dungeon' || row.dungeon === request.filter.place.name));
      return { bags: bags.length, runs: new Set(bags.flatMap(row => row.drop.run_id ? [row.drop.run_id] : [])).size,
        runs_ended: request.filter.place?.kind === 'outside' ? null : new Set(ended.map(row => row.drop.run_id)).size,
        whites: whites.length, items_counted: counted,
        ut_items: counted ? items.filter(item => catalog.find(c => c.object_type === item.item_type)?.tier === 'UT').length : null,
        st_items: counted ? items.filter(item => catalog.find(c => c.object_type === item.item_type)?.tier === 'ST').length : null,
        enchanted_items: counted ? items.filter(item => (item.enchant.applied ?? 0) > 0).length : null,
        best_enchants: counted && items.length ? Math.max(...items.map(item => item.enchant.applied ?? 0)) : null,
        last_white: last ? { drop_id: last.drop.id, run_id: last.drop.run_id, dungeon: last.dungeon, at: last.drop.at } : null,
        first_at: bags.at(-1)?.drop.at ?? null };
    };
    return ready({ totals: totals(matching, request.filter.range), days: request.days.slice(0, 8).map(day => totals(matching.filter(row => BigInt(row.drop.at) >= BigInt(day.start) && BigInt(row.drop.at) < BigInt(day.end)), day)) });
  };
  client.getItemHistory = async request => {
    const matching = rows.filter(row => row.drop.items.includes(request.item_type));
    const items = matching.flatMap(row => row.drop.contents.filter(item => item.item_type === request.item_type));
    const variants = new Map<string, EnchantVariant>(), places = new Map<string | null, PlaceCount>(), sources = new Map<string, SourceCount>();
    for (const row of matching) {
      const appearances = row.drop.contents.filter(item => item.item_type === request.item_type);
      const place = places.get(row.dungeon) ?? { dungeon: row.dungeon, count: 0, last_at: row.drop.at };
      place.count += appearances.length; places.set(row.dungeon, place);
      const sourceKey = `${row.drop.source_enemy}:${row.drop.source_estimated}`;
      const source = sources.get(sourceKey) ?? { source_enemy: row.drop.source_enemy, estimated: row.drop.source_estimated, count: 0 };
      source.count += appearances.length; sources.set(sourceKey, source);
      for (const { enchant } of appearances) {
        const key = JSON.stringify(enchant);
        const variant = variants.get(key) ?? { state: enchant.state, applied: enchant.applied, ids: enchant.ids, count: 0 };
        variant.count++; variants.set(key, variant);
      }
    }
    return ready({ item_type: request.item_type, totals: { appearances: items.length, bags: matching.length,
      taken: items.filter(item => item.taken_observed_at !== null).length, first_at: matching.at(-1)?.drop.at ?? null, last_at: matching[0]?.drop.at ?? null },
      enchant_variants: [...variants.values()], by_dungeon: [...places.values()].map(place => ({ ...place, runs_since: place.dungeon === null ? null : 7 })).sort((a, b) => b.count - a.count), by_source: [...sources.values()], bags: page(matching, request.cursor, request.limit) });
  };
  client.getLootStats = async filter => {
    const matching = rows.filter(({ drop }) => !filter.range || BigInt(drop.at) >= BigInt(filter.range.start) && BigInt(drop.at) < BigInt(filter.range.end));
    const tiers = (list: LootDropRow[]) => [null, 0, 1, 2, 4, 255].map(bag_rank => ({ bag_rank, bag_type: list.find(row => row.drop.bag_rank === bag_rank)?.drop.bag_type ?? null, count: list.filter(row => row.drop.bag_rank === bag_rank).length }));
    const sourceGroups = new Map<string, LootDropRow[]>();
    for (const row of matching) {
      if (row.drop.source_enemy === null) continue;
      const key = `${row.drop.source_enemy}:${row.dungeon}`;
      sourceGroups.set(key, [...(sourceGroups.get(key) ?? []), row]);
    }
    return ready({ dungeons: dungeons.map(dungeon => {
      const bags = matching.filter(row => row.dungeon === dungeon);
      const whiteRuns = new Set(rows.filter(row => row.dungeon === dungeon && row.drop.bag_rank === 255).map(row => row.drop.run_id)).size;
      return { dungeon, runs_ended: 41, clears: 35, bags: tiers(bags), whites: bags.filter(row => row.drop.bag_rank === 255).length, runs_since_last_white: rows.some(row => row.dungeon === dungeon && row.drop.bag_rank === 255) ? 7 : null,
        // Synthetic 41-run history: a leading gap, consecutive white runs, then seven dry runs.
        white_runs: whiteRuns, avg_runs_per_white: whiteRuns ? 41 / whiteRuns : null, longest_dry: whiteRuns ? Math.max(7, 41 - whiteRuns - 7) : null,
        best: bags.length ? (() => {
          const best = [...bags].sort((a, b) => (b.drop.bag_rank ?? -1) - (a.drop.bag_rank ?? -1))[0].drop;
          if (best.bag_rank === null) return null;
          return { drop_id: best.id, run_id: best.run_id, bag_rank: best.bag_rank, bag_type: best.bag_type, at: best.at };
        })() : null };
    }), sources: [...sourceGroups.values()].map(bags => ({ source_enemy: bags[0].drop.source_enemy!, dungeon: bags[0].dungeon,
      bags: tiers(bags), whites: bags.filter(row => row.drop.bag_rank === 255).length, runs_ended: bags[0].dungeon === null ? null : 41 })), totals: tiers(matching), whites: matching.filter(row => row.drop.bag_rank === 255).length });
  };
  let chase: ChaseItem[] = catalog.slice(0, stress ? 100 : 6).map((item, i) => ({ item_type: item.object_type, dungeon: i % 2 ? dungeons[i % dungeons.length] : null, pinned_at: rows.at(-1)!.drop.at }));
  const settings = client.querySettings;
  client.querySettings = async () => ({ ...await settings(), loot: { chase: structuredClone(chase) } });
  client.setChaseList = async items => { chase = structuredClone(items); };
  client.getCollection = async request => ready({ grid_dungeon: request.dungeon,
    chase: request.chase.map(item => {
      const eligible = rows.filter(row => item.dungeon === null || row.dungeon === item.dungeon);
      const seen = eligible.filter(row => row.drop.items.includes(item.item_type));
      const since = seen[0]?.drop.at ?? item.pinned_at;
      const dry = new Set(eligible.filter(row => row.drop.run_id !== null && BigInt(row.drop.at) > BigInt(since)).map(row => row.drop.run_id)).size;
      return { item, seen: seen.length, last_at: seen[0]?.drop.at ?? null, dry_runs: dry };
    }), grid: catalog.filter(item => item.tier !== 'T12').flatMap(item => {
      const seen = rows.filter(row => (request.dungeon !== null && row.dungeon === request.dungeon) && row.drop.items.includes(item.object_type));
      return seen.length ? [{ item_type: item.object_type, tier: item.tier, count: seen.length, last_at: seen[0].drop.at }] : [];
    }) });
  client.searchItems = async (query, limit) => catalog.filter(item => item.name.toLowerCase().includes(query.toLowerCase())).slice(0, limit);
  client.listCatalogDungeons = async () => [...dungeons];
  const visuals = client.getObjectVisuals;
  client.getObjectVisuals = async types => {
    const result = await visuals(types.filter(type => type < 2000));
    return { ...result, objects: [...result.objects.map(object => object.bag?.is_white ? { ...object, bag: { ...object.bag, rank: 255 } } : object), ...catalog.filter(item => types.includes(item.object_type)).map(item => ({ object_type: item.object_type, name: item.name, item_tier: item.tier, sprite: null, bag: null }))] };
  };
  return preview;
}

<script lang="ts">
  import { itemHover } from '../components/item-hover';
  import { debugMode } from '../debug/debug';
  import { reasonLabel } from '../home/evidence';
  import type { LootDropRow } from '../bindings/LootDropRow';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { HomeVisuals } from '../home/visuals';
  import EnchantLabel from './EnchantLabel.svelte';
  import ItemTile from '../components/ItemTile.svelte';
  import SpriteImage from '../components/SpriteImage.svelte';
  import { bagTone } from '../styles/rarity';
  import { routeHref } from '../navigation/routes';
  import { runDate } from '../runs/runs-list';
  import { bagLabel, placeLabel, sourceLabel } from './presentation';
  import { bestItem, tierClass, clockWhen } from './loot-format';
  let { rows, visuals, resolveSpriteUrl, now, columns, itemType, groups = null, selectedDropId = null, onSelect, rowHref }: {
    rows: LootDropRow[]; visuals: HomeVisuals; resolveSpriteUrl: (sprite: SpriteRef) => string | null; now: Date;
    columns: 'feed' | 'item'; itemType?: number;
    groups?: Array<{ key: string; label: string; line: string; rows: LootDropRow[] }> | null;
    selectedDropId?: string | null; onSelect?: (row: LootDropRow) => void; rowHref?: (row: LootDropRow) => string | null;
  } = $props();
  const visual = (type: number | null) => visuals.objects.find(v => v.object_type === type) ?? null;
  let sections = $derived(groups ?? [{ key: 'all', label: '', line: '', rows }]);
  function select(event: MouseEvent | KeyboardEvent, row: LootDropRow) {
    if (event.target instanceof Element && event.target.closest('a')) return;
    if (event instanceof KeyboardEvent && event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    if (columns === 'item') (event.currentTarget as HTMLElement).querySelector<HTMLAnchorElement>('a.run-link')?.click();
    else onSelect?.(row);
  }
</script>
<table aria-label="Bags" data-columns={columns}>
  <colgroup><col class="time" /><col class="bag" />{#if columns === 'item'}<col class="enchants" />{/if}<col />{#if columns === 'item'}<col class="taken-col" />{/if}<col class="source" /><col class="place" /><col class="arrow" /></colgroup>
  <thead><tr><th>{columns === 'item' ? 'When' : 'Time'}</th><th>Bag</th>{#if columns === 'item'}<th>Enchants</th>{/if}<th>{columns === 'item' ? 'Also in the bag' : 'Contents'}</th>{#if columns === 'item'}<th>Taken</th>{/if}<th>Source</th><th>Place</th><th><span class="sr-only">Run</span></th></tr></thead>
  {#each sections as group (group.key)}
    <tbody>
      {#if group.label}<tr class="group"><td colspan={columns === 'item' ? 8 : 6}><b>{group.label}</b>{#if group.line}{' · '}{group.line}{/if}</td></tr>{/if}
      {#each group.rows as row (row.drop.id)}
        {@const drop = row.drop}
        {@const bag = visual(drop.bag_type)}
        {@const sourceVisual = visual(drop.source_enemy)}
        {@const source = sourceLabel(drop, sourceVisual?.name ?? null)}
        {@const items = [...drop.contents].sort((a, b) => a.slot - b.slot)}
        {@const copies = items.filter(item => item.item_type === itemType)}
        {@const others = columns === 'item' ? items.filter(item => item.item_type !== itemType) : items}
        {@const best = bestItem(items, type => visual(type)?.item_tier ?? null)}
        <tr data-row-id={`drop:${drop.id}`} tabindex="0" aria-current={selectedDropId === drop.id ? 'true' : undefined} class:selected={selectedDropId === drop.id} onclick={event => select(event, row)} onkeydown={event => select(event, row)}>
          <td title={clockWhen(drop.at, now)}>{columns === 'item' ? clockWhen(drop.at, now) : runDate(drop.at).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })}</td>
          <td><span class="cell" data-bag-tone={bagTone(bag?.bag ?? null)} class:white={drop.bag_rank === 255}><SpriteImage sprite={bag?.sprite ?? null} sheetUrl={bag?.sprite ? resolveSpriteUrl(bag.sprite) : null} fit={20} alt="" /><span title={bagLabel(drop.bag_rank, bag)}>{bagLabel(drop.bag_rank, bag)}</span></span></td>
          {#if columns === 'item'}<td>
            {#each copies as copy}<div class="cell"><EnchantLabel enchant={copy.enchant} /></div>{:else}—{/each}
          </td>{/if}
          <td><div class="cell contents">
            {#each others.slice(0, 6) as item (`${item.slot}:${item.item_type}`)}
              {@const object = visual(item.item_type)}
              <a class="tile {tierClass(object?.item_tier)}" class:taken={item.taken_observed_at !== null} use:itemHover={{ type: item.item_type, enchant: item.enchant, name: object?.name ?? `#${item.item_type}`, tier: object?.item_tier, note: item.taken_observed_at !== null ? 'Removal observed' : undefined }} aria-label={object?.name ?? `#${item.item_type}`} href={routeHref({ kind: 'item', type: item.item_type })}><ItemTile sprite={object?.sprite ?? null} {resolveSpriteUrl} fit={24} alt="" enchant={item.enchant} /></a>
            {/each}
            {#if others.length > 6}<small>+{others.length - 6}</small>{/if}
            {#if columns === 'feed' && best}<span class={tierClass(visual(best.item_type)?.item_tier)} use:itemHover={{ type: best.item_type, enchant: best.enchant, name: visual(best.item_type)?.name ?? `#${best.item_type}`, tier: visual(best.item_type)?.item_tier }}>{visual(best.item_type)?.name ?? `#${best.item_type}`}</span>{/if}
            {#if !drop.contents_known}<small>{items.length ? 'partial' : 'contents not seen'}</small>{:else if !others.length}<small>{columns === 'item' ? '—' : 'Empty bag'}</small>{/if}
          </div></td>
          {#if columns === 'item'}<td>{#each copies as copy}<div title={copy.taken_observed_at !== null ? 'Removal observed; who took the item is unknown.' : undefined}>{copy.taken_observed_at !== null ? 'Taken' : '—'}</div>{:else}—{/each}</td>{/if}
          <td class="source-cell"><div class="cell">{#if columns === 'item'}<SpriteImage sprite={sourceVisual?.sprite ?? null} sheetUrl={sourceVisual?.sprite ? resolveSpriteUrl(sourceVisual.sprite) : null} fit={20} alt="" />{/if}<span title={source.text}>{source.text}</span>{#if source.estimated}<small class="tag" title={$debugMode ? reasonLabel(source.reason) : undefined}>est.</small>{/if}</div></td>
          <td title={placeLabel(row.dungeon)}>{placeLabel(row.dungeon)}</td>
          <td>{#if rowHref?.(row) || drop.run_id}<a class="run-link" aria-label="Open run" href={rowHref?.(row) ?? routeHref({ kind: 'run', id: drop.run_id! })}>›</a>{/if}</td>
        </tr>
      {/each}
    </tbody>
  {/each}
</table>
<style>
  table { width: 100%; table-layout: fixed; border-collapse: collapse; font-size: 12px; font-variant-numeric: tabular-nums; }
  table[data-columns="item"] { min-width: 760px; }
  [data-columns="item"] th { white-space: nowrap; }
  .time { width: 75px; } .bag { width: 115px; } .source { width: 23%; } .place { width: 17%; } .arrow { width: 24px; }
  [data-columns="item"] .time { width: 110px; }
  .enchants { width: 118px; } .taken-col { width: 52px; }
  th { text-align: left; font-weight: 500; font-size: 11px; color: var(--text-muted); padding: 0 6px 6px; }
  td { height: 30px; padding: 0 6px; border-top: 1px solid var(--border-soft, var(--border)); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  tr[data-row-id] { cursor: pointer; } tr[data-row-id]:hover { background: var(--surface-hover); }
  tr.selected { background: #f2c14e14; } tr.selected td:first-child { box-shadow: inset 2px 0 var(--gold); }
  tr:focus-visible, a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .group td { height: 26px; border: 0; color: var(--text-muted); font-size: 11px; }
  .group b { color: var(--text-secondary); }
  .cell { display: flex; align-items: center; gap: 6px; min-width: 0; overflow: hidden; }
  .cell > span { overflow: hidden; text-overflow: ellipsis; }
  .source-cell .cell { flex-wrap: wrap; gap: 2px 6px; padding-block: 4px; }
  .source-cell .cell > span { white-space: normal; overflow-wrap: anywhere; }
  .contents { gap: 3px; } .tile { display: inline-flex; flex: none; border: 1px solid transparent; border-radius: 4px; }
  .ut { color: var(--violet-text); } .st { color: var(--bag-orange); } .tile.ut, .tile.st { border-color: currentColor; }
  .taken { opacity: .45; } .white { color: var(--bag-white); filter: drop-shadow(0 0 3px #ffffff30); }
  small { color: var(--text-muted); } .tag { border: 1px solid var(--border); border-radius: 4px; padding: 0 3px; }
  a { color: var(--text-secondary); text-decoration: none; }
  @media (max-width: 1100px) { .bag { width: 95px; } .source { width: 24%; } }
</style>

<script lang="ts">
  import { itemHover } from '../../components/item-hover';
  import { objectVisual } from '../../bridge/visual-index';
  import { onMount } from 'svelte';
  import type { DropView } from '../../bindings/DropView';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import type { HomeVisuals } from '../visuals';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import ItemTile from '../../components/ItemTile.svelte';
  import Tooltip from '../../components/Tooltip.svelte';
  import DebugNote from '../../debug/DebugNote.svelte';
  import { coverageLabels, reasonLabel } from '../evidence';
  import { lootItems } from '../v2/loot';
  import { rankedDrops, overflowSummary } from './loot-rank';
  import { formatStat } from '../../styles/format';
  let { drops, visuals, resolveSpriteUrl, clock, scroll = false, selectedDropId = null, newestFirst = false }: {
    scroll?: boolean; selectedDropId?: string | null; newestFirst?: boolean;
    drops: DropView[]; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string | null; clock: (at: string) => string;
  } = $props();
  const id = $props.id();
  let host: HTMLDivElement;
  let height = $state(0);
  const at = (drop: DropView) => /^\d+$/.test(drop.at) ? BigInt(drop.at) : -1n;
  let ranked = $derived(newestFirst ? [...rankedDrops(drops)].sort((a, b) => at(a) < at(b) ? 1 : at(a) > at(b) ? -1 : 0)
    : rankedDrops(drops));
  let slots = $derived(Math.max(0, Math.floor((height + 3) / 39)));
  let capacity = $derived(scroll ? ranked.length : ranked.length <= slots ? slots : Math.max(0, slots - 1));
  let hidden = $derived(ranked.slice(capacity));
  const visual = (type: number) => objectVisual(visuals.objects, type);
  onMount(() => {
    if (typeof ResizeObserver === 'undefined') { height = host.clientHeight; return; }
    const observer = new ResizeObserver(entries => { height = entries[0]?.contentRect.height ?? 0; });
    observer.observe(host);
    return () => observer.disconnect();
  });
</script>
<div class="bag-list-root" class:scroll>
<div class="bag-list" bind:this={host}>
  {#if ranked.length && capacity > 0}
  <div class="bag-rows" style:overflow-y={scroll ? 'auto' : undefined}
    {...scroll ? { tabindex: 0, role: 'region', 'aria-label': 'All bags' } : {}}>
  {#each ranked.slice(0, capacity) as drop (drop.id)}
    {@const bag = visual(drop.bag_type)}
    {@const items = lootItems(drop, visuals)}
    <div class="row" class:selected={selectedDropId === drop.id} data-drop-id={drop.id} data-row-id={scroll ? `drop:${drop.id}` : undefined} {...(scroll ? { tabindex: -1 } : {})}>
      <SpriteImage sprite={bag?.sprite ?? null} sheetUrl={bag?.sprite ? resolveSpriteUrl(bag.sprite) : null} fit={24} alt={bag?.name ?? `Bag ${drop.bag_type}`} />
      <span class="items">
        {#each items.slice(0, 4) as item (`${item.slot}:${item.itemType}`)}
          {@const object = visual(item.itemType)}
          <span class="item" data-tier={item.tier ?? ''} use:itemHover={{ type: item.itemType, enchant: item.enchant, name: object?.name ?? `Item ${item.itemType}`, tier: item.tier }}><ItemTile sprite={object?.sprite ?? null} {resolveSpriteUrl} fit={24} alt={object?.name ?? `Item ${item.itemType}`} enchant={item.enchant} /></span>
        {/each}
        {#if items.length > 4}<span class="more">+{formatStat(items.length - 4)}</span>{/if}
        {#if !items.length}<span>—</span>{/if}
      </span>
      <time>{clock(drop.at)}</time>
    </div>
  {/each}
  </div>
  {/if}
  {#if hidden.length}
    <Tooltip id={`${id}-overflow`} text={overflowSummary(hidden).map(row => `${visual(row.bagType)?.name ?? `Bag ${row.bagType}`}: ${formatStat(row.count)}`).join(', ')}>
      {#snippet trigger(tooltipId)}
        <button type="button" class="overflow" aria-describedby={tooltipId}>
          <span>+{formatStat(hidden.length)} more</span>
          {#each overflowSummary(hidden) as row (row.bagType)}
            {@const bag = visual(row.bagType)}
            <span class="hidden-type" data-overflow-bag={row.bagType}><SpriteImage sprite={bag?.sprite ?? null} sheetUrl={bag?.sprite ? resolveSpriteUrl(bag.sprite) : null} fit={16} alt={bag?.name ?? `Bag ${row.bagType}`} />{formatStat(row.count)}</span>
          {/each}
        </button>
      {/snippet}
    </Tooltip>
  {:else if !ranked.length}<p>No Bags</p>{/if}
</div>
<!-- Out of flow and outside the measured flex area; debug notes get no flex share. -->
<div class="bag-list-notes">
{#each ranked as drop (drop.id)}
  {#if !drop.contents_known}<DebugNote text="Partially observed contents; other items may be missing." />{/if}
  {#if drop.source_estimated}<DebugNote>Source estimated: {reasonLabel(drop.source_reason)}</DebugNote>{/if}
  {#each drop.issues as issue}<DebugNote text={coverageLabels[issue]} />{/each}
{/each}
</div>
</div>
<style>
  .row.selected { outline: 2px solid var(--gold); outline-offset: -2px; }
  .bag-list-root { position: relative; flex: 1; height: 100%; min-height: 36px; min-width: 0; }
  .bag-list-root .bag-list-notes { position: absolute; top: 100%; left: 0; right: 0; z-index: 1; max-height: 160px; overflow: auto; background: var(--surface-panel); }
  .bag-list-root .bag-list { flex: 1; height: 100%; min-height: 36px; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .bag-list-root .bag-rows { min-height: 0; display: flex; flex-direction: column; gap: 3px; overflow: hidden; }
  .bag-list-root .row { flex: none; height: 36px; display: grid; grid-template-columns: 24px minmax(0, 1fr) 68px; align-items: center; gap: 6px; padding: 0 5px; border-radius: 5px; background: var(--surface-raised); }
  .bag-list-root .items { display: flex; align-items: center; gap: 4px; min-width: 0; overflow: hidden; }
  .bag-list-root .item { display: inline-flex; flex: none; border-radius: 3px; border: 1px solid transparent; }
  .bag-list-root [data-tier="UT"] { border-color: var(--violet-text); box-shadow: 0 0 6px #a77bff44; }
  .bag-list-root [data-tier="ST"] { border-color: var(--bag-orange); box-shadow: 0 0 6px #ffb34744; }
  .bag-list-root time { text-align: right; white-space: nowrap; font-size: 11px; font-variant-numeric: tabular-nums; color: var(--text-secondary); }
  .bag-list-root .more { font-size: 10px; padding: 1px 3px; border: 1px solid var(--border); border-radius: 4px; }
  .bag-list-root .overflow { display: flex; gap: 5px; align-items: center; height: 36px; min-width: 0; max-width: 100%; overflow: hidden; padding: 0; border: 0; background: transparent; color: inherit; font: inherit; white-space: nowrap; font-size: 11px; cursor: help; }
  .bag-list-root .hidden-type { flex: none; display: inline-flex; align-items: center; gap: 2px; font-variant-numeric: tabular-nums; }
  .bag-list-root p { margin: 0; color: var(--text-secondary); }
</style>

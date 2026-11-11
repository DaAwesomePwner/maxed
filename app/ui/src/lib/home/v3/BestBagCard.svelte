<script lang="ts">
  import { itemHover } from '../../components/item-hover';
  import { objectVisual } from '../../bridge/visual-index';
  import type { DropView } from '../../bindings/DropView';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import type { HomeVisuals } from '../visuals';
  import type { LootItem } from '../v2/loot';
  import ItemTile from '../../components/ItemTile.svelte';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import DebugNote from '../../debug/DebugNote.svelte';
  import { coverageLabels, reasonLabel } from '../evidence';
  import { enchantCount } from '../../styles/enchant';
  import { rarityName } from './loot-rank';
  import { onDestroy } from 'svelte';
  // `scope` names whose loot this is (a run, visit or history scope). The card
  // flashes only when the best bag changes within one loaded scope, never when a
  // scope first loads or is switched. Null means the scope is still loading.
  let { best, visuals, resolveSpriteUrl, meta, scope = '', selected = false }: {
    selected?: boolean; best: { drop: DropView; item: LootItem | null } | null; visuals: HomeVisuals;
    resolveSpriteUrl: (s: SpriteRef) => string | null; meta: string; scope?: string | null;
  } = $props();
  let itemVisual = $derived(objectVisual(visuals.objects, best?.item?.itemType));
  let featured = $derived(best?.drop ?? null);
  let bagVisual = $derived(objectVisual(visuals.objects, featured?.bag_type));
  let count = $derived(enchantCount(best?.item?.enchant));
  let rarity = $derived(rarityName(count));
  let node: HTMLDivElement | undefined = $state();
  let seenScope: string | null = null;
  let previous: string | null = null;
  let animation: Animation | undefined;
  onDestroy(() => animation?.cancel());
  $effect(() => {
    const key = featured?.id ?? null, current = scope;
    const changed = current !== null && current === seenScope && key !== null && key !== previous;
    seenScope = current; previous = key;
    if (!changed || !node) return;
    animation?.cancel();
    if (!(best?.item?.tier === 'UT' || best?.item?.tier === 'ST' || bagVisual?.bag?.is_white)) return;
    if (globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return;
    const colour = getComputedStyle(node).color;
    // A one-shot tint in the tier colour that settles back to the card surface; no glow.
    animation = node.animate?.([
      { backgroundColor: `color-mix(in srgb, ${colour} 16%, var(--surface-raised))` },
      {},
    ], { duration: 600, iterations: 1 });
  });
</script>
{#if featured}
  <div class="best-card" class:selected {...(selected ? { tabindex: -1, 'data-row-id': `drop:${featured.id}` } : {})} data-tier={best?.item?.tier ?? ''} data-drop-id={featured.id} bind:this={node}>
    <div class="pictures">
      <span class="bag-box"><SpriteImage sprite={bagVisual?.sprite ?? null} sheetUrl={bagVisual?.sprite ? resolveSpriteUrl(bagVisual.sprite) : null} fit={40} alt={bagVisual?.name ?? `Bag ${featured.bag_type}`} /></span>
      {#if best?.item}<span class="item-box" use:itemHover={{ type: best.item.itemType, enchant: best.item.enchant, name: itemVisual?.name ?? `Item ${best.item.itemType}`, tier: best.item.tier }}><ItemTile sprite={itemVisual?.sprite ?? null} {resolveSpriteUrl} fit={44} alt={itemVisual?.name ?? `Item ${best.item.itemType}`} enchant={best.item.enchant} /></span>{/if}
    </div>
    <div class="copy">
      {#if best?.item}<div class="name clamp">{itemVisual?.name ?? `Item ${best.item.itemType}`}</div>
      <div class="meta">{#if best.item.tier}<span class="tier">{best.item.tier}</span>{/if}<span>{meta}</span></div>
      {#if rarity}<div class="enchants"><span class="rarity" data-rarity={count}>{rarity}</span><span>{count} enchants</span></div>{/if}
      {:else}<div class="name clamp">{bagVisual?.name ?? `Bag ${featured.bag_type}`}</div><div class="meta">{meta}</div>{/if}
    </div>
  </div>
  {#if !featured.contents.length && !(featured.contents_known && featured.items.length)}<DebugNote text="Contents unknown" />{/if}
  {#if featured.bag_rank === null}<DebugNote text="Bag rank unavailable." />{/if}
  {#if featured.source_estimated}<DebugNote>Source estimated: {reasonLabel(featured.source_reason)}</DebugNote><DebugNote text="Source estimated." />{/if}
  {#if !featured.contents_known}<DebugNote text="Partially observed contents; other items may be missing." />{/if}
  {#if featured.contents.length && featured.contents.every(item => item.taken_observed_at !== null)}<DebugNote text="Removal was observed for every retained item. Current contents are unavailable; an item may have reappeared." />{/if}
  {#each featured.issues as issue}<DebugNote text={coverageLabels[issue]} />{/each}
{:else}<p class="best-empty">No bags</p>{/if}
<DebugNote text="Observed contents, not pickup or ownership." />
<style>
  .best-card.selected { border-color: var(--hairline-strong); box-shadow: inset 2px 0 0 var(--text-secondary); }
  .best-card { --tier: var(--text-primary); color: var(--tier); display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); align-items: center; padding: var(--space-3); border-radius: var(--radius-control); background: var(--surface-raised); border: 1px solid var(--hairline); min-width: 0; }
  .best-card[data-tier="UT"] { --tier: var(--tier-ut); }
  .best-card[data-tier="ST"] { --tier: var(--tier-st); }
  .best-card .pictures { display: flex; gap: var(--space-1); align-items: center; }
  .best-card .bag-box, .best-card .item-box { display: grid; place-items: center; width: 56px; height: 56px; border-radius: var(--radius-control); }
  .best-card .item-box { box-sizing: border-box; background: var(--surface-inset); border: 1px solid var(--tier); }
  .best-card .copy { min-width: 0; }
  .best-card .name { font-weight: var(--weight-semibold); font-size: var(--text-label); line-height: var(--leading-label); color: var(--tier); }
  .best-card .clamp { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  .best-card .meta, .best-card .enchants { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); margin-top: var(--space-1); font-size: var(--text-meta); line-height: var(--leading-meta); color: var(--text-secondary); }
  .best-card .meta > span:last-child { min-width: 0; overflow-wrap: anywhere; }
  .best-card .tier, .best-card .rarity { display: inline-flex; align-items: center; box-sizing: border-box; height: 18px; padding: 0 var(--space-1); border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); font-size: var(--text-meta); font-weight: var(--weight-medium); }
  .best-card .tier { color: var(--tier); }
  .best-card [data-rarity="1"] { color: var(--rarity-uncommon); }
  .best-card [data-rarity="2"] { color: var(--rarity-rare); }
  .best-card [data-rarity="3"] { color: var(--rarity-legendary); }
  .best-card [data-rarity="4"] { color: var(--rarity-divine); }
  .best-empty { margin: 0; color: var(--text-secondary); }
  @media (max-width: 900px) {
    .best-card { gap: var(--space-2); padding: var(--space-2); }
    .best-card .bag-box { width: 40px; }
    .best-card .item-box { width: 48px; height: 48px; }
  }
</style>

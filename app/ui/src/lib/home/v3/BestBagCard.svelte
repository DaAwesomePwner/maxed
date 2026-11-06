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
  // glows only when the best bag changes within one loaded scope, never when a
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
    animation = node.animate?.([
      { boxShadow: `0 0 22px 4px ${colour}` },
      { boxShadow: `0 0 0 0 transparent` },
    ], { duration: 600, iterations: 1 });
  });
</script>
{#if featured}
  <div class="best-card" class:selected {...(selected ? { tabindex: -1, 'data-row-id': `drop:${featured.id}` } : {})} data-tier={best?.item?.tier ?? ''} data-drop-id={featured.id} bind:this={node}>
    <div class="pictures">
      <span class="bag-glow"><SpriteImage sprite={bagVisual?.sprite ?? null} sheetUrl={bagVisual?.sprite ? resolveSpriteUrl(bagVisual.sprite) : null} fit={40} alt={bagVisual?.name ?? `Bag ${featured.bag_type}`} /></span>
      {#if best?.item}<span class="item-box" use:itemHover={{ type: best.item.itemType, enchant: best.item.enchant, name: itemVisual?.name ?? `Item ${best.item.itemType}`, tier: best.item.tier }}><ItemTile sprite={itemVisual?.sprite ?? null} {resolveSpriteUrl} fit={44} alt={itemVisual?.name ?? `Item ${best.item.itemType}`} enchant={best.item.enchant} /></span>{/if}
    </div>
    <div class="copy">
      {#if best?.item}<div class="name clamp">{itemVisual?.name ?? `Item ${best.item.itemType}`}</div>
      <div class="meta">{#if best.item.tier}<span class="tier">{best.item.tier}</span>{/if}<span>{meta}</span></div>
      {#if rarity}<div class="enchants"><span class="rarity" data-rarity={count}>{rarity}</span><span>{count} enchants</span></div>{/if}
      {:else}<div class="name clamp">{bagVisual?.name ?? `Bag ${featured.bag_type}`}</div><div class="meta">{meta}</div>{/if}
    </div>
  </div>
  {#if !best?.item}<DebugNote text="Contents unknown" />{/if}
  {#if featured.bag_rank === null}<DebugNote text="Bag rank unavailable." />{/if}
  {#if featured.source_estimated}<DebugNote>Source estimated: {reasonLabel(featured.source_reason)}</DebugNote><DebugNote text="Source estimated." />{/if}
  {#if !featured.contents_known}<DebugNote text="Partially observed contents; other items may be missing." />{/if}
  {#if featured.contents.length && featured.contents.every(item => item.taken_observed_at !== null)}<DebugNote text="Removal was observed for every retained item. Current contents are unavailable; an item may have reappeared." />{/if}
  {#each featured.issues as issue}<DebugNote text={coverageLabels[issue]} />{/each}
{:else}<p class="best-empty">No Bags</p>{/if}
<DebugNote text="Observed contents, not pickup or ownership." />
<style>
  .best-card.selected { outline: 2px solid var(--gold); outline-offset: -2px; }
  .best-card { --tier: var(--text-primary); color: var(--tier); display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 12px; align-items: center; padding: 9px 10px; border-radius: 8px; background: linear-gradient(90deg, #ffffff12, #ffffff05); border: 1px solid #ffffff2a; min-width: 0; }
  .best-card[data-tier="UT"] { --tier: var(--violet-text); }
  .best-card[data-tier="ST"] { --tier: var(--bag-orange); }
  .best-card .pictures { display: flex; gap: 4px; align-items: center; }
  .best-card .bag-glow, .best-card .item-box { display: grid; place-items: center; width: 56px; height: 56px; border-radius: 10px; }
  .best-card .bag-glow { background: radial-gradient(circle, #ffffff30, #ffffff08 70%); }
  .best-card .item-box { background: var(--surface-inset, var(--surface-panel)); border: 1px solid var(--tier); box-shadow: 0 0 12px color-mix(in srgb, var(--tier) 33%, transparent); }
  .best-card .copy { min-width: 0; }
  .best-card .name { font-weight: 700; font-size: 15px; color: var(--tier); }
  .best-card .clamp { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; }
  .best-card .meta, .best-card .enchants { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-top: 3px; font-size: 12px; color: var(--text-secondary); }
  .best-card .meta > span:last-child { min-width: 0; overflow-wrap: anywhere; }
  .best-card .tier { font-size: 10px; font-weight: 800; padding: 1px 5px; border-radius: 3px; color: var(--tier); background: var(--surface-raised); }
  .best-card .rarity { border: 1px solid currentColor; padding: 1px 6px; border-radius: 3px; font-size: 10.5px; }
  .best-card [data-rarity="1"] { color: var(--rarity-uncommon); }
  .best-card [data-rarity="2"] { color: var(--rarity-rare); }
  .best-card [data-rarity="3"] { color: var(--rarity-legendary); }
  .best-card [data-rarity="4"] { color: var(--rarity-divine); box-shadow: 0 0 8px #ffcf2933; }
  .best-empty { margin: 0; color: var(--text-secondary); }
  @media (max-width: 900px) {
    .best-card { gap: 6px; padding: 7px; }
    .best-card .bag-glow { width: 40px; }
    .best-card .item-box { width: 46px; height: 48px; }
  }
</style>

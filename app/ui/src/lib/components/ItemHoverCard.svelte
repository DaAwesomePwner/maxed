<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { ItemDetailLookup } from '../bridge/item-details';
  import { closeItemHover, ITEM_HOVER_ID, itemHoverState } from './item-hover';
  import { itemCardModel } from './item-card';
  import { placeFixed, scrollBounds } from './placement';

  let { client }: { client: Pick<Client, 'getItemDetails'> } = $props();
  let lookup = $state<ItemDetailLookup | null>(null);
  let failed = $state(false);
  let card = $state<HTMLDivElement>();
  let requestToken = 0;
  const anchor = $derived($itemHoverState?.anchor);
  const request = $derived($itemHoverState ? JSON.stringify({ type: $itemHoverState.params.type, ids: $itemHoverState.params.enchant?.state === 'recorded' ? $itemHoverState.params.enchant.ids : [] }) : null);
  const model = $derived($itemHoverState ? itemCardModel($itemHoverState.params, lookup, failed) : null);
  $effect(() => {
    const token = ++requestToken;
    lookup = null;
    failed = false;
    if (!anchor || !request) return;
    const { type, ids } = JSON.parse(request) as { type: number; ids: number[] };
    let active = true;
    client.getItemDetails(type, ids).then(result => {
      if (active && token === requestToken) lookup = result;
    }).catch(() => {
      if (active && token === requestToken) failed = true;
    });
    return () => { active = false; };
  });
  $effect(() => {
    if (!$itemHoverState?.open || !anchor || !card || !model) return;
    const node = anchor, element = card;
    let active = true;
    const place = () => {
      if (!active) return;
      const width = document.documentElement.clientWidth || window.innerWidth;
      const height = document.documentElement.clientHeight || window.innerHeight;
      const bounds = scrollBounds(node, width);
      element.style.maxWidth = `min(15rem, ${Math.max(0, bounds.stop - bounds.start - 16)}px)`;
      element.style.left = '0px';
      element.style.top = '0px';
      const position = placeFixed(node.getBoundingClientRect(), element.getBoundingClientRect(), bounds, height);
      element.style.left = `${position.left}px`;
      element.style.top = `${position.top}px`;
    };
    void tick().then(place);
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => {
      active = false;
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  });
  onDestroy(closeItemHover);
</script>

<svelte:window onblur={() => closeItemHover()} onkeydown={event => { if (event.key === 'Escape') closeItemHover(); }} />
{#if $itemHoverState?.open && model}
  <div id={ITEM_HOVER_ID} role="tooltip" class="item-hover-card" data-rarity={model.rarity?.level} bind:this={card}>
    <div class="name {model.tierClass}">{model.name}</div>
    <!-- Render rarity between the tier/slot base and the surface's suffix. -->
    <div class="meta">{model.base}{#if model.rarity}{model.base ? ' · ' : ''}<span class="rarity">{model.rarity.name}</span>{/if}{#if $itemHoverState.params.suffix}{model.base || model.rarity ? ' · ' : ''}{$itemHoverState.params.suffix}{/if}</div>
    {#if model.section.kind === 'message'}
      <div class="section message">{model.section.text}</div>
    {:else if model.section.kind === 'rows'}
      <div class="section">
        {#if model.section.label}<div class="meta">{model.section.label}</div>{/if}
        {#each model.section.rows as row}
          <div class="enchant-row">
            <span class="dot" aria-hidden="true"></span>
            <div>{row.name}{#if row.unknown} <span class="unknown">not in catalog</span>{/if}
              {#if row.description}<div class="description">{row.description}</div>{/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
    {#if model.note}<div class="note">{model.note}</div>{/if}
  </div>
{/if}

<style>
  .item-hover-card { position: fixed; z-index: 30; pointer-events: none; max-width: 15rem; box-sizing: border-box; padding: var(--space-3); border: 1px solid var(--border-strong); border-radius: var(--radius-control); background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--elevation-popover); font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-normal); overflow-wrap: anywhere; }
  .name { font-weight: var(--weight-medium); }
  .ut { color: var(--tier-ut); }
  .st { color: var(--tier-st); }
  .meta, .note { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .section { margin-top: var(--space-2); padding-top: var(--space-2); border-top: 1px solid var(--hairline); }
  .message { color: var(--text-muted); font-style: italic; }
  .enchant-row { display: grid; grid-template-columns: 6px minmax(0, 1fr); gap: var(--space-2); margin-top: var(--space-1); }
  .dot { width: 6px; height: 6px; margin-top: var(--space-2); border-radius: var(--radius-pill); background: var(--text-muted); }
  [data-rarity="1"] .rarity { color: var(--rarity-uncommon); }
  [data-rarity="2"] .rarity { color: var(--rarity-rare); }
  [data-rarity="3"] .rarity { color: var(--rarity-legendary); }
  [data-rarity="4"] .rarity { color: var(--rarity-divine); }
  [data-rarity="1"] .dot { background: var(--rarity-uncommon); }
  [data-rarity="2"] .dot { background: var(--rarity-rare); }
  [data-rarity="3"] .dot { background: var(--rarity-legendary); }
  [data-rarity="4"] .dot { background: var(--rarity-divine); }
  .description { color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .unknown { color: var(--estimated); font-size: var(--text-meta); }
  .note { margin-top: var(--space-2); }
</style>

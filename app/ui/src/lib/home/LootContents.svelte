<script lang="ts">
  import { objectVisual } from '../bridge/visual-index';
  import DebugNote from '../debug/DebugNote.svelte';
  import type { DropView } from '../bindings/DropView';
  import { enchantText, metric, timestamp } from '../runs/story';
  import { emptyHomeVisuals, type HomeVisuals } from './visuals';
  import ObjectLabel from './ObjectLabel.svelte';
  let { drop, visuals = emptyHomeVisuals(), textOnly = false, showEnchant = false, unknownText = '\u2014' }: {
    drop: DropView; visuals?: HomeVisuals; textOnly?: boolean; showEnchant?: boolean; unknownText?: string;
  } = $props();
  let partial = $derived(!drop.contents_known || drop.issues.some(issue =>
    ['midstream', 'decode_failure', 'stream_gap', 'missing_stats', 'resource_limit'].includes(issue)));
  function name(type: number) { return objectVisual(visuals.objects, type)?.name ?? `Item ${type}`; }
</script>

<span class="contents">
  {#if !drop.contents_known}<span>&mdash;</span>{#if unknownText !== '\u2014'}<DebugNote inline text={unknownText} />{/if}{/if}
  {#if drop.contents.length || (drop.contents_known && drop.items.length)}
    <span>Items</span>
    {#if partial}<DebugNote inline text="Partially observed contents; other items may be missing." />{/if}
    {#if drop.contents.length}
      {#each drop.contents as item (`${item.slot}:${item.item_type}`)}
        <span class="item" data-item-key={`${item.slot}:${item.item_type}`}>
          {#if textOnly}{name(item.item_type)}{:else}<ObjectLabel type={item.item_type} kind="item" {visuals} enchant={item.enchant} />{/if}
          × {metric(item.quantity)}{#if showEnchant}{' \u00b7 '}{enchantText(item.enchant)}{/if}
          {#if item.taken_observed_at != null}
            <span class="taken">Taken{' \u00b7 '}{timestamp(item.taken_observed_at)}</span>
          {/if}
        </span>
      {/each}
      {#if drop.contents.every(item => item.taken_observed_at != null)}
        <!-- Retained removals do not establish current occupancy or ownership. -->
        <DebugNote inline text="Removal was observed for every retained item. Current contents are unavailable; an item may have reappeared." />
      {/if}
    {:else}
      {#each drop.items as type, index (`${index}:${type}`)}
        <span class="item">{#if textOnly}{name(type)}{:else}<ObjectLabel {type} kind="item" {visuals} />{/if}{' \u00b7 '}&mdash;</span>
      {/each}
    {/if}
  {:else if drop.contents_known}<span>Empty</span>{#if partial}<DebugNote inline text="Partial observations" />{/if}{/if}
</span>

<style>
  .contents, .item { display: block; min-width: 0; overflow-wrap: anywhere; line-height: var(--leading-body); }
  .item { margin-block: var(--space-2); }
  .taken { color: var(--text-secondary); font-weight: var(--weight-normal, 400); }
</style>

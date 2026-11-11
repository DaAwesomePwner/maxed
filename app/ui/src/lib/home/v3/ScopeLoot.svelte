<script lang="ts">
  import { itemHover } from '../../components/item-hover';
  import { objectVisual } from '../../bridge/visual-index';
  import type { ScopeLootPage } from '../../bindings/ScopeLootPage';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import type { HomeVisuals } from '../visuals';
  import ItemTile from '../../components/ItemTile.svelte';
  import DebugNote from '../../debug/DebugNote.svelte';
  import { formatStat } from '../../styles/format';
  import { lootCount } from '../nexus';
  let { page, pageNumber, visuals, resolveSpriteUrl, enabled, next, first }: {
    page: ScopeLootPage | null; pageNumber: number; visuals: HomeVisuals;
    resolveSpriteUrl: (s: SpriteRef) => string | null; enabled: boolean; next: () => void; first: () => void;
  } = $props();
  let items = $derived(page?.items.slice(0, 32) ?? []);
</script>
<div class="scope-loot">
{#if page === null}<p>—</p>{:else if !items.length}<p>No loot</p>{:else}
  {#each [true, false] as big}
    <div class="loot-row" class:large={big}>
      {#each items.filter(item => (item.tier === 'UT' || item.tier === 'ST') === big) as item (item.item_type)}
        {@const visual = objectVisual(visuals.objects, item.item_type)}
        <span class="tile" class:big data-tier={item.tier ?? ''} data-loot-type={item.item_type} use:itemHover={{ type: item.item_type, enchant: item.best_enchant, name: visual?.name ?? `Item ${item.item_type}`, tier: item.tier, suffix: `×${formatStat(item.count)}`, label: 'Best copy' }}>
          <ItemTile sprite={visual?.sprite ?? null} {resolveSpriteUrl} fit={big ? 40 : 24} alt={visual?.name ?? `Item ${item.item_type}`} enchant={item.best_enchant} />
          {#if item.count > 1}<span class="count">{lootCount(item.count)}</span>{/if}
        </span>
      {/each}
    </div>
  {/each}
{/if}
{#if page && (page.next || pageNumber > 1)}
  <nav aria-label="Loot Pages"><button disabled={!enabled || pageNumber === 1} onclick={first}>First page</button><span>{pageNumber}</span><button disabled={!enabled || !page.next} onclick={next}>Next page</button></nav>
{/if}
<DebugNote text="Counts are retained appearances in bags, not stack sizes or pickups. Item tiers and enchant evidence may be unavailable." />
</div>
<style>
  .scope-loot .loot-row { display: flex; flex-wrap: wrap; align-content: start; gap: var(--space-2); padding-top: var(--space-1); padding-right: var(--space-1); }
  .scope-loot .large { min-height: 48px; padding-bottom: var(--space-1); }
  .scope-loot .tile { position: relative; flex: none; display: grid; place-items: center; width: 28px; height: 28px; border: 1px solid var(--hairline); border-radius: var(--radius-sprite); background: var(--surface-raised); }
  .scope-loot .tile.big { width: 44px; height: 44px; }
  .scope-loot [data-tier="UT"] { border-color: var(--tier-ut); }
  .scope-loot [data-tier="ST"] { border-color: var(--tier-st); }
  .scope-loot .count { position: absolute; top: -6px; right: -2px; max-width: 100%; box-sizing: border-box; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--text-meta); line-height: 16px; padding: 0 2px; border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); background: var(--surface-panel); color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  .scope-loot nav { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-2); margin-top: var(--space-2); font-size: var(--text-meta); color: var(--text-secondary); }
  .scope-loot button { display: inline-flex; align-items: center; height: 32px; padding: 0 var(--space-3); font: inherit; color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-control); cursor: pointer; }
  .scope-loot button:hover:not(:disabled) { background: var(--surface-hover); }
  .scope-loot button:disabled { color: var(--text-disabled); cursor: default; }
  .scope-loot p { margin: 0; color: var(--text-secondary); }
</style>

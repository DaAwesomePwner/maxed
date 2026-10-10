<script lang="ts">
  import { objectVisual } from '../bridge/visual-index';
  import SpriteImage from '../components/SpriteImage.svelte';
  import ItemTile from '../components/ItemTile.svelte';
  import type { EnchantView } from '../bindings/EnchantView';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import { spriteUrl } from '../bridge/visuals';
  import { emptyHomeVisuals, type HomeVisuals } from './visuals';
  let { type, kind, visuals = emptyHomeVisuals(), enchant = null }: {
    type: number | null; kind: 'enemy' | 'item' | 'bag'; visuals?: HomeVisuals; enchant?: EnchantView | null;
  } = $props();
  let visual = $derived(objectVisual(visuals.objects, type));
  let name = $derived(visual?.name ?? `${kind[0].toUpperCase()}${kind.slice(1)}`);
  let sheetUrl = $derived(visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null);
  const resolve = (sprite: SpriteRef) => spriteUrl(visuals.client_fingerprint, sprite);
</script>
<span class="object-label" title={type === null ? undefined : `Catalog ID: ${type}`}>
  {#if kind === 'item'}<ItemTile sprite={visual?.sprite ?? null} resolveSpriteUrl={resolve} scale={2} alt={name} {enchant} />
  {:else}<SpriteImage sprite={visual?.sprite ?? null} {sheetUrl} scale={2} alt={name} />{/if}
  <span>{name}</span>
</span>
<style>
  .object-label { display: inline-flex; align-items: center; gap: var(--space-2); vertical-align: middle; color: inherit; }
</style>

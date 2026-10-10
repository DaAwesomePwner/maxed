<script lang="ts">
  import SpriteImage from './SpriteImage.svelte';
  import type { EnchantView } from '../bindings/EnchantView';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import { localSheetUrl } from '../bridge/visuals';
  import { enchantCount, pipLabel, pipSize, pipSprite } from '../styles/enchant';
  let { sprite, resolveSpriteUrl, alt, scale = 2, fit, enchant = null }: {
    sprite: SpriteRef | null; resolveSpriteUrl?: (sprite: SpriteRef) => string | null;
    alt: string; scale?: number; fit?: number; enchant?: EnchantView | null;
  } = $props();
  let count = $derived(enchantCount(enchant));
  let pip = $derived(pipSprite(count));
  // Matches SpriteImage's frame, which falls back to 8x8 art at the same scale.
  let fitted = $derived(fit !== undefined && Number.isFinite(fit) && Number.isInteger(fit)
    && fit >= 8 && fit <= 256 ? fit : undefined);
  let tile = $derived(fitted ?? Math.max(sprite?.width ?? 8, sprite?.height ?? 8)
    * (Number.isInteger(scale) && scale >= 1 && scale <= 16 ? scale : 4));
  let size = $derived(pipSize(tile));
  let zoom = $derived(size / 16);
  let pipUrl = $derived(pip ? localSheetUrl(resolveSpriteUrl?.(pip) ?? null) : null);
  let failedUrl = $state<string | null>(null);
  function validateTile(event: Event, url: string, rect: SpriteRef) {
    const image = event.currentTarget as HTMLImageElement;
    if (rect.width !== image.naturalWidth || rect.height !== image.naturalHeight) failedUrl = url;
  }
</script>

<span class="item-tile">
  <SpriteImage {sprite} sheetUrl={sprite ? resolveSpriteUrl?.(sprite) ?? null : null} {scale} fit={fitted} {alt} />
  {#if pip && count !== null}
    {@const label = pipLabel(count)}
    {@const art = pipUrl !== null && failedUrl !== pipUrl}
    <span class="pip" class:fallback={!art} role="img" aria-label={label} title={label} data-pip={count}
      style:width={art ? `${size}px` : undefined} style:height={art ? `${size}px` : undefined}>
      {#if art && pipUrl}
        {@const url = pipUrl}
        {@const rect = pip}
        <img src={url} alt="" aria-hidden="true" draggable="false"
          style:transform={`scale(${zoom})`}
          onerror={() => { failedUrl = url; }} onload={(event) => validateTile(event, url, rect)} />
      {:else}
        <!-- Without extracted pip art, one gem per enchant in the rarity colour, two per row. -->
        <span class="gems" data-rarity={count} aria-hidden="true" style:grid-template-columns={`repeat(${Math.min(count, 2)}, auto)`}>{#each Array.from({ length: count }) as _, index (index)}<i></i>{/each}</span>
      {/if}
    </span>
  {/if}
</span>

<style>
  .item-tile { position: relative; display: inline-flex; flex: none; vertical-align: middle; }
  .pip { position: absolute; right: 1px; bottom: 1px; overflow: hidden; pointer-events: none; }
  .pip.fallback { overflow: visible; }
  img { position: absolute; top: 0; left: 0; max-width: none; transform-origin: top left; image-rendering: crisp-edges; image-rendering: pixelated; }
  .gems { display: grid; justify-content: end; }
  .gems i { width: 3px; height: 3px; margin: 1px; transform: rotate(45deg); background: var(--gem); box-shadow: 0 0 0 1px #000; }
  [data-rarity="1"] { --gem: var(--rarity-uncommon); }
  [data-rarity="2"] { --gem: var(--rarity-rare); }
  [data-rarity="3"] { --gem: var(--rarity-legendary); }
  [data-rarity="4"] { --gem: var(--rarity-divine); }
</style>

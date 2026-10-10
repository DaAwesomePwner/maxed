<script lang="ts">
  import type { SpriteRef } from '../bindings/SpriteRef';
  import { localSheetUrl } from '../bridge/visuals';
  let { sprite, sheetUrl, alt, scale = 4, fit }: { sprite: SpriteRef | null; sheetUrl: string | null; alt: string; scale?: number; fit?: number } = $props();
  // Bound both source rectangles and rendered frames before constructing any styles.
  let rectValid = $derived(sprite !== null && [sprite.x, sprite.y, sprite.width, sprite.height].every(Number.isSafeInteger)
    && sprite.x >= 0 && sprite.y >= 0 && sprite.width > 0 && sprite.height > 0
    && sprite.width <= 256 && sprite.height <= 256 && sprite.x + sprite.width <= 16384 && sprite.y + sprite.height <= 16384);
  let box = $derived(fit !== undefined && Number.isFinite(fit) && fit > 0 ? fit : null);
  let trim = $derived(box !== null && rectValid && sprite?.trim
    && [sprite.trim.x, sprite.trim.y, sprite.trim.width, sprite.trim.height].every(Number.isSafeInteger)
    && sprite.trim.x >= 0 && sprite.trim.y >= 0 && sprite.trim.width > 0 && sprite.trim.height > 0
    && sprite.trim.x + sprite.trim.width <= sprite.width
    && sprite.trim.y + sprite.trim.height <= sprite.height ? sprite.trim : null);
  let w = $derived(trim?.width ?? (rectValid ? sprite!.width : 8));
  let h = $derived(trim?.height ?? (rectValid ? sprite!.height : 8));
  let zoom = $derived(box !== null ? Math.max(1, Math.floor(box / Math.max(w, h)))
    : Number.isInteger(scale) && scale >= 1 && scale <= 16 ? scale : 4);
  let width = $derived(box ?? w * zoom);
  let height = $derived(box ?? h * zoom);
  let localUrl = $derived(localSheetUrl(sheetUrl));
  let sourceKey = $derived(`${localUrl}|${sprite?.key}|${sprite?.x}|${sprite?.y}|${sprite?.width}|${sprite?.height}`);
  let failedKey = $state<string | null>(null);
  let loadedKey = $state<string | null>(null);
  let imageUrl = $derived(localUrl);
  let available = $derived(rectValid && localUrl !== null && failedKey !== sourceKey);
  function validateTile(event: Event, key: string, rect: SpriteRef) {
    if (key !== sourceKey) return;
    const image = event.currentTarget as HTMLImageElement;
    if (rect.width !== image.naturalWidth || rect.height !== image.naturalHeight) failedKey = key;
    else loadedKey = key;
  }
</script>

<span class="frame" role="img" aria-label={alt} style:width={`${width}px`} style:height={`${height}px`} data-fallback={!available} data-pending={available && loadedKey !== sourceKey}>
  {#if available && sprite && imageUrl}
    {#key sourceKey}
      {@const key = sourceKey}
      {@const rect = sprite}
      <span class="crop" style:width={`${w * zoom}px`} style:height={`${h * zoom}px`}
        style:left={`${(width - w * zoom) / 2}px`} style:top={`${(height - h * zoom) / 2}px`}>
        <img class="sprite" src={imageUrl} alt="" aria-hidden="true" draggable="false"
          style:visibility={loadedKey === sourceKey ? 'visible' : 'hidden'}
          style:transform={`translate(${-(trim?.x ?? 0) * zoom}px, ${-(trim?.y ?? 0) * zoom}px) scale(${zoom})`}
          onerror={() => { if (key === sourceKey) failedKey = key; }} onload={(event) => validateTile(event, key, rect)} />
      </span>
    {/key}
  {:else if !available}<span class="fallback" aria-hidden="true">?</span>{/if}
</span>

<style>
  .frame { display: inline-block; position: relative; flex: none; overflow: hidden; vertical-align: middle; border-radius: var(--radius-sprite); background: var(--surface-raised); }
  img { position: absolute; top: 0; left: 0; max-width: none; transform-origin: top left; image-rendering: crisp-edges; image-rendering: pixelated; }
  .crop { position: absolute; display: block; overflow: hidden; }
  .fallback { display: grid; place-items: center; width: 100%; height: 100%; color: var(--text-muted); font: var(--weight-semibold) var(--text-label)/var(--leading-label) var(--font-ui); }
</style>

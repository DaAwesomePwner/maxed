<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { clockTime } from '../nexus';
  import type { HomeVisuals } from '../visuals';
  import type { WhiteBannerState } from './white-queue';

  let { state, visuals, resolveSpriteUrl, itemHref }: {
    state: WhiteBannerState; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string;
    itemHref: (itemType: number) => string;
  } = $props();
  const current = $derived(state.current);
  const bagSprite = $derived(objectVisual(visuals.objects, current?.bagType)?.sprite ?? null);
  const itemSprite = $derived(objectVisual(visuals.objects, current?.itemType)?.sprite ?? null);
</script>

<div class="white-banner-region" role="status" aria-live="polite" aria-atomic="true">
{#if current}
  {#key current.dropId}
    <div class="white-banner">
      <div class="pictures">
        <span class="bag-glow"><SpriteImage sprite={bagSprite} sheetUrl={bagSprite ? resolveSpriteUrl(bagSprite) : null} fit={48} alt="White bag" /></span>
        <SpriteImage sprite={itemSprite} sheetUrl={itemSprite ? resolveSpriteUrl(itemSprite) : null} fit={64} alt={current.itemName} />
      </div>
      <div class="copy">
        <p class="label">White bag</p>
        <h2>{#if current.itemType !== null}<a href={itemHref(current.itemType)}>{current.itemName}</a>{:else}{current.itemName}{/if}</h2>
        {#if current.streak?.kind === 'ended'}
          <p>Ended a {current.streak.runs}-run {current.areaName} dry streak</p>
        {:else if current.streak?.kind === 'first'}
          <p>No {current.areaName} white in {current.streak.runs} runs before this</p>
        {/if}
        <p class="meta">{current.sourceName ?? '—'} · {clockTime(current.at) ?? '—'}</p>
        {#if state.overflow > 0}<p class="overflow">+{state.overflow} more whites</p>{/if}
      </div>
    </div>
  {/key}
{/if}
</div>

<style>
  /* A static celebratory banner: white-bag border, one short entrance, nothing loops or glows. */
  .white-banner-region { min-width: 0; }
  .white-banner { position: relative; box-sizing: border-box;
    display: flex; flex-wrap: wrap; align-items: center; justify-content: center; gap: var(--space-5);
    min-width: 0; min-height: 100%; padding: var(--space-6); border: 1px solid var(--bag-white);
    border-radius: var(--radius-card); background: var(--surface-panel); color: var(--text-primary);
    font-family: var(--font-home); animation: maxed-enter var(--motion-page) var(--motion-ease) both; }
  .pictures { display: flex; align-items: center; gap: var(--space-3); }
  .bag-glow { display: flex; border-radius: var(--radius-sprite); }
  .copy { min-width: 0; overflow-wrap: anywhere; }
  .label { color: var(--bag-white); font-size: var(--text-caption); line-height: var(--leading-caption); font-weight: var(--weight-medium); }
  h2 { margin: var(--space-1) 0; font-size: var(--text-hero); line-height: var(--leading-hero); font-weight: var(--weight-medium); }
  a { color: inherit; text-decoration: none; text-underline-offset: 3px; }
  a::after { content: ''; position: absolute; inset: 0; }
  a:hover { text-decoration: underline; }
  .white-banner:has(a:focus-visible) { outline: 2px solid var(--focus-ring); outline-offset: -3px; }
  p { margin: var(--space-1) 0 0; font-size: var(--text-caption); line-height: var(--leading-caption); }
  .meta { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .overflow { color: var(--bag-white); font-weight: var(--weight-medium); }
  @media (prefers-reduced-motion: reduce) {
    .white-banner { animation: none; }
  }
</style>

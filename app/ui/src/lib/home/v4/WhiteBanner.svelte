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
      <span class="rays" aria-hidden="true"></span>
      <div class="pictures">
        <span class="bag-glow"><SpriteImage sprite={bagSprite} sheetUrl={bagSprite ? resolveSpriteUrl(bagSprite) : null} fit={48} alt="White bag" /></span>
        <SpriteImage sprite={itemSprite} sheetUrl={itemSprite ? resolveSpriteUrl(itemSprite) : null} fit={64} alt={current.itemName} />
      </div>
      <div class="copy">
        <p class="label">WHITE BAG</p>
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
  .white-banner-region { min-width: 0; }
  .white-banner { position: relative; isolation: isolate; overflow: hidden; box-sizing: border-box;
    display: flex; flex-wrap: wrap; align-items: center; justify-content: center; gap: 20px;
    min-width: 0; min-height: 100%; padding: 24px; border: 1px solid var(--bag-white);
    border-radius: var(--radius-card); background: var(--surface-panel); color: var(--text-primary);
    font-family: var(--font-home); animation: maxed-enter var(--motion-page) var(--motion-ease) both; }
  .rays { position: absolute; z-index: -1; inset: 0; overflow: hidden; pointer-events: none; }
  .rays::before { content: ''; position: absolute; inset: -150%;
    background: repeating-conic-gradient(from 0deg, #ffffff0d 0deg 12deg, transparent 12deg 30deg);
    animation: white-rays 24s linear infinite; }
  .pictures { display: flex; align-items: center; gap: 12px; }
  .bag-glow { display: flex; border-radius: var(--radius-sprite); box-shadow: var(--glow-white);
    animation: white-glow 2s var(--motion-ease) infinite alternate; }
  .copy { min-width: 0; overflow-wrap: anywhere; }
  .label { color: var(--bag-white); font-size: 12px; font-weight: 600; letter-spacing: .12em; }
  h2 { margin: 6px 0; font: 600 24px/1.2 var(--font-home-display); }
  a { color: inherit; text-decoration: none; }
  a::after { content: ''; position: absolute; inset: 0; }
  a:hover { text-decoration: underline; }
  .white-banner:has(a:focus-visible) { outline: 2px solid var(--focus-ring); outline-offset: -3px; }
  p { margin: 6px 0 0; font-size: 13px; }
  .meta { color: var(--text-secondary); font-size: 12px; }
  .overflow { color: var(--bag-white); font-weight: 600; }
  @keyframes white-rays { to { transform: rotate(360deg); } }
  @keyframes white-glow { from { box-shadow: 0 0 4px #ffffff20; } to { box-shadow: var(--glow-white); } }
  @media (prefers-reduced-motion: reduce) {
    .white-banner, .rays::before, .bag-glow { animation: none; }
  }
</style>

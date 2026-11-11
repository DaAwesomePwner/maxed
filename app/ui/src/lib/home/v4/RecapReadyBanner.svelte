<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import Badge from '../../components/Badge.svelte';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { formatElapsed } from '../../styles/format';
  import { bestItem } from '../v3/loot-rank';
  import type { HomeVisuals } from '../visuals';
  import type { RecapState } from './recap';
  import type { createRecapPresentation } from './recap-presentation';
  import type { RunRecapView, VisitRecapView } from './recap-view';

  let { recap, view, visuals, portalType, resolveSpriteUrl, presentation }: {
    recap: RecapState; view: RunRecapView | VisitRecapView | null; visuals: HomeVisuals;
    portalType: number | null; resolveSpriteUrl: (sprite: SpriteRef) => string;
    presentation: ReturnType<typeof createRecapPresentation>;
  } = $props();
  let region: HTMLElement;
  const labels = { cleared: 'Cleared', died: 'Died', left: 'Left early', unknown: 'Outcome unknown' };
  const title = $derived(view?.title ?? recap.summary.title);
  const outcome = $derived(recap.summary.kind === 'visit' ? 'Realm visit' : labels[recap.summary.outcome ?? 'unknown']);
  const clearTime = $derived(view?.kind === 'run' ? view.clearTime
    : recap.summary.outcome === 'cleared' && recap.summary.completedAt
      ? formatElapsed(recap.summary.startedAt, recap.summary.completedAt, true) : null);
  const portal = $derived(objectVisual(visuals.objects, recap.summary.kind === 'visit' ? 1796 : portalType));
  const drop = $derived(view?.best?.drop ?? recap.summary.best?.drop);
  const item = $derived(drop ? bestItem(drop, visuals) : null);
  const loot = $derived(objectVisual(visuals.objects, item?.itemType ?? drop?.bag_type));
  function blur(event: FocusEvent) {
    if (!(event.relatedTarget instanceof Node) || !region.contains(event.relatedTarget)) presentation.setFocused(false);
  }
  onDestroy(() => {
    presentation.setHovered(false); presentation.setFocused(false);
  });
</script>

<section class="v4-card recap-ready" aria-label="Recap ready" bind:this={region}
  onmouseenter={() => presentation.setHovered(true)} onmouseleave={() => presentation.setHovered(false)}
  onfocusin={() => presentation.setFocused(true)} onfocusout={blur}>
  <SpriteImage sprite={portal?.sprite ?? null} sheetUrl={portal?.sprite ? resolveSpriteUrl(portal.sprite) : null} fit={32} alt="" />
  <div class="recap-ready-copy">
    <p class="v4-meta">Recap ready</p>
    <h2>{title} <span class="recap-outcome" data-outcome={recap.summary.kind === 'visit' ? 'visit' : recap.summary.outcome ?? 'unknown'}>· {outcome}</span></h2>
    {#if clearTime !== null}
      <p class="recap-ready-time">{clearTime}{#if view?.kind === 'run' && view.pb}<Badge label="PB" tone="success" />{/if}</p>
    {:else if view?.kind === 'visit'}<p>{view.timeHere}</p>{/if}
  </div>
  {#if drop}<div class="recap-ready-loot">
    <SpriteImage sprite={loot?.sprite ?? null} sheetUrl={loot?.sprite ? resolveSpriteUrl(loot.sprite) : null} fit={28} alt="" />
    <span>{loot?.name ?? 'Best drop observed'}</span>
  </div>{/if}
  <div class="recap-ready-actions">
    <button type="button" onclick={presentation.open}>View recap</button>
    <button type="button" aria-label="Dismiss recap banner" onclick={presentation.hide}>×</button>
  </div>
</section>

<style>
  .recap-ready { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-3); padding: var(--space-3) var(--space-4); }
  .recap-ready-copy { flex: 1 1 180px; min-width: 0; overflow-wrap: anywhere; }
  h2 { margin: 2px 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  p { margin: 0; font-variant-numeric: tabular-nums; }
  .recap-ready-time { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-caption); line-height: var(--leading-caption); }
  .recap-outcome { color: var(--text-secondary); font-size: var(--text-caption); font-weight: var(--weight-normal); }
  .recap-outcome[data-outcome="cleared"] { color: var(--success); }
  .recap-outcome[data-outcome="died"] { color: var(--danger); }
  .recap-ready-loot { display: flex; align-items: center; gap: var(--space-2); max-width: 100%; font-size: var(--text-meta); line-height: var(--leading-meta); }
  .recap-ready-loot span { min-width: 0; overflow-wrap: anywhere; }
  .recap-ready-actions { display: flex; gap: var(--space-2); }
  button { box-sizing: border-box; min-height: var(--control-dense); padding: var(--space-1) var(--space-3); border: 1px solid var(--border-strong);
    border-radius: var(--radius-control); background: var(--surface-raised); color: var(--text-primary); font: inherit;
    font-size: var(--text-caption); line-height: var(--leading-caption); }
  button[aria-label="Dismiss recap banner"] { min-width: var(--control-dense); border-color: transparent; background: transparent; color: var(--text-secondary); }
  button[aria-label="Dismiss recap banner"]:hover { background: var(--surface-hover); color: var(--text-primary); }
  button:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
</style>

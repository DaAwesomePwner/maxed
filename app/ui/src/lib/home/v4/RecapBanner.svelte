<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import { itemHover } from '../../components/item-hover';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { routeHref } from '../../navigation/routes';
  import { bestItem } from '../v3/loot-rank';
  import type { HomeVisuals } from '../visuals';
  import type { RunRecapView, VisitRecapView } from './recap-view';

  let { view, visuals, portalType, resolveSpriteUrl }: {
    view: RunRecapView | VisitRecapView; visuals: HomeVisuals; portalType: number | null; resolveSpriteUrl: (s: SpriteRef) => string;
  } = $props();
  const portal = $derived(objectVisual(visuals.objects, portalType));
  const drop = $derived(view.best?.drop);
  const item = $derived(drop ? bestItem(drop, visuals) : null);
  const bagVisual = $derived(objectVisual(visuals.objects, drop?.bag_type));
  const itemVisual = $derived(objectVisual(visuals.objects, item?.itemType));
</script>

<header class="v4-card recap-banner" data-outcome={view.kind === 'run' ? view.outcome ?? 'unknown' : undefined}>
  <SpriteImage sprite={portal?.sprite ?? null} sheetUrl={portal?.sprite ? resolveSpriteUrl(portal.sprite) : null} fit={48} alt="" />
  <div class="recap-banner-copy">
    <h2>{view.title} · <span class="recap-outcome">{view.kind === 'visit' ? 'REALM VISIT' : view.outcomeLabel}</span></h2>
    {#if view.kind === 'visit'}
      <p class="v4-meta">Time here</p><div class="recap-clear-time">{view.timeHere}</div>
    {:else if view.clearTime !== null}<div class="recap-clear-time">{view.clearTime}{#if view.pb}<span class="recap-tag">PB</span>{/if}</div>{/if}
    <p class="v4-meta">{view.meta.join(' · ')}</p>
    {#if view.chain}<p class="v4-meta">{view.chain.label}</p>{/if}
  </div>
  {#if drop}
    <div class="recap-banner-loot">
      <SpriteImage sprite={bagVisual?.sprite ?? null} sheetUrl={bagVisual?.sprite ? resolveSpriteUrl(bagVisual.sprite) : null} fit={40} alt={item ? bagVisual?.name ?? '—' : ''} />
      <div>
        {#if item}
          <a class="recap-item" href={routeHref({ kind: 'item', type: item.itemType })}
            use:itemHover={{ type: item.itemType, enchant: item.enchant, name: itemVisual?.name ?? '—', tier: item.tier }}>
            <SpriteImage sprite={itemVisual?.sprite ?? null} sheetUrl={itemVisual?.sprite ? resolveSpriteUrl(itemVisual.sprite) : null} fit={40} alt="" />
            <span>{itemVisual?.name ?? '—'}</span>
          </a>
        {:else}<span>{bagVisual?.name ?? '—'}</span>{/if}
        {#if view.kind === 'run' && view.streakNote}<p class="v4-meta">{view.streakNote}</p>{/if}
      </div>
    </div>
  {/if}
</header>

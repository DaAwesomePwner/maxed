<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import { itemHover } from '../../components/item-hover';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { routeHref } from '../../navigation/routes';
  import type { HomeVisuals } from '../visuals';
  import type { RecapChip } from './recap-view';

  let { chips, visuals, resolveSpriteUrl, label = 'Run highlights' }: {
    chips: RecapChip[]; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string;
    label?: string;
  } = $props();
</script>

{#if chips.length}
  <ul class="recap-chips" aria-label={label}>
    {#each chips as chip}<li data-kind={chip.kind}><span>{chip.text}</span>
      {#each chip.itemTypes ?? [] as type}
        {@const visual = objectVisual(visuals.objects, type)}
        <a href={routeHref({ kind: 'item', type })} use:itemHover={{ type, name: visual?.name ?? '—' }}>
          <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} fit={24} alt={visual?.name ?? '—'} />
        </a>
      {/each}
    </li>{/each}
  </ul>
{/if}

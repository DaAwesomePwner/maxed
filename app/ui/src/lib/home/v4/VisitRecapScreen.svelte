<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { clockTime } from '../nexus';
  import type { HomeVisuals } from '../visuals';
  import BagList from '../v3/BagList.svelte';
  import BestBagCard from '../v3/BestBagCard.svelte';
  import LootTally from '../v3/LootTally.svelte';
  import { bestItem, homeLoot } from '../v3/loot-rank';
  import RecapBanner from './RecapBanner.svelte';
  import RecapTiles from './RecapTiles.svelte';
  import RecapChips from './RecapChips.svelte';
  import VisitEventsCard from './VisitEventsCard.svelte';
  import type { VisitRecapView } from './recap-view';
  import './screens.css';

  let { view, visuals, resolveSpriteUrl, onDismiss, actions }: {
    view: VisitRecapView; visuals: HomeVisuals;
    resolveSpriteUrl: (s: SpriteRef) => string; onDismiss: () => void; actions?: Snippet;
  } = $props();
  const headingId = $props.id();
  const best = $derived(view.best ? { drop: view.best.drop, item: bestItem(view.best.drop, visuals) } : null);
  const bags = $derived(view.drops.filter(drop => drop.id !== best?.drop.id));
  const counts = $derived(homeLoot(view.drops, visuals).counts);
  const partialEvents = $derived(view.tiles.some(tile => tile.key === 'events' && tile.partial));
  const clock = (at: string) => clockTime(at) ?? '—';
</script>

<section class="v4-screen recap-screen visit-recap-screen" aria-label="Recap">
  <RecapBanner {view} {visuals} portalType={1796} {resolveSpriteUrl} />
  <RecapTiles tiles={view.tiles} />
  <div class="recap-columns">
    <VisitEventsCard rows={view.events} partial={partialEvents} {visuals} {resolveSpriteUrl} />
    <section class="v4-card v4-loot" aria-labelledby={headingId}>
      <h3 id={headingId}>Loot this visit</h3>
      {#if view.dropsPartial}<p class="recap-note">Partial loot history</p>{/if}
      <BestBagCard {best} {visuals} {resolveSpriteUrl} meta={best ? clock(best.drop.at) : ''} scope={view.visitId} />
      <div class="v4-bags">{#key view.visitId}<BagList drops={bags} {visuals} {resolveSpriteUrl} {clock} scroll newestFirst />{/key}</div>
      <LootTally {...counts} />
    </section>
  </div>
  <RecapChips chips={view.chips} label="Visit highlights" {visuals} {resolveSpriteUrl} />
  <footer class="recap-actions">
    <div>{@render actions?.()}<button type="button" onclick={onDismiss}>Back</button></div>
    <p>Available from Last recap until the next recap</p>
  </footer>
</section>

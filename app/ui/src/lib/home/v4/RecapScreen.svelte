<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { routeHref } from '../../navigation/routes';
  import { clockTime } from '../nexus';
  import type { HomeVisuals } from '../visuals';
  import BagList from '../v3/BagList.svelte';
  import BestBagCard from '../v3/BestBagCard.svelte';
  import LootTally from '../v3/LootTally.svelte';
  import { bestItem, homeLoot } from '../v3/loot-rank';
  import ChainStrip from './ChainStrip.svelte';
  import DeathBanner from './DeathBanner.svelte';
  import DryStreakRow from './DryStreakRow.svelte';
  import FightByFight from './FightByFight.svelte';
  import RecapBanner from './RecapBanner.svelte';
  import RecapChips from './RecapChips.svelte';
  import RecapTiles from './RecapTiles.svelte';
  import type { RunRecapView } from './recap-view';
  import './screens.css';

  let { view, visuals, portalType, resolveSpriteUrl, onDismiss, actions, recordsUnavailable = false, onRetry }: {
    view: RunRecapView; visuals: HomeVisuals; portalType: number | null;
    resolveSpriteUrl: (s: SpriteRef) => string; onDismiss: () => void; actions?: Snippet;
    recordsUnavailable?: boolean; onRetry?: () => void;
  } = $props();
  const headingId = $props.id();
  const href = $derived(routeHref({ kind: 'run', id: view.runId }));
  const best = $derived(view.best ? { drop: view.best.drop, item: bestItem(view.best.drop, visuals) } : null);
  const bags = $derived(view.drops.filter(drop => drop.id !== best?.drop.id));
  const counts = $derived(homeLoot(view.drops, visuals).counts);
  const clock = (at: string) => clockTime(at) ?? '—';
</script>

<section class="v4-screen recap-screen" aria-label="Recap">
  {#if view.died !== null}
    <DeathBanner {view} {visuals} {resolveSpriteUrl} />
  {:else}<RecapBanner {view} {visuals} {portalType} {resolveSpriteUrl} />{/if}
  <RecapTiles tiles={view.tiles} />
  <div class="recap-columns">
    <div class="v4-main">
      <FightByFight rows={view.fights} partial={view.fightsPartial} {visuals} {resolveSpriteUrl} />
      {#if view.chain}<ChainStrip chain={view.chain} {visuals} {resolveSpriteUrl} />{/if}
    </div>
    <section class="v4-card v4-loot" aria-labelledby={headingId}>
      <h3 id={headingId}>Loot</h3>
      <DryStreakRow dungeon={view.title} stats={view.streak} ended={view.streakEnded} />
      <BestBagCard {best} {visuals} {resolveSpriteUrl} meta={best ? clock(best.drop.at) : ''} scope={view.runId} />
      <div class="v4-bags">{#key view.runId}<BagList drops={bags} {visuals} {resolveSpriteUrl} {clock} scroll newestFirst />{/key}</div>
      <LootTally {...counts} />
    </section>
  </div>
  <RecapChips chips={view.chips} {visuals} {resolveSpriteUrl} />
  {#if recordsUnavailable}<div class="recap-actions"><p>Records unavailable</p><button type="button" onclick={onRetry}>Retry</button></div>{/if}
  <section class="recap-party" aria-label="Party leaders">
    {#each view.party.top as player}<span><strong>{player.name}</strong> <span>{player.share}</span></span>{/each}
    {#if view.party.more > 0}<span>{view.party.more} more</span>{/if}
    <a {href}>Full party meter ›</a>
  </section>
  <footer class="recap-actions">
    <div>{@render actions?.()}<a {href}>Open run story ›</a><button type="button" onclick={onDismiss}>Back</button></div>
    <p>Available from Last recap until the next recap</p>
  </footer>
</section>

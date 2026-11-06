<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { knownNumber } from '../evidence';
  import { clockTime } from '../nexus';
  import type { HomeVisuals } from '../visuals';
  import BagList from '../v3/BagList.svelte';
  import { bagsBeforeDeath, killerLabel } from './death';
  import HpTrace from './HpTrace.svelte';
  import RecapTiles from './RecapTiles.svelte';
  import type { RunRecapView } from './recap-view';

  let { view, visuals, resolveSpriteUrl }: {
    view: RunRecapView; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string;
  } = $props();
  const killer = $derived(killerLabel(view.death, visuals.objects));
  const unverified = $derived(!killer.verified && Boolean(view.death?.killed_by?.trim()));
  const killerVisual = $derived(objectVisual(visuals.objects, killer.spriteType));
  const className = $derived(objectVisual(visuals.objects, view.character?.class_id)?.name?.trim() || '—');
  const maxed = $derived(knownNumber(view.character?.maxed));
  const fame = $derived(knownNumber(view.death?.fame));
  const meta = $derived(`${view.character?.name?.trim() || '—'} · ${className} · ${maxed === null ? '—' : `${maxed}/8`} · ${fame ?? '—'} fame`);
  const bags = $derived(view.died ? view.died.at === null ? view.drops : bagsBeforeDeath(view.drops, view.died.at) : []);
  const clock = (at: string) => clockTime(at) ?? '—';
</script>

<header class="v4-card death-banner" data-outcome="died">
  <div class="death-copy">
    <h2>{view.title} · DIED AT {view.died?.elapsed ?? '—'}</h2>
    <div class="killer">
      {#if killer.spriteType !== null}
        <SpriteImage sprite={killerVisual?.sprite ?? null} sheetUrl={killerVisual?.sprite ? resolveSpriteUrl(killerVisual.sprite) : null}
          fit={40} alt="" />
      {/if}
      <p title={unverified ? 'Name from the death message; not matched to the catalog' : undefined}>Killed by {killer.text}{#if unverified}{' '}<span class="unverified">unverified</span>{/if}</p>
    </div>
    <p class="death-meta">{meta}</p>
    <HpTrace samples={view.death?.hp_trace ?? []} diedAt={view.died?.at ?? ''} />
  </div>
  <div class="death-right">
    <RecapTiles tiles={view.tiles.filter(tile => tile.key === 'dps' || tile.key === 'rank')} />
    <section aria-label="Bags before death">
      <h3>Bags before death</h3>
      <div class="death-bags">{#key view.runId}<BagList drops={bags} {visuals} {resolveSpriteUrl} {clock} scroll newestFirst />{/key}</div>
    </section>
  </div>
</header>

<style>
  .death-banner { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 24px; padding: 24px;
    border-color: var(--danger); background: linear-gradient(110deg, color-mix(in srgb, var(--danger) 18%, var(--surface-panel)), var(--surface-panel)); }
  .death-copy, .death-right { min-width: 0; }
  .death-copy { overflow-wrap: anywhere; }
  h2 { margin: 0; color: var(--danger); font: 600 18px/1.3 var(--font-home-display); }
  .killer { display: flex; align-items: center; gap: 10px; margin-top: 16px; }
  .killer p { margin: 0; min-width: 0; font: 600 20px/1.3 var(--font-home-display); }
  .unverified { color: var(--text-secondary); font-size: 11px; font-weight: 400; }
  .death-meta { margin: 12px 0 0; color: var(--text-secondary); font-size: 12px; }
  .death-right :global(.recap-tiles) { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  h3 { margin: 16px 0 8px; font-size: 12px; font-weight: 500; color: var(--text-secondary); }
  .death-bags { height: 114px; min-width: 0; }
  @media (max-width: 950px) { .death-banner { grid-template-columns: minmax(0, 1fr); } }
  @media (max-width: 540px) { .death-banner { padding: 16px; } }
</style>

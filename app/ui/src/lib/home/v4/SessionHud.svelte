<script lang="ts">
  import type { HomeHud } from '../../bindings/HomeHud';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { itemHover } from '../../components/item-hover';
  import type { HomeVisuals } from '../visuals';
  import { overflow, potionNames, potionStat, rankText, sessionClock, signedPercent, WHITE_BAG_TYPE } from './hud';

  let { hud, sessionStarted, sessionEnded = null, nowMs, visuals, resolveSpriteUrl, lootHref }: {
    hud: HomeHud | null; sessionStarted: string | null; sessionEnded?: string | null; nowMs: number;
    visuals: HomeVisuals; resolveSpriteUrl: (sprite: SpriteRef) => string; lootHref: string;
  } = $props();
  const comparison = $derived(hud?.combat?.vs_party ?? null);
  const items = $derived(overflow(hud?.ut_st ?? [], 3));
  const moreItems = $derived(Math.max(0, (hud?.ut_st_count ?? 0) - items.shown.length));
  const potions = $derived([...(hud?.potions ?? [])].sort((a, b) => b.count - a.count));
  const pots = $derived(overflow(potions, 3));
  const morePots = $derived(potions.slice(3).reduce((sum, potion) => sum + potion.count, 0));
  const white = $derived(visuals.objects.find(object => object.object_type === WHITE_BAG_TYPE));
  const fame = $derived(hud?.fame_per_hour == null || !Number.isFinite(hud.fame_per_hour)
    ? '—' : Math.round(hud.fame_per_hour).toLocaleString());
</script>

<dl class="session-hud" aria-label="Session HUD">
  <div data-hud="fame"><dt>Fame/h</dt><dd>{fame}</dd></div>
  <div data-hud="combat">
    <dt>You vs party avg</dt>
    <dd><strong class:positive={comparison !== null && Number.isFinite(comparison) && comparison > 0 && Math.round(Math.abs(comparison) * 100) > 0}
      class:negative={comparison !== null && Number.isFinite(comparison) && comparison < 0 && Math.round(Math.abs(comparison) * 100) > 0}>{signedPercent(comparison)}</strong>
      <span class="sr-only">Average rank</span>
      <span class="rank">{rankText(hud?.combat?.avg_rank ?? null)}</span></dd>
  </div>
  <div data-hud="whites"><dt>Whites</dt><dd><a href={lootHref}>
    {#if white?.sprite}<SpriteImage sprite={white.sprite} sheetUrl={resolveSpriteUrl(white.sprite)} alt="White bag" fit={20} />{/if}
    <span>{hud?.whites ?? '—'}</span>
  </a></dd></div>
  <div data-hud="ut-st"><dt>UT/ST</dt><dd>
    {#each items.shown as type (type)}
      {@const visual = visuals.objects.find(object => object.object_type === type)}
      <a href={lootHref} use:itemHover={{ type, name: visual?.name ?? undefined, tier: visual?.item_tier }}>
        <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null}
          alt={visual?.name ?? `Item ${type}`} fit={20} />
      </a>
    {/each}
    {#if moreItems > 0}<a href={lootHref}>+{moreItems}</a>
    {:else if items.shown.length === 0}<a href={lootHref}>{hud ? 0 : '—'}</a>{/if}
  </dd></div>
  <div data-hud="pots"><dt>Pots</dt><dd><a href={lootHref}>
    {#each pots.shown as potion (potion.stat)}
      {@const visual = visuals.objects.find(object => object.sprite && potionStat(object.name) === potion.stat)}
      <span class="potion">
        {#if visual?.sprite}<SpriteImage sprite={visual.sprite} sheetUrl={resolveSpriteUrl(visual.sprite)} alt={potionNames[potion.stat]} fit={20} />
        {:else}<span>{potionNames[potion.stat]}</span>{/if}
        <span>{potion.count}</span>
      </span>
    {/each}
    {#if morePots > 0}<span>+{morePots}</span>
    {:else if pots.shown.length === 0}<span>{hud ? 0 : '—'}</span>{/if}
  </a></dd></div>
  <div data-hud="session"><dt>Session</dt><dd>{sessionClock(sessionStarted, nowMs, sessionEnded)}</dd></div>
</dl>

<style>
  .session-hud { display: flex; align-items: center; gap: 18px; box-sizing: border-box; height: 38px;
    flex: 0 0 38px; min-width: 0; margin: 0; padding: 0 12px; overflow-x: auto; white-space: nowrap;
    border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface-panel);
    font: 12px/1.2 var(--font-home); font-variant-numeric: tabular-nums; }
  [data-hud], dd, a, .potion { display: flex; align-items: center; gap: 6px; }
  [data-hud] { flex: none; }
  dt, .rank { color: var(--text-secondary); }
  dd { margin: 0; color: var(--text-primary); }
  strong { font-weight: 600; }
  .positive { color: var(--success); }
  .negative { color: var(--danger); }
  a { color: inherit; text-decoration: none; min-height: 28px; }
  a:hover { color: var(--text-primary); text-decoration: underline; }
  a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 1px; }
  [data-hud="session"] { margin-left: auto; }
  @media (max-width: 950px) {
    .session-hud { flex-wrap: wrap; height: auto; min-height: 38px; flex-basis: auto; gap: 4px 12px; padding: 4px 8px; }
    [data-hud], dd, a, .potion { gap: 4px; }
    [data-hud] { flex-direction: column; align-items: flex-start; gap: 2px; }
    dd { min-height: 28px; }
  }
</style>

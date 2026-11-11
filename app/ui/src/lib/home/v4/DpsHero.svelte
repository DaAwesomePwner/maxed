<script lang="ts">
  import type { DamageBucket } from '../../bindings/DamageBucket';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { formatDps, formatShare } from '../../styles/format';
  import { relative, type Usual } from './hero';
  import YouVsPartyChart from './YouVsPartyChart.svelte';

  let { label, live, dps, rank, players, share, bestHit, party, usual, buckets, height = 160, target = null, resolveSpriteUrl }: {
    label: string; live: boolean; dps: number | null; rank: number | null; players: number | null;
    share: number | null; bestHit: number | null; party: { avg: number; players: number } | null;
    usual: Usual; buckets: DamageBucket[]; height?: number;
    target?: { name: string; kind: string; elapsed: string; sprite: SpriteRef | null } | null;
    resolveSpriteUrl?: (sprite: SpriteRef) => string;
  } = $props();
  const vsParty = $derived(relative(dps, party && party.players > 0 ? party.avg : null));
  const vsUsual = $derived(relative(dps, usual.kind === 'value' ? usual.dps : null));
  const rankText = $derived(rank !== null && Number.isInteger(rank) && rank > 0 ? `#${rank}` : '—');
  const playersText = $derived(players !== null && Number.isInteger(players) && players > 0 ? String(players) : '—');
  const percent = (value: number) => Math.round(Math.abs(value) * 100);
</script>

<section class="dps-hero" aria-label={label} data-live={live}>
  <div class="summary">
    <header><h2>{label}</h2></header>
    {#if target}
      <div class="target">
        <SpriteImage sprite={target.sprite} sheetUrl={target.sprite && resolveSpriteUrl ? resolveSpriteUrl(target.sprite) : null}
          alt={target.name} fit={32} />
        <div><strong>{target.name}</strong><span class="target-meta"><span>{target.kind}</span><span>{target.elapsed}</span></span></div>
      </div>
    {/if}
    <span class="sr-only">Your DPS</span>
    <div class="dps">{formatDps(dps)}</div>
    <dl>
      <div><dt>Rank</dt><dd>{rankText} of {playersText}</dd></div>
      <div><dt>Share</dt><dd>{formatShare(share)}</dd></div>
      <div><dt>Best hit</dt><dd>{formatDps(bestHit)}</dd></div>
    </dl>
    <p class:positive={vsParty !== null && percent(vsParty) > 0 && vsParty >= 0} class:negative={vsParty !== null && percent(vsParty) > 0 && vsParty < 0}
      title={`vs party (${party?.players ?? 0} players)`}>
      {#if vsParty === null}vs party —
      {:else if percent(vsParty) === 0}0% vs party average
      {:else}{vsParty < 0 ? '▼' : '▲'} {percent(vsParty)}% {vsParty < 0 ? 'below' : 'above'} party average{/if}
    </p>
    {#if usual.kind === 'learning'}<p class="usual">learning ({usual.fights} fights)</p>
    {:else if usual.kind === 'value'}
      <p class="usual">{vsUsual === null ? 'vs your usual —' : percent(vsUsual) === 0 ? '0% vs your usual' : `${vsUsual < 0 ? '▼' : '▲'} ${percent(vsUsual)}% vs your usual`} · avg {formatDps(usual.dps)} · {usual.fights} fights</p>
    {/if}
  </div>
  <YouVsPartyChart {buckets} {height} />
</section>

<style>
  /* The screen's one bright moment: your live DPS in gold on a quiet graphite card. */
  .dps-hero { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 240px), 1fr)); align-items: center;
    gap: var(--space-5); padding: var(--space-5); min-width: 0; border: 1px solid var(--hairline); border-radius: var(--radius-card);
    background: var(--surface-panel); color: var(--text-primary); font-family: var(--font-home); font-variant-numeric: tabular-nums; }
  .summary { min-width: 0; overflow-wrap: anywhere; }
  header { display: flex; align-items: center; gap: var(--space-2); }
  h2 { margin: 0; color: var(--text-secondary); font-size: var(--text-caption); line-height: var(--leading-caption); font-weight: var(--weight-medium); }
  .dps { margin: var(--space-1) 0 var(--space-3); color: var(--gold); font-size: var(--text-number); line-height: var(--leading-number);
    font-weight: var(--weight-medium); font-variant-numeric: tabular-nums; }
  dl { display: flex; flex-wrap: wrap; gap: var(--space-2) var(--space-5); margin: 0 0 var(--space-3); }
  dt { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  dd { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); }
  p { margin: var(--space-1) 0 0; color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .positive { color: var(--success); }
  .negative { color: var(--danger); }
  .usual, .target-meta { color: var(--text-muted); }
  .target { display: flex; align-items: center; gap: var(--space-3); margin: var(--space-3) 0; font-size: var(--text-caption); line-height: var(--leading-caption); }
  .target strong { font-weight: var(--weight-medium); }
  .target-meta { display: flex; gap: var(--space-3); font-size: var(--text-meta); line-height: var(--leading-meta); }
</style>

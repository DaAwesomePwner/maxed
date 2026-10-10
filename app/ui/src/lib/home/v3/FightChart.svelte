<script lang="ts">
  import type { DamageBucket } from '../../bindings/DamageBucket';
  import { damageChartView } from '../v2/DamageChart.svelte';
  import { formatDps, formatElapsed } from '../../styles/format';
  import { debugMode } from '../../debug/debug';
  let { buckets }: { buckets: DamageBucket[] } = $props();
  const uid = $props.id();
  let view = $derived(damageChartView({ buckets, playerSeries: [], selectedPlayerId: null }));
  // Each M starts a disconnected segment; close it independently at baseline 92.
  let area = $derived(view.partyPath.split('M').filter(Boolean).map(segment => {
    const coordinates = [...segment.matchAll(/([\d.e+-]+),([\d.e+-]+)/g)];
    return coordinates.length ? `M${segment} L${coordinates.at(-1)![1]},92 L${coordinates[0][1]},92 Z` : '';
  }).join(' '));
</script>

<div class="fight-chart">
  <div class="legend"><span>Party DPS</span><span class="you">You</span><span class="peak">Peak {formatDps(view.max)} DPS</span></div>
  {#if view.start !== null && view.end !== null}
    <div class="plot">
      <svg viewBox="0 0 600 104" preserveAspectRatio="none" role="img" aria-label="Party and your DPS over the fight">
        <defs><linearGradient id={`${uid}-area`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--violet)" stop-opacity=".24" />
          <stop offset="100%" stop-color="var(--violet)" stop-opacity="0" />
        </linearGradient></defs>
        {#each [28, 52, 76] as y}<line x1="12" x2="588" y1={y} y2={y} />{/each}
        <path class="area" d={area} fill={`url(#${uid}-area)`} />
        <path class="party" d={view.partyPath} /><path class="local" d={view.localPath} />
      </svg>
    </div>
    <div class="axis"><span>0:00</span><span>{formatElapsed(String(view.start), String(view.end))}</span></div>
  {:else}<div class="empty">No DPS</div>{/if}
  {#if $debugMode}<details><summary>Chart evidence</summary>
    {#if view.start === null}<p>Fight DPS unavailable: no positive-duration buckets.</p>{/if}
    {#each view.reasons as reason}<p>{reason}</p>{/each}
    <p>{view.rows.length} of {buckets.length} supplied intervals. No interpolation across missing observations.</p>
  </details>{/if}
</div>

<style>
  .fight-chart { display: flex; flex-direction: column; gap: 4px; min-width: 0; min-height: 0; box-sizing: border-box;
    flex: 1; border-top: 1px solid var(--border); padding-top: 8px; font-size: 11px; font-variant-numeric: tabular-nums; }
  .legend, .axis { display: flex; justify-content: space-between; color: var(--text-muted); gap: 12px; white-space: nowrap; }
  .legend > :first-child { color: var(--violet-text); } .you { color: var(--gold); } .peak { margin-left: auto; }
  /* Absolute svg: its aspect-ratio height must not raise the panel's min-content. */
  .plot { position: relative; min-height: 56px; flex: 1; }
  svg { position: absolute; inset: 0; display: block; width: 100%; height: 100%; }
  line { stroke: var(--border); opacity: .5; }
  .party, .local { fill: none; stroke-width: 2; vector-effect: non-scaling-stroke; }
  .party { stroke: var(--violet); } .local { stroke: var(--gold); }
  .empty { display: grid; place-items: center; min-height: 56px; flex: 1; color: var(--text-muted); }
  details { white-space: normal; } p { margin: 4px 0; }
</style>

<script lang="ts">
  import type { DamageBucket } from '../../bindings/DamageBucket';
  import { damageChartView } from '../v2/DamageChart.svelte';
  import { formatDps, formatElapsed } from '../../styles/format';
  import { debugMode } from '../../debug/debug';
  let { buckets }: { buckets: DamageBucket[] } = $props();
  let view = $derived(damageChartView({ buckets, playerSeries: [], selectedPlayerId: null }));
  // Each M starts a disconnected segment; close it independently at baseline 92.
  let area = $derived(view.partyPath.split('M').filter(Boolean).map(segment => {
    const coordinates = [...segment.matchAll(/([\d.e+-]+),([\d.e+-]+)/g)];
    return coordinates.length ? `M${segment} L${coordinates.at(-1)![1]},92 L${coordinates[0][1]},92 Z` : '';
  }).join(' '));
</script>

<div class="fight-chart">
  <div class="legend"><span class="party-key">Party DPS</span><span class="you">You</span><span class="peak">Peak {formatDps(view.max)} DPS</span></div>
  {#if view.start !== null && view.end !== null}
    <div class="plot">
      <svg viewBox="0 0 600 104" preserveAspectRatio="none" role="img" aria-label="Party and your DPS over the fight">
        {#each [28, 52, 76] as y}<line x1="12" x2="588" y1={y} y2={y} />{/each}
        <path class="area" d={area} />
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
  .fight-chart { display: flex; flex-direction: column; gap: var(--space-1); min-width: 0; min-height: 0; box-sizing: border-box;
    flex: 1; border-top: 1px solid var(--hairline); padding-top: var(--space-2); font-size: var(--text-meta); line-height: var(--leading-meta); color: var(--chart-label); font-variant-numeric: tabular-nums; }
  .legend, .axis { display: flex; justify-content: space-between; gap: var(--space-3); white-space: nowrap; }
  .legend > span { display: inline-flex; align-items: center; gap: var(--space-1); }
  .legend > span:not(.peak)::before { content: ''; width: 12px; height: 2px; border-radius: 1px; background: currentColor; }
  .party-key { color: var(--chart-party); } .you { color: var(--chart-you); } .peak { margin-left: auto; }
  /* Absolute svg: its aspect-ratio height must not raise the panel's min-content. */
  .plot { position: relative; min-height: 56px; flex: 1; }
  svg { position: absolute; inset: 0; display: block; width: 100%; height: 100%; }
  line { stroke: var(--chart-grid); vector-effect: non-scaling-stroke; }
  .area { fill: var(--chart-party); fill-opacity: .1; }
  .party, .local { fill: none; stroke-width: 2; vector-effect: non-scaling-stroke; }
  .party { stroke: var(--chart-party); } .local { stroke: var(--chart-you); }
  .empty { display: grid; place-items: center; min-height: 56px; flex: 1; }
  details { white-space: normal; } p { margin: var(--space-1) 0; }
</style>

<script lang="ts">
  import type { DamageBucket } from '../../bindings/DamageBucket';
  import { formatDps, formatElapsed } from '../../styles/format';
  import { partyChartView } from './you-vs-party';

  let { buckets, height = 160 }: { buckets: DamageBucket[]; height?: number } = $props();
  let width = $state(600);
  const plotHeight = $derived(Number.isFinite(height) && height >= 64 ? height : 160);
  const plotWidth = $derived(width || 600);
  const view = $derived(partyChartView(buckets, plotWidth, plotHeight));
  const id = $props.id();
  function measure(node: HTMLElement) {
    const resize = () => { width = node.clientWidth || 600; };
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(resize);
    resize();
    observer?.observe(node);
    return { destroy() { observer?.disconnect(); } };
  }
</script>

<div class="you-vs-party-chart">
  <div class="legend"><span><i class="you" aria-hidden="true"></i>You</span>
    {#if view.partyRecorded}<span><i class="party" aria-hidden="true"></i>Party average</span>{/if}
  </div>
  <div class="plot" use:measure style:height={`${plotHeight}px`}>
    {#if view.hasData && view.start !== null && view.end !== null}
      <svg viewBox={`0 0 ${plotWidth} ${plotHeight}`} role="img" aria-labelledby={`${id}-title`}>
        <title id={`${id}-title`}>{view.partyRecorded ? 'Your DPS and party average over the fight' : 'Your DPS over the fight'}</title>
        {#each view.ticks as tick}
          <line x1={view.left} x2={view.right} y1={tick.y} y2={tick.y} />
          <text x={view.left - 8} y={tick.y} text-anchor="end" dominant-baseline="middle">{tick.label}</text>
        {/each}
        {#if view.partyRecorded}
          <path class="party" d={view.partyLine.path} />
          {#if view.partyLine.marker}
            <circle class="party" cx={view.partyLine.marker.x} cy={view.partyLine.marker.y} r="4">
              <title>Party average: {formatDps(view.partyLine.marker.value)} DPS</title>
            </circle>
          {/if}
        {/if}
        <path class="you" d={view.youLine.path} />
        {#if view.youLine.marker}
          <circle class="you" cx={view.youLine.marker.x} cy={view.youLine.marker.y} r="4">
            <title>You: {formatDps(view.youLine.marker.value)} DPS</title>
          </circle>
        {/if}
        <text x={view.left} y={plotHeight - 4}>0:00</text>
        <text x={view.right} y={plotHeight - 4} text-anchor="end">{formatElapsed(String(view.start), String(view.end))}</text>
      </svg>
    {:else}<div class="empty">{buckets.length === 0 ? 'No live fight' : 'No DPS'}</div>{/if}
  </div>
  {#if buckets.length > 0 && !view.partyRecorded}<p>Party average line not recorded for this fight</p>{/if}
</div>

<style>
  .you-vs-party-chart { min-width: 0; color: var(--chart-label); font: var(--text-meta)/var(--leading-meta) var(--font-home); }
  .legend { display: flex; flex-wrap: wrap; gap: var(--space-4); margin-bottom: var(--space-1); }
  .legend span { display: inline-flex; align-items: center; gap: var(--space-2); }
  i { width: 12px; height: 2px; background: var(--series); }
  .you { --series: var(--chart-you); } .party { --series: var(--chart-party); }
  .plot { position: relative; width: 100%; }
  svg { display: block; position: absolute; inset: 0; width: 100%; height: 100%; font-variant-numeric: tabular-nums; }
  line { stroke: var(--chart-grid); stroke-width: 1; vector-effect: non-scaling-stroke; }
  text { fill: var(--chart-label); }
  path { fill: none; stroke: var(--series); stroke-width: 2; stroke-linejoin: round; stroke-linecap: round;
    vector-effect: non-scaling-stroke; }
  path.party { stroke-width: 1.5; }
  circle { fill: var(--series); stroke: var(--surface-panel); stroke-width: 2; vector-effect: non-scaling-stroke; }
  .empty { display: grid; place-items: center; height: 100%; }
  p { margin: var(--space-1) 0 0; }
</style>

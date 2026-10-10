<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { FamePoint } from '../bindings/FamePoint';
  import { famePreviewSegments, clockTime } from './nexus';
  import { formatStat } from '../styles/format';
  let { points, period, partial = false, sampled = false, gaps = false }: {
    points: FamePoint[]; period: string; partial?: boolean; sampled?: boolean; gaps?: boolean;
  } = $props();
  const id = $props.id();
  let valid = $derived(points.filter((point) => /^\d+$/.test(point.at) && Number.isFinite(point.fame) && point.fame >= 0));
  let segments = $derived(famePreviewSegments(valid));
  let bounded = $derived(segments.flat());
  let geometry = $derived.by(() => {
    if (!bounded.length) return [];
    const times = bounded.map((point) => BigInt(point.at));
    const origin = times.reduce((a, b) => a < b ? a : b);
    const end = times.reduce((a, b) => a > b ? a : b);
    const min = Math.min(...bounded.map((point) => point.fame));
    const max = Math.max(...bounded.map((point) => point.fame));
    return segments.map((segment) => segment.map((point) => ({
      x: end === origin ? 300 : 12 + Number((BigInt(point.at) - origin) * 1000000n / (end - origin)) / 1000000 * 576,
      y: max === min ? 60 : 108 - (point.fame - min) / (max - min) * 96,
    })));
  });
  // Lines split at character changes; preview sampling can omit intermediate values.
  let description = $derived(`${period} Fame`);
</script>
<div class="fame-chart">
  {#if bounded.length}
    <svg viewBox="0 0 600 120" preserveAspectRatio="none" role="img" aria-labelledby={`${id}-title ${id}-description`}>
  <title id={`${id}-title`}>Fame over Time</title>
  <desc id={`${id}-description`}>{description}</desc>
  <defs><linearGradient id={`${id}-fill`} x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="var(--gold)" stop-opacity=".28" /><stop offset="1" stop-color="var(--gold)" stop-opacity="0" /></linearGradient></defs>
  <g aria-hidden="true" pointer-events="none" stroke="#ffffff10">
    {#each [30, 60, 90] as y}<line x1="0" x2="600" y1={y} y2={y} vector-effect="non-scaling-stroke" />{/each}
  </g>
  {#each geometry as segment}
    {#if segment.length > 1 && !gaps}
      <polygon points={`${segment[0].x},120 ${segment.map(point => `${point.x},${point.y}`).join(' ')} ${segment[segment.length - 1].x},120`} fill={`url(#${id}-fill)`} />
      <polyline points={segment.map(point => `${point.x},${point.y}`).join(' ')} />
    {/if}
    {#each segment as point}<circle cx={point.x} cy={point.y} r="2" />{/each}
  {/each}
</svg>
<div class="axis"><span>{clockTime(bounded[0].at) ?? '—'} · {formatStat(bounded[0].fame)}</span><span>{clockTime(bounded[bounded.length - 1].at) ?? '—'} · {formatStat(bounded[bounded.length - 1].fame)}</span>
</div>
  {:else}<p>No Fame</p>{/if}
  {#if partial || sampled || valid.length > 120}<DebugNote>{partial ? 'Partial fame coverage; missing baselines. ' : ''}{sampled || valid.length > 120 ? 'Sampled preview; intermediate values may be omitted.' : ''}</DebugNote>{/if}
{#if gaps}<DebugNote text="Gap locations are unavailable; all fame lines and fills are hidden for this scope." />{/if}
</div>
<style>
.fame-chart { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .fame-chart svg { display: block; width: 100%; height: 150px; flex: 1; min-height: 90px; }
  .fame-chart polyline { fill: none; stroke: var(--gold); stroke-width: 2; vector-effect: non-scaling-stroke; }
  .fame-chart circle { fill: var(--gold); }
  .fame-chart .axis { display: flex; justify-content: space-between; gap: 8px; font-size: 11px; color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  .fame-chart p { color: var(--text-secondary); font-size: var(--text-label); margin: 4px 0; }
</style>

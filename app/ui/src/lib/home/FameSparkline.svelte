<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { RunView } from '../bindings/RunView';
  import type { FamePoint } from '../bindings/FamePoint';
  import { famePreviewSegments, clockTime } from './nexus';
  import { formatStat } from '../styles/format';
  let { points, period, partial = false, sampled = false, gaps = false, runs = [] }: {
    points: FamePoint[]; period: string; runs?: Pick<RunView, 'id' | 'started' | 'dungeon'>[]; partial?: boolean; sampled?: boolean; gaps?: boolean;
  } = $props();
  const id = $props.id();
  let valid = $derived(points.filter((point) => /^\d+$/.test(point.at) && Number.isFinite(point.fame) && point.fame >= 0));
  let segments = $derived(famePreviewSegments(valid));
  let bounded = $derived(segments.flat());
  let scale = $derived.by(() => {
    if (!bounded.length) return null;
    const times = bounded.map((point) => BigInt(point.at));
    const origin = times.reduce((a, b) => a < b ? a : b);
    const end = times.reduce((a, b) => a > b ? a : b);
    const min = Math.min(...bounded.map((point) => point.fame));
    const max = Math.max(...bounded.map((point) => point.fame));
    // Give a flat series a visible scale too; fame never falls below zero.
    const low = min === max ? Math.max(0, min - 1) : min;
    const high = min === max ? max + 1 : max;
    return { origin, end, low, high };
  });
  const x = (at: string) => !scale || scale.end === scale.origin ? 300
    : 12 + Number((BigInt(at) - scale.origin) * 1000000n / (scale.end - scale.origin)) / 1000000 * 576;
  const y = (fame: number) => !scale ? 60 : 108 - (fame - scale.low) / (scale.high - scale.low) * 96;
  let ticks = $derived(scale ? [...new Set([scale.high, Math.round((scale.low + scale.high) / 2), scale.low])] : []);
  let geometry = $derived(segments.map(segment => segment.map(point => ({ x: x(point.at), y: y(point.fame) }))));
  // Only the retained recent run starts are available, not a full run timeline.
  let markers = $derived(runs.filter(run => scale && /^\d+$/.test(run.started)
    && BigInt(run.started) >= scale.origin && BigInt(run.started) <= scale.end));
  // Lines split at character changes; preview sampling can omit intermediate values.
  let description = $derived(`${period} fame${scale ? `, ${formatStat(scale.low)} to ${formatStat(scale.high)}` : ''}. Triangles mark ${markers.length} retained recent run starts in this time range.`);
</script>
<div class="fame-chart">
  {#if bounded.length}
    <div class="plot">
    <div class="y-axis" aria-label="Fame scale" style:width={`${Math.max(...ticks.map(value => formatStat(value).length))}ch`}>
      {#each ticks as value}<span style:top={`${y(value) / 120 * 100}%`}>{formatStat(value)}</span>{/each}
    </div>
    <div class="chart-canvas"><svg viewBox="0 0 600 120" preserveAspectRatio="none" role="img" aria-labelledby={`${id}-title ${id}-description`}>
  <title id={`${id}-title`}>Fame over time</title>
  <desc id={`${id}-description`}>{description}</desc>
  <g class="grid" aria-hidden="true" pointer-events="none">
    {#each ticks as value}<line x1="12" x2="588" y1={y(value)} y2={y(value)} vector-effect="non-scaling-stroke" />{/each}
  </g>
  <line class="y-line" x1="12" x2="12" y1="12" y2="108" vector-effect="non-scaling-stroke" />
  {#each geometry as segment, index}
    {@const line = segment.length > 1 && !gaps}
    {#if line}
      <polygon points={`${segment[0].x},120 ${segment.map(point => `${point.x},${point.y}`).join(' ')} ${segment[segment.length - 1].x},120`} />
      <polyline points={segment.map(point => `${point.x},${point.y}`).join(' ')} />
    {/if}
    <!-- A drawn line marks only its latest point; unconnected readings keep every dot. -->
    {#each segment as point, i}{#if !line || (index === geometry.length - 1 && i === segment.length - 1)}<circle cx={point.x} cy={point.y} r="3" />{/if}{/each}
  {/each}
  {#each markers as run (run.id)}
    <path class="run-start" data-run-start={run.id} d={`M ${x(run.started) - 4} 119 L ${x(run.started)} 112 L ${x(run.started) + 4} 119 Z`}><title>Run start: {run.dungeon} · {clockTime(run.started)}</title></path>
  {/each}
</svg></div>
</div>
<div class="axis"><span>{clockTime(bounded[0].at) ?? '—'}</span><span>{clockTime(bounded[bounded.length - 1].at) ?? '—'}</span>
</div>
  {:else}<p>No fame</p>{/if}
  {#if partial || sampled || valid.length > 120}<DebugNote>{partial ? 'Partial fame coverage; missing baselines. ' : ''}{sampled || valid.length > 120 ? 'Sampled preview; intermediate values may be omitted.' : ''}</DebugNote>{/if}
{#if gaps}<DebugNote text="Gap locations are unavailable; all fame lines and fills are hidden for this scope." />{/if}
</div>
<style>
  .fame-chart { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .plot { display: flex; gap: var(--space-2); flex: 1; min-height: 152px; }
  .chart-canvas { position: relative; flex: 1; min-width: 0; }
  .y-axis { position: relative; flex: none; color: var(--chart-label); font-size: var(--text-meta); font-variant-numeric: tabular-nums; }
  .y-axis span { position: absolute; right: 0; transform: translateY(-50%); white-space: nowrap; }
  .grid line { stroke: var(--chart-grid); }
  .y-line { stroke: var(--hairline-strong); }
  .run-start { fill: var(--chart-label); }
  .fame-chart svg { position: absolute; inset: 0; display: block; width: 100%; height: 100%; }
  /* One gold series (your fame): a flat line with a quiet 10% fill, no gradient. */
  .fame-chart polygon { fill: var(--chart-you); fill-opacity: .1; }
  .fame-chart polyline { fill: none; stroke: var(--chart-you); stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; vector-effect: non-scaling-stroke; }
  .fame-chart circle { fill: var(--chart-you); stroke: var(--surface-panel); stroke-width: 2; vector-effect: non-scaling-stroke; }
  .fame-chart .axis { display: flex; justify-content: space-between; gap: var(--space-2); color: var(--chart-label); font-size: var(--text-meta);
    line-height: var(--leading-meta); font-variant-numeric: tabular-nums; }
  .fame-chart p { margin: var(--space-1) 0; color: var(--text-muted); font-size: var(--text-caption); }
</style>

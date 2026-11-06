<script lang="ts">
  import type { HpSample } from '../../bindings/HpSample';
  import { hpTraceSummary, hpTraceView } from './death';

  let { samples, diedAt, width = 320, height = 64 }: {
    samples: HpSample[]; diedAt: string; width?: number; height?: number;
  } = $props();
  const trace = $derived(hpTraceView(samples, width, height, diedAt));
  const summary = $derived(hpTraceSummary(samples, trace.gaps));
</script>

<figure class="hp-trace">
  <figcaption aria-hidden="true"><span>observed HP</span><span>Final 5 seconds</span></figcaption>
  {#if trace.paths.length}
    <span class="sr-only">{summary}</span>
    <svg {width} {height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
      {#each trace.paths as d}<path {d} />{/each}
    </svg>
  {:else}<p>No HP readings before death</p>{/if}
</figure>

<style>
  .hp-trace { margin: 16px 0 0; min-width: 0; max-width: 400px; }
  figcaption { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 8px; font-size: 11px; color: var(--text-secondary); }
  svg { display: block; width: 100%; height: auto; margin-top: 8px; overflow: visible; }
  path { fill: none; stroke: var(--danger); stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  p { margin: 12px 0 0; color: var(--text-secondary); font-size: 12px; }
</style>

<script lang="ts">
  import type { HomeDungeonStreak } from '../../bindings/HomeDungeonStreak';
  import { formatStat } from '../../styles/format';

  let { dungeon, stats, ended }: { dungeon: string; stats: HomeDungeonStreak | null; ended: number | null } = $props();
  const runs = $derived(stats?.runs_since_last_white ?? null);
  const average = $derived(stats?.avg_runs_per_white ?? null);
  const scale = $derived(average !== null && Number.isFinite(average) && average > 0 && runs !== null
    ? Math.max(average * 2, runs, 1) : null);
</script>

<div class="dry-streak-row">
  {#if ended !== null}<span class="ended">Ended at {ended} runs</span>
  {:else if stats === null}<p>Dry streak —</p>
  {:else if runs === null}<p>No white seen in {stats.runs_ended} runs</p>
  {:else}
    <p>{dungeon} dry streak {runs}</p>
    {#if scale !== null && average !== null}
      <div class="bar" class:past-average={runs > average} role="meter" aria-label={`${dungeon} dry streak`}
        aria-valuemin="0" aria-valuemax={scale} aria-valuenow={runs}
        aria-valuetext={`${runs} runs; average gap ${formatStat(average)} runs`}>
        <span class="fill" style:width={`${runs / scale * 100}%`}></span>
        <span class="tick" style:left={`${average / scale * 100}%`} title={`Average gap: ${formatStat(average)} runs`}></span>
      </div>
    {/if}
    <p class="caption" title="Observed, not a drop rate">Your average gap between {dungeon} whites: {formatStat(average)} runs</p>
  {/if}
</div>

<style>
  .dry-streak-row { min-width: 0; color: var(--text-primary); font-family: var(--font-home); font-size: var(--text-caption);
    line-height: var(--leading-caption); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  p { margin: 0; }
  .bar { position: relative; height: 6px; margin: var(--space-2) 0; border-radius: 2px; background: var(--bar-track); }
  .fill { display: block; height: 100%; border-radius: inherit; background: var(--text-disabled); }
  .past-average .fill { background: var(--warning); }
  .tick { position: absolute; top: -4px; bottom: -4px; width: 2px; background: var(--text-secondary); transform: translateX(-1px); }
  .caption { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .ended { color: var(--success); }
</style>

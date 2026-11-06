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
    <p class="caption" title="observed, not a drop rate">your average gap between {dungeon} whites: {formatStat(average)} runs</p>
  {/if}
</div>

<style>
  .dry-streak-row { min-width: 0; color: var(--text-primary); font: 13px/1.4 var(--font-home); overflow-wrap: anywhere; }
  p { margin: 0; }
  .bar { position: relative; height: 6px; margin: 10px 0; border-radius: var(--radius-pill); background: var(--surface-raised); }
  .fill { display: block; height: 100%; border-radius: inherit; background: var(--violet); }
  .past-average .fill { background: var(--warning); }
  .tick { position: absolute; top: -3px; bottom: -3px; width: 2px; background: var(--text-secondary); transform: translateX(-1px); }
  .caption { color: var(--text-secondary); font-size: 11px; }
  .ended { display: inline-block; padding: 3px 8px; border: 1px solid var(--success); border-radius: var(--radius-pill); color: var(--success); }
</style>

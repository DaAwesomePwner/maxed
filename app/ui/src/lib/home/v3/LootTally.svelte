<script lang="ts">
  import { formatStat } from '../../styles/format';
  let { bags, ut, st, items }: { bags: number; ut: number; st: number; items: number } = $props();
</script>
<dl class="loot-tally" aria-label="Loot tally">
  {#each [['Bags', bags], ['UT', ut], ['ST', st], ['Items', items]] as [label, value]}
    <div><dt>{label}</dt><dd>{formatStat(Number(value))}</dd></div>
  {/each}
</dl>
<style>
  .loot-tally { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); margin: 0; gap: var(--space-2); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .loot-tally div { display: flex; gap: var(--space-1); min-width: 0; align-items: baseline; }
  .loot-tally dt { color: var(--text-muted); }
  .loot-tally dd { margin: 0; color: var(--text-primary); font-variant-numeric: tabular-nums; }
</style>

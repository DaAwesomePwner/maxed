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
  .loot-tally { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); margin: 0; gap: 8px; font-size: 12px; }
  .loot-tally div { display: flex; gap: 5px; min-width: 0; align-items: baseline; }
  .loot-tally dt { color: var(--text-secondary); }
  .loot-tally dd { margin: 0; font-variant-numeric: tabular-nums; }
</style>

<script lang="ts">
  import { formatStat } from '../styles/format';
  let { shown, total, noun, onmore }: { shown: number; total: number; noun: string; onmore: () => void } = $props();
  let hidden = $derived(Math.max(0, total - shown));
</script>
{#if hidden > 0}
  <p class="show-more"><span>{formatStat(hidden)} more {noun} not shown</span>
    <button class="ctl ghost" type="button" onclick={onmore}>Show more</button></p>
{/if}
<style>
  .show-more { display: flex; align-items: center; justify-content: flex-end; gap: 8px; margin: 6px 0 0; color: var(--text-muted); font-size: 11px; }
</style>

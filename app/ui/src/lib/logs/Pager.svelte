<script lang="ts">
  import '../styles/controls.css';
  let { atLatest, hasOlder, loading, noun, onLatest, onNewer, onOlder }: {
    atLatest: boolean; hasOlder: boolean; loading: boolean; noun: string;
    onLatest: () => void; onNewer: () => void; onOlder: () => void;
  } = $props();
</script>
<div class="pager">
  {#if loading}<span role="status">Loading older {noun}</span>{/if}
  <button class="ctl ghost" disabled={atLatest || loading} onclick={onLatest}>Latest</button>
  <button class="ctl sq" aria-label={`Newer ${noun}`} disabled={atLatest || loading} onclick={onNewer}>‹</button>
  <button class="ctl sq" aria-label={`Older ${noun}`} disabled={!hasOlder || loading} onclick={onOlder}>›</button>
</div>
<style>
  .pager { display: flex; align-items: center; justify-content: flex-end; gap: var(--space-2); color: var(--text-muted); font-size: var(--text-meta); font-variant-numeric: tabular-nums; }
</style>

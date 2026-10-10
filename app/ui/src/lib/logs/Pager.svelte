<script lang="ts">
  import '../styles/controls.css';
  let { atLatest, hasOlder, loading, noun, onLatest, onNewer, onOlder }: {
    atLatest: boolean; hasOlder: boolean; loading: boolean; noun: string;
    onLatest: () => void; onNewer: () => void; onOlder: () => void;
  } = $props();
</script>
<div class="pager">
  {#if loading}<span role="status">Loading older…</span>{/if}
  <button class="ctl ghost" disabled={atLatest || loading} onclick={onLatest}>Latest</button>
  <button class="ctl sq" aria-label={`Newer ${noun}`} disabled={atLatest || loading} onclick={onNewer}>‹</button>
  <button class="ctl sq" aria-label={`Older ${noun}`} disabled={!hasOlder || loading} onclick={onOlder}>›</button>
</div>
<style>
  .pager { display: flex; align-items: center; justify-content: flex-end; gap: 6px; color: var(--text-muted); font-size: 12px; font-variant-numeric: tabular-nums; }
</style>

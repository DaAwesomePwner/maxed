<script lang="ts">
  import { windowIndex, WINDOW_SIZE } from './story';
  let { window = 0, count, label, noun, size = WINDOW_SIZE, controlLabel = 'Window', onWindow }: {
    window?: number; count: number; label: string; noun: string; size?: number; controlLabel?: string; onWindow: (index: number) => void;
  } = $props();
  let current = $derived(windowIndex(window, count, size));
  let last = $derived(windowIndex(Number.MAX_SAFE_INTEGER, count, size));
</script>

<nav aria-label={`${label} Pages`}>
  <button type="button" aria-disabled={current === 0} onclick={() => { if (current > 0) onWindow(current - 1); }}>Previous {controlLabel}</button>
  <span>{count ? current * size + 1 : 0}&ndash;{Math.min((current + 1) * size, count)} of {count} {noun}</span>
  <button type="button" aria-disabled={current >= last} onclick={() => { if (current < last) onWindow(current + 1); }}>Next {controlLabel}</button>
</nav>

<style>
  nav { display: grid; grid-template-columns: minmax(0, auto) minmax(0, 1fr) minmax(0, auto); align-items: center; gap: var(--space-1) var(--space-2); font-size: var(--text-label); min-width: 0; }
  span { text-align: center; font-variant-numeric: tabular-nums; color: var(--text-secondary); overflow-wrap: anywhere; }
  button { min-width: 0; overflow-wrap: anywhere; font: inherit; color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-control); min-height: var(--target-min); padding: var(--space-1) var(--space-2); }
  button[aria-disabled="true"] { color: var(--text-muted); }
</style>

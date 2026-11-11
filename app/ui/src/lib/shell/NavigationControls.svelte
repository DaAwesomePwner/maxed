<script lang="ts">
  let { canBack, canForward, back, forward }: { canBack: boolean; canForward: boolean; back: () => void; forward: () => void } = $props();
</script>
<div class="navigation-controls" role="group" aria-label="History">
  <button aria-label="Back" title="Back" disabled={!canBack} onclick={back}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3.5 5.5 8l4.5 4.5" /></svg></button>
  <button aria-label="Forward" title="Forward" disabled={!canForward} onclick={forward}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3.5 10.5 8 6 12.5" /></svg></button>
</div>
<style>
  .navigation-controls { display: flex; flex-wrap: nowrap; gap: var(--space-1); margin-left: auto; }
  button { box-sizing: border-box; width: var(--space-8); height: var(--space-8); min-height: 0; padding: 0; display: grid; place-items: center; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--text-secondary); }
  button:hover:not(:disabled) { background: var(--surface-hover); border-color: transparent; color: var(--text-primary); }
  button:disabled { color: var(--text-disabled); opacity: 1; }
  svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
</style>

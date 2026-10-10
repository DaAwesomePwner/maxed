<script lang="ts">
  let { canBack, canForward, back, forward }: { canBack: boolean; canForward: boolean; back: () => void; forward: () => void } = $props();
</script>
<div class="navigation-controls" role="group" aria-label="History">
  <button aria-label="Back" title="Back" disabled={!canBack} onclick={back}>←</button>
  <button aria-label="Forward" title="Forward" disabled={!canForward} onclick={forward}>→</button>
</div>
<style>
  .navigation-controls { display: flex; flex-wrap: nowrap; gap: 2px; margin-left: auto; }
  button { box-sizing: border-box; width: 28px; height: 28px; min-height: 28px; padding: 0; display: grid; place-items: center; border: 1px solid transparent; border-radius: 6px; background: transparent; color: var(--text-primary); }
  button:hover:not(:disabled) { background: var(--surface-raised); border-color: var(--border); }
  button:disabled { color: var(--text-disabled); opacity: .5; }
</style>

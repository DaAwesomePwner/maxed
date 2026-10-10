<script lang="ts">
  import type { StatusRegistry } from './registry';
  let { registry }: { registry: StatusRegistry } = $props();
</script>

<!-- Keep the live region mounted before its first announcement. No focus target. -->
<div class="status-overlay" class:active={Boolean($registry)} role="status" aria-label="Background activity" aria-live="polite" aria-atomic="true">{$registry}</div>

<style>
  .status-overlay {
    position: fixed; right: var(--space-3); bottom: var(--space-3); z-index: 30;
    max-width: min(26rem, calc(100vw - 2 * var(--space-3))); box-sizing: border-box;
    pointer-events: none; font-size: var(--text-label); line-height: var(--leading-label);
    color: var(--text-secondary); background: var(--surface-raised);
    border-radius: var(--radius-control); overflow-wrap: anywhere;
  }
  .active { padding: var(--space-1) var(--space-2); border: 1px solid var(--border); }
  @media (prefers-reduced-motion: reduce) {
    .status-overlay { animation: none; transition: none; }
  }
</style>

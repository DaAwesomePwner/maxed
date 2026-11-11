<script lang="ts">
  import '../styles/controls.css';
  let { message, onRetry }: { message: string; onRetry: () => void } = $props();
</script>
<div class="error" role="alert"><span>{message}</span><button class="ctl ghost" onclick={onRetry}>Retry</button></div>
<style>
  .error { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-1) var(--space-1) var(--space-1) var(--space-3); border: 1px solid color-mix(in srgb, var(--danger) 45%, transparent); border-radius: var(--radius-control); color: var(--danger); font-size: var(--text-caption); }
</style>

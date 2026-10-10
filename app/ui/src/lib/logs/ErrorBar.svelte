<script lang="ts">
  import '../styles/controls.css';
  let { message, onRetry }: { message: string; onRetry: () => void } = $props();
</script>
<div class="error" role="alert"><span>{message}</span><button class="ctl ghost" onclick={onRetry}>Retry</button></div>
<style>
  .error { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border: 1px solid var(--danger); border-radius: 6px; color: var(--danger); font-size: 12px; }
</style>

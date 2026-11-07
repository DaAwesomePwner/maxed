<script module lang="ts">
  const dismissed = new WeakSet<object>();
</script>
<script lang="ts">
  let { visible = false, session, onDismiss }: { visible?: boolean; session?: object; onDismiss?: () => void } = $props();
  let hidden = $state(false);
  function dismiss() { onDismiss?.(); hidden = true; if (session) dismissed.add(session); }
</script>

{#if visible && !hidden && (!session || !dismissed.has(session))}
  <div class="attribution-notice">
    <p role="status">Some older history couldn't be matched to an account and isn't counted here.</p>
    <button type="button" onclick={dismiss} aria-label="Dismiss older history notice">Dismiss</button>
  </div>
{/if}

<style>
  .attribution-notice { display: flex; align-items: center; gap: 1rem; }
  p { margin: 0; }
</style>

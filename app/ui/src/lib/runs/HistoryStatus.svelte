<script lang="ts">
  import type { HistoryState } from './history';
  let { history }: { history: HistoryState } = $props();
  let failed = false;
  let announcement = $state('');
  $effect(() => {
    const failure = history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable)
      || (history.phase === 'stale' && history.message.startsWith('Saving failed.'));
    if (failure && !failed) {
      failed = true;
      announcement = 'History Error';
    } else if (failed && history.phase === 'ready') {
      failed = false;
      announcement = 'History Ready';
    }
  });
</script>

<!-- Numeric revisions and catch-up ticks never change this live region. -->
<p class="sr-only" role="status" aria-atomic="true">{announcement}</p>

<script lang="ts">
  import Card from '../../../src/lib/components/Card.svelte';
  import EmptyState from '../../../src/lib/components/EmptyState.svelte';
  import ListRow from '../../../src/lib/components/ListRow.svelte';
  import Tooltip from '../../../src/lib/components/Tooltip.svelte';
  let { retry = () => {} }: { retry?: () => void } = $props();
</script>
<Card heading="Session"><p>Observed session</p></Card>
<Card heading="Session" tone="hero"><p>Hero content</p></Card>
<EmptyState heading="Capture unavailable" description="Reconnect capture to try again." state="error">
  {#snippet action()}<button type="button" onclick={retry}>Retry capture</button>{/snippet}
</EmptyState>
<ListRow href="/runs/1" label="The Shatters" selected><span>11:04 · cleared</span></ListRow>
<Tooltip id="direct-tip" text="Observed during this capture.">
  {#snippet trigger(describedby)}<button type="button" aria-describedby={describedby}>Evidence</button>{/snippet}
</Tooltip>

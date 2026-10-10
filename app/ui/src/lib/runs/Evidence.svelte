<script lang="ts">
  import { debugMode } from '../debug/debug';
  import type { EvidenceAt } from '../bindings/EvidenceAt';
  import Tooltip from '../components/Tooltip.svelte';
  import Badge from '../components/Badge.svelte';
  import { evidenceText } from './story';
  let { evidence, estimated = false }: { evidence: EvidenceAt[]; estimated?: boolean } = $props();
  const id = $props.id();
  let explanation = $derived(evidence.length ? evidence.map(evidenceText).join(' · ') : 'No outcome evidence is saved for this run.');
</script>
{#if $debugMode}
<Tooltip id={`${id}-outcome`} text={explanation}>
  {#snippet trigger(describedby)}<button type="button" aria-describedby={describedby}>Outcome Evidence</button>{/snippet}
</Tooltip>
{#if estimated}<Badge label="Estimated clear time" tone="warning" />{/if}
{/if}
<style>
  button { font: inherit; color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-control); min-height: var(--target-min); padding: var(--space-2) var(--space-3); cursor: help; }
</style>

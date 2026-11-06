<script lang="ts">
  import Tooltip from '../components/Tooltip.svelte';
  import { dpsMetric, type DpsScope } from './dps-metric';
  let { basis }: { basis: DpsScope } = $props();
  const id = $props.id();
  let metric = $derived(dpsMetric(basis));
</script>
<Tooltip id={`${id}-dps`} text={metric.tooltip}>
  {#snippet trigger(tooltipId)}<button type="button" aria-describedby={tooltipId}>{metric.label}</button>{/snippet}
</Tooltip>
<style>
  button { border: 0; padding: 0; background: transparent; color: inherit; font: inherit; cursor: help; text-decoration: underline dotted; text-underline-offset: 3px; }
</style>

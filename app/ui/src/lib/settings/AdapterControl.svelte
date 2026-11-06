<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { AdapterInfo } from '../bindings/AdapterInfo';
  let { adapters, value = $bindable(''), disabled = false, preview = false }: { adapters: AdapterInfo[] | null; value?: string; disabled?: boolean; preview?: boolean } = $props();
  const id = $props.id();
</script>
<label for={id}>Capture Adapter</label>
<select {id} bind:value {disabled}>
  <option value="">Auto</option>
  {#if value && !adapters?.some((adapter) => adapter.id === value)}<option {value}>Saved Adapter</option>{/if}
  {#each adapters ?? [] as adapter (adapter.id)}<option value={adapter.id}>{adapter.description || 'Unnamed Adapter'}</option>{/each}
</select>
{#if preview}<p>Capture adapters are unavailable in preview.</p>{/if}
{#if !preview && adapters?.length === 0}<p>No Adapters</p><DebugNote text="Auto has not verified capture access." />{/if}
<style>
  label { display: block; font-weight: var(--weight-semibold); margin-bottom: var(--space-2); }
  select { display: block; width: 100%; min-width: 0; min-height: var(--target-min); padding: var(--space-1) var(--space-2); font: inherit; color: var(--text-primary); background: var(--surface-canvas); border: 1px solid var(--border-strong); border-radius: var(--radius-control); }
  p { color: var(--text-secondary); margin-top: var(--space-2); }
</style>

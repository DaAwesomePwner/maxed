<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { AdapterInfo } from '../bindings/AdapterInfo';
  let { adapters, value = $bindable(''), disabled = false, preview = false }: { adapters: AdapterInfo[] | null; value?: string; disabled?: boolean; preview?: boolean } = $props();
  const id = $props.id();
</script>
<label for={id}>Capture adapter</label>
<select {id} bind:value {disabled}>
  <option value="">Auto</option>
  {#if value && !adapters?.some((adapter) => adapter.id === value)}<option {value}>Saved adapter</option>{/if}
  {#each adapters ?? [] as adapter (adapter.id)}<option value={adapter.id}>{adapter.description || 'Unnamed adapter'}</option>{/each}
</select>
{#if preview}<p>Capture adapters are unavailable in preview.</p>{/if}
{#if !preview && adapters?.length === 0}<p>No adapters</p><DebugNote text="Auto has not verified capture access." />{/if}
<style>
  label { display: block; margin-bottom: var(--space-1); color: var(--text-secondary); font-size: var(--text-caption); line-height: var(--leading-caption); font-weight: var(--weight-medium); }
  select { display: block; width: 100%; min-width: 0; }
  p { color: var(--text-secondary); margin-top: var(--space-2); }
</style>

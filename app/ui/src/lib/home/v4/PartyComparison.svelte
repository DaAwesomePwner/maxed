<script lang="ts">
  import { signedPercent } from './hud';
  import { tone } from './recap-view';

  let { value }: { value: number | null } = $props();
  const delta = $derived(value !== null && Number.isFinite(value) ? value : null);
  const comparisonTone = $derived(tone(delta));
</script>

<td data-tone={comparisonTone}>
  {#if delta !== null}
    <span class="sr-only">{Math.round(Math.abs(delta) * 100)}% {comparisonTone === 'up' ? 'above' : comparisonTone === 'down' ? 'below' : 'vs'} party average</span>
    <span aria-hidden="true">{signedPercent(delta)}</span>
  {:else}—{/if}
</td>

<style>
  [data-tone="up"] { color: var(--success); }
  [data-tone="down"] { color: var(--danger); }
</style>

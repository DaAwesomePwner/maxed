<script lang="ts">
  import type { Snippet } from 'svelte';
  let { heading, children }: { heading: string; children: Snippet } = $props();
  const id = $props.id();
</script>

<section aria-labelledby={id}>
  <h2 {id}>{heading}</h2>
  {@render children()}
</section>

<style>
  /* Same card as lib/components/Card: one padding, one radius, hairline edge. */
  section { min-width: 0; padding: var(--space-5); background: var(--surface-panel); border: 1px solid var(--hairline); border-radius: var(--radius-card); }
  h2 { margin: 0 0 var(--space-4); font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); }
</style>

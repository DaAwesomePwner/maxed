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
  section { min-width: 0; padding: var(--space-3); background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-card); box-shadow: var(--elevation-card); }
  h2 { margin: 0 0 var(--space-2); font-size: var(--text-body); line-height: var(--leading-body); font-weight: var(--weight-semibold); }
</style>

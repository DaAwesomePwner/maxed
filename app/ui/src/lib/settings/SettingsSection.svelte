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
  section { min-width: 0; padding: 12px; background: var(--surface-panel); border: 1px solid var(--border); border-radius: 8px; }
  h2 { margin: 0 0 12px; font-size: 14px; line-height: var(--leading-body); font-weight: var(--weight-semibold); }
</style>

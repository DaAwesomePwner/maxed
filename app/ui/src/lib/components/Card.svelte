<script lang="ts">
  import type { Snippet } from 'svelte';
  let { heading, children, tone = 'normal' }: { heading: string; children?: Snippet; tone?: 'normal' | 'hero' } = $props();
  const componentId = $props.id();
  const headingId = `${componentId}-heading`;
</script>

<section class:hero={tone === 'hero'} aria-labelledby={headingId}>
  <h2 id={headingId}>{heading}</h2>
  {#if children}<div class="content">{@render children()}</div>{/if}
</section>

<style>
  /* The hero tone keeps its semantics but shares the plain panel surface. */
  section { min-width: 0; padding: var(--space-5); color: var(--text-primary); background: var(--surface-panel); border: 1px solid var(--hairline); border-radius: var(--radius-card); overflow-wrap: anywhere; }
  h2 { margin: 0; font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); }
  .content { margin-top: var(--space-4); }
</style>

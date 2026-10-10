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
  section { min-width: 0; padding: var(--space-6); color: var(--text-primary); background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-card); box-shadow: var(--elevation-card); overflow-wrap: anywhere; }
  .hero { background: var(--hero-radial); }
  h2 { margin: 0; font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-semibold); }
  .content { margin-top: var(--space-4); }
</style>

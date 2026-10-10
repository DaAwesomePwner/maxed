<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { Snippet } from 'svelte';
  let { heading, description = '', action, state = 'empty' }: { heading: string; description?: string; action?: Snippet; state?: 'empty' | 'loading' | 'unavailable' | 'error' } = $props();
  const componentId = $props.id();
  const headingId = `${componentId}-heading`;
</script>

<section aria-labelledby={headingId} aria-busy={state === 'loading'} data-state={state}>
  <h2 id={headingId}>{heading}</h2>
  <DebugNote text={description} />
  {#if action}<div class="action">{@render action()}</div>{/if}
</section>

<style>
  section { padding: var(--space-8); border: 1px solid var(--border); border-radius: var(--radius-card); background: var(--surface-panel); color: var(--text-primary); overflow-wrap: anywhere; }
  h2 { margin: 0; font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-semibold); }
  .action { margin-top: var(--space-4); }
  [data-state="error"] { border-inline-start: 3px solid var(--danger); }
</style>

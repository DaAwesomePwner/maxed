<script lang="ts">
  import type { Snippet } from 'svelte';
  import { debugMode } from './debug';
  // Caveats about estimates, partial coverage, and provisional data render only in debug mode.
  let { text = '', inline = false, children }: { text?: string; inline?: boolean; children?: Snippet } = $props();
</script>

{#if $debugMode && (text || children)}
  {#if inline}<span class="debug-note" data-debug-note>{#if children}{@render children()}{:else}{text}{/if}</span>
  {:else}<p class="debug-note" data-debug-note>{#if children}{@render children()}{:else}{text}{/if}</p>{/if}
{/if}

<style>
  /* Debug caveats: warning-coloured text on an inset well; the colour carries the meaning, no dashed frame. */
  .debug-note { color: var(--warning); font-family: var(--font-ui); font-size: var(--text-caption); line-height: var(--leading-caption); font-weight: var(--weight-normal); overflow-wrap: anywhere; }
  p.debug-note { margin: 0; padding: var(--space-1) var(--space-2); background: var(--surface-inset); border: 1px solid var(--hairline); border-radius: var(--radius-control); }
  span.debug-note::before { content: ' \00b7  '; }
</style>

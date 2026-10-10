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
  .debug-note { color: var(--warning); font-family: var(--font-mono, monospace); font-size: var(--text-label); line-height: var(--leading-label); overflow-wrap: anywhere; }
  p.debug-note { margin: 0; padding: var(--space-1) var(--space-2); border: 1px dashed var(--warning); border-radius: var(--radius-control); }
  span.debug-note::before { content: ' \00b7  '; }
</style>

<script lang="ts">
  import type { Snippet } from 'svelte';
  let { href, label, children, selected = false }: { href: string; label: string; children?: Snippet; selected?: boolean } = $props();
</script>

<!-- Children are presentational only: this anchor is the row's sole action. -->
<a {href} class:selected aria-current={selected ? 'page' : undefined}>
  <span class="label">{label}</span>{#if children}<span class="details">{@render children()}</span>{/if}
</a>

<style>
  a { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2) var(--space-4); min-height: var(--row-min); padding: var(--space-3); border: 1px solid transparent; border-radius: var(--radius-control); color: var(--text-primary); text-decoration: none; font-size: var(--text-label); line-height: var(--leading-label); overflow-wrap: anywhere; }
  a:hover { background: var(--surface-hover); border-color: var(--text-muted); }
  .selected { background: var(--surface-raised); border-color: var(--border-strong); border-inline-start-color: var(--gold); }
  .label { font-weight: var(--weight-semibold); }
  .details { color: var(--text-secondary); min-width: 0; }
</style>

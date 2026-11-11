<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { LootView } from '../navigation/routes';
  let { view, subline, children }: { view: LootView; subline: string | null; children?: Snippet } = $props();
</script>
<header>
  <div><h2>Loot</h2>{#if subline !== null}<p>{subline}</p>{/if}</div>
  <nav aria-label="Loot views">
    {#each [['feed', 'Feed'], ['stats', 'Drop rates'], ['collection', 'Collection']] as [key, label]}
      <a href={key === 'feed' ? '#/loot' : `#/loot/${key}`} aria-current={view === key ? 'page' : undefined}>{label}</a>
    {/each}
  </nav>
  <div class="right">{@render children?.()}</div>
</header>
<style>
  header { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-3); margin-bottom: var(--space-3); }
  h2 { margin: 0; font-size: var(--text-hero); line-height: var(--leading-hero); font-weight: var(--weight-medium); }
  p { margin: 0; color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  nav { display: flex; gap: var(--space-1); align-self: center; margin-left: var(--space-4); }
  a { height: var(--control-dense); display: inline-flex; align-items: center; padding: 0 var(--space-3); border-radius: var(--radius-control); color: var(--text-secondary); font-size: var(--text-label); text-decoration: none;
    transition: background-color var(--motion-fast) var(--motion-ease), color var(--motion-fast) var(--motion-ease); }
  a:hover { background: var(--surface-hover); color: var(--text-primary); }
  a[aria-current] { background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--highlight-top); }
  a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  .right { margin-left: auto; }
</style>

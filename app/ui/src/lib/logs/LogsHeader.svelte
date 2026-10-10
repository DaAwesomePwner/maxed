<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { LogsView } from '../navigation/routes';
  let { view, subline, children }: { view: LogsView; subline: string | null; children?: Snippet } = $props();
</script>
<header>
  <div><h2>Logs</h2>{#if subline !== null}<p>{subline}</p>{/if}</div>
  <nav aria-label="Logs views">
    {#each [['chat', 'Chat'], ['keys', 'Key pops'], ['events', 'Events'], ['alerts', 'Alerts']] as [key, label]}
      <a href={key === 'chat' ? '#/logs' : `#/logs/${key}`} aria-current={view === key ? 'page' : undefined}>{label}</a>
    {/each}
  </nav>
  <div class="right">{@render children?.()}</div>
</header>
<style>
  header { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; margin-bottom: 10px; }
  h2 { margin: 0; font-size: 22px; }
  p { margin: 3px 0 0; color: var(--text-muted); font-size: 12px; }
  nav { display: flex; gap: 2px; align-self: center; margin-left: 14px; }
  a { height: 28px; display: inline-flex; align-items: center; padding: 0 12px; border-radius: 6px; color: var(--text-secondary); font-size: 13px; text-decoration: none; }
  a:hover { background: #ffffff0a; color: var(--text-primary); }
  a[aria-current] { background: var(--surface-raised); color: var(--text-primary); box-shadow: inset 0 -2px 0 var(--violet); }
  a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 3px; }
  .right { margin-left: auto; }
</style>

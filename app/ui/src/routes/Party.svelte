<script lang="ts">
  import type { Route } from '../lib/navigation/routes';
  import type { Client } from '../lib/bridge/client';
  import PartyLive from '../lib/party/PartyLive.svelte';
  import PlayersView from '../lib/party/PlayersView.svelte';
  import PresetsView from '../lib/party/PresetsView.svelte';
  let { route, client, preview = false }: { preview?: boolean; route: Extract<Route, { kind: `party-${string}` }>; client?: Client } = $props();
  let view = $derived(route.kind === 'party-presets' ? 'presets'
    : route.kind === 'party-players' || route.kind === 'party-player' ? 'players' : 'live');
</script>

<nav aria-label="Party views">
  <a href="#/party" aria-current={view === 'live' ? 'page' : undefined}>Live</a>
  <a href="#/party/players" aria-current={view === 'players' ? 'page' : undefined}>Players</a>
  <a href="#/party/presets" aria-current={view === 'presets' ? 'page' : undefined}>Presets</a>
</nav>
{#if route.kind === 'party-run' || route.kind === 'party-live'}
  {#if client}<PartyLive {client} runId={route.kind === 'party-run' ? route.runId : undefined} />
  {:else}<p>Party unavailable. No connection.</p>{/if}
{:else if view === 'players'}
  {#if client}<PlayersView {client} {preview} playerKey={route.kind === 'party-player' ? route.key : undefined} />
  {:else}<p>Party unavailable. No connection.</p>{/if}
{:else if view === 'presets'}
  {#if client}<PresetsView {client} />{:else}<p>Party unavailable. No connection.</p>{/if}
{/if}

<style>
  /* View switch: unselected is quiet text, selected is a neutral surface step. */
  nav { display: flex; flex-wrap: wrap; gap: var(--space-1); margin-bottom: var(--space-4); }
  a { display: flex; align-items: center; min-height: var(--control-dense); padding: 0 var(--space-3); border-radius: var(--radius-control); color: var(--text-secondary); font-size: var(--text-label); text-decoration: none; transition: background-color var(--motion-fast) var(--motion-ease), color var(--motion-fast) var(--motion-ease); }
  a:hover { background: var(--surface-hover); color: var(--text-primary); }
  a[aria-current] { background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--highlight-top); }
  p { color: var(--text-secondary); }
</style>

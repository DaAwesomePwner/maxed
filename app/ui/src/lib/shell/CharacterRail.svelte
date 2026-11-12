<script lang="ts">
  import HeroCard from './HeroCard.svelte';
  import LiveStatusCard from './LiveStatusCard.svelte';
  import NavigationControls from './NavigationControls.svelte';
  import { getContext, type Snippet } from 'svelte';
  import { readable } from 'svelte/store';
  import { SHELL_CONNECTION, type ShellConnection } from './context';
  import { emptyHomeLiveWindow } from '../home/v2/live';
  import { focusRailView, type CharacterRailView } from './rail';
  import type { CaptureStatus } from '../bindings/CaptureStatus';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { Features, Route } from '../navigation/routes';
  let { view, capture, route, features, preview, busy, error, snapshotError = '', missingCatalog = false, onStart, onStop, onRetry, resolveSpriteUrl, canBack = false, canForward = false, back = () => {}, forward = () => {}, statusExtra }: {
    view: CharacterRailView; capture: CaptureStatus | null; route: Route; features: Features; preview: boolean;
    busy: boolean; error: string; onStart: () => void; onStop: () => void; onRetry: () => void;
    resolveSpriteUrl?: (sprite: SpriteRef) => string | null;
    snapshotError?: string;
    missingCatalog?: boolean;
    canBack?: boolean; canForward?: boolean; back?: () => void; forward?: () => void; statusExtra?: Snippet;
  } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  const homeWindow = connection?.homeWindow ?? readable(emptyHomeLiveWindow());
  let focusedView = $derived(focusRailView(view, $homeWindow.focusedFightId ?? null, $homeWindow));
  let section = $derived(route.kind.startsWith('party-') ? 'party' : ['run', 'fight', 'player', 'run-player'].includes(route.kind) ? 'runs' : route.kind === 'item' ? 'loot' : route.kind);
</script>

<aside class="character-rail" aria-label="Character">
  <div class="rail-top">
    <div class="brand">Maxed</div>
    <NavigationControls {canBack} {canForward} {back} {forward} />
  </div>
  <div class="rail-cards">
    {#if snapshotError}<p class="data-unavailable">{snapshotError}</p>
      {#if !snapshotError.includes('Restart Maxed')}<button onclick={onRetry}>Reconnect</button>{/if}
    {/if}
    <HeroCard {view} {resolveSpriteUrl} sheetUrl={view.sprite ? resolveSpriteUrl?.(view.sprite) ?? null : null} />
    <LiveStatusCard view={focusedView} {capture} {preview} {busy} {error} {onStart} {onStop} {onRetry} />
    {#if statusExtra}<div class="rail-status-extra">{@render statusExtra()}</div>{/if}
    {#if missingCatalog}
      <p class="data-unavailable"><a href="#/setup">Open setup</a></p>
    {/if}
  </div>
  <nav aria-label="Navigation">
    <a href="#/" aria-current={section === 'home' ? 'page' : undefined}>Home</a>
    <a href="#/runs" aria-current={section === 'runs' ? 'page' : undefined}>Runs</a>
    {#if features.lootBrowsing}<a href="#/loot" aria-current={section === 'loot' ? 'page' : undefined}>Loot</a>{/if}
    {#if features.logs}<a href="#/logs" aria-current={section === 'logs' ? 'page' : undefined}>Logs</a>
    {:else}<a href="#/alerts" aria-current={section === 'alerts' ? 'page' : undefined}>Alerts</a>{/if}
    {#if features.stats}<a href="#/stats" aria-current={section === 'stats' ? 'page' : undefined}>Stats</a>{/if}
    {#if features.character}<a href="#/character" aria-current={section === 'character' ? 'page' : undefined}>Character</a>{/if}
    {#if features.party}<a href="#/party" aria-current={section === 'party' ? 'page' : undefined}>Party</a>{/if}
  </nav>
  <a class="settings-link" href="#/settings" aria-current={section === 'settings' ? 'page' : undefined}>Settings</a>
</aside>

<style>
  /* Only cards surrender space. Navigation never enters a scrolling ancestor. */
  .character-rail { min-height: 0; min-width: 0; height: 100%; overflow: hidden; padding: var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); border-right: 1px solid var(--hairline); background: var(--surface-canvas); }
  .rail-cards > button { flex: 0 0 auto; }
  .brand { flex: 0 0 auto; padding-inline: var(--space-1); font-size: var(--text-body); line-height: var(--leading-body); font-weight: var(--weight-semibold); color: var(--text-primary); }
  .data-unavailable { flex: 0 0 auto; margin: 0; color: var(--text-secondary); font-size: var(--text-label); line-height: var(--leading-label); }
  .rail-cards { flex: 0 1 auto; min-height: 0; overflow-x: hidden; overflow-y: auto; scrollbar-width: thin; display: flex; flex-direction: column; gap: var(--space-2); }
  nav { flex: 0 0 auto; display: grid; gap: 2px; }
  a { display: flex; align-items: center; min-height: var(--row-default); padding: 0 var(--space-3); border-radius: var(--radius-control); color: var(--text-secondary); text-decoration: none; font-size: var(--text-label); line-height: var(--leading-label); transition: background-color var(--motion-fast) var(--motion-ease), color var(--motion-fast) var(--motion-ease); }
  .data-unavailable a { display: inline; min-height: 0; padding: 0; color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  a:hover { background: var(--surface-hover); color: var(--text-primary); }
  a[aria-current] { background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--highlight-top); }
  .settings-link { flex: 0 0 auto; margin-top: auto; }
  .rail-top { flex: 0 0 auto; display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); min-height: var(--space-8); }
  .rail-status-extra { flex: 0 0 auto; display: grid; gap: var(--space-1); min-width: 0; padding-inline: var(--space-1); }
  @media (max-width: 900px) { .character-rail { padding: var(--space-2); } }
</style>

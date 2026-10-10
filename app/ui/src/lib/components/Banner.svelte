<script module lang="ts">
  import type { AlertKind } from '../bindings/AlertKind';
  export const bannerLabels: Record<AlertKind, string> = { whisper: 'Whisper', guild_message: 'Guild Message', party_message: 'Party Message', white_bag: 'White Bag', personal_best: 'Personal Best', flawless_run: 'Flawless Run', death: 'Death', key_popped: 'Key Popped' };
</script>

<script lang="ts">
  import MuteDungeon from '../alerts/MuteDungeon.svelte';
  import SpriteImage from './SpriteImage.svelte';
  import type { DungeonPortalSprite } from '../bridge/dungeon-portal';
  import DebugNote from '../debug/DebugNote.svelte';
  import { onMount } from 'svelte';
  import type { AlertView } from '../bindings/AlertView';
  import { alertDestination } from '../alerts/destination';
  let { id, kind, title, context, onDismiss, celebrate = true, presentation = 'banner', view = null, portal = null }: {
    id: string; kind: AlertKind; title: string; context: string; onDismiss: () => void;
    /** The persistent host enables shimmer only for a previously unseen event. */
    celebrate?: boolean;
    presentation?: 'toast' | 'banner';
    view?: AlertView | null;
    portal?: DungeonPortalSprite | null;
  } = $props();
  let destination = $derived(view ? alertDestination(view) : null);
  let happened = $derived.by(() => {
    if (!view || !/^\d+$/.test(view.at)) return null;
    const millis = BigInt(view.at) / 1000n;
    if (millis > 8_640_000_000_000_000n) return null;
    const date = new Date(Number(millis));
    return { iso: date.toISOString(), label: date.toLocaleString() };
  });
  let hidden = $state(false);
  onMount(() => {
    const update = () => { hidden = document.hidden; };
    update();
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  });
</script>

{#key id}
  <section class="banner" class:toast={presentation === 'toast'} class:paused={hidden} class:celebrate={celebrate && kind !== 'death'} data-kind={kind} aria-label={bannerLabels[kind]}>
    {#if portal}<span class="portal-icon"><SpriteImage sprite={portal.sprite} sheetUrl={portal.sheetUrl} alt={portal.name} fit={24} /></span>{/if}
    <div class="copy"><span class="kind">{bannerLabels[kind]}</span><h2>{title}</h2>{#if view?.details?.type === 'chat'}<p class="excerpt">{view.details.excerpt}</p>{/if}<DebugNote text={context} />
      {#if view}
        <p class="happened sr-only">{#if happened}<time datetime={happened.iso}>{happened.label}</time>{:else}&mdash;{/if}</p>
        {#if !destination}<DebugNote text="No saved destination is available. Details remain in Alerts." />{/if}
      {/if}
    </div>
    <div class="actions">
      {#if kind === 'key_popped' && view?.details?.type === 'key_popped'}<MuteDungeon dungeon={view.details.dungeon} />{/if}
      {#if view}<a href={destination ?? "#/alerts"}>View</a>{/if}
      <button type="button" aria-label={`Dismiss ${bannerLabels[kind]}`} onclick={onDismiss}>Dismiss</button>
    </div>
  </section>
{/key}

<style>
  .banner { position: relative; isolation: isolate; display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); width: 100%; min-width: 0; box-sizing: border-box; padding: var(--space-1) var(--space-3); border: 1px solid var(--gold); border-radius: var(--radius-banner); background: var(--surface-panel); color: var(--text-primary); overflow: hidden; }
  .celebrate::before { content: ''; position: absolute; pointer-events: none; z-index: -1; inset: 0 auto 0 0; width: 30%; background: linear-gradient(90deg, transparent, #ffffff55, transparent); animation: maxed-shimmer var(--motion-shimmer) var(--motion-ease) both; }
  .paused::before { animation-play-state: paused; }
  [data-kind="white_bag"] { border-color: var(--bag-white); box-shadow: var(--glow-white); }
  [data-kind="death"] { border-color: var(--danger); background: var(--surface-panel); box-shadow: none; }
  [data-kind="flawless_run"] { border-color: var(--success); }
  .copy { display: flex; flex-wrap: wrap; align-items: baseline; gap: 0 var(--space-2); flex: 1 1 12rem; min-width: 0; overflow-wrap: anywhere; }
  .portal-icon { display: flex; flex: none; }
  .portal-icon :global(.frame[data-fallback="true"]) { display: none; }
  .kind { color: var(--gold); font-size: var(--text-label); font-weight: var(--weight-semibold); }
  [data-kind="white_bag"] .kind { color: var(--bag-white); }
  [data-kind="death"] .kind { color: var(--danger); }
  [data-kind="flawless_run"] .kind { color: var(--success); }
  h2 { margin: 0; min-width: 0; font-size: var(--text-body); line-height: var(--leading-body); }
  .excerpt { flex-basis: 100%; margin: 0; min-width: 0; color: var(--text-secondary); font-size: var(--text-label); }
  p { margin: 0; color: var(--text-secondary); font-size: var(--text-label); line-height: var(--leading-label); }
  .happened { margin-top: var(--space-1); font-variant-numeric: tabular-nums; }
  .actions { display: flex; flex: none; max-width: 100%; margin-left: auto; flex-wrap: wrap; gap: var(--space-2); }
  button, a { display: inline-flex; align-items: center; justify-content: center; box-sizing: border-box; flex: none; min-height: var(--target-min); border: 1px solid var(--border-strong); border-radius: var(--radius-control); padding: var(--space-2); color: var(--text-primary); background: var(--surface-panel); font: inherit; cursor: pointer; text-decoration: none; }
  button:hover, a:hover { background: var(--surface-hover); border-color: var(--text-muted); }
  .toast { padding: var(--space-3); gap: var(--space-3); background: var(--surface-panel); box-shadow: none; }
  .toast h2 { font-size: var(--text-body); line-height: var(--leading-body); }
  .toast .copy { display: block; }
  @media (max-width: 900px) { .copy { display: block; } }
  @media (max-width: 440px) { .copy { flex-basis: 7rem; } .banner { padding: var(--space-1) var(--space-2); } }
  @media (prefers-reduced-motion: reduce) { .celebrate::before { display: none; } }
</style>

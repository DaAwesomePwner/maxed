<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { fly } from 'svelte/transition';
  import type { Client } from '../bridge/client';
  import { createDungeonPortalSprite, type DungeonPortalSprite } from '../bridge/dungeon-portal';
  import type { AlertKind } from '../bindings/AlertKind';
  import type { AlertView } from '../bindings/AlertView';
  import Banner, { bannerLabels } from './Banner.svelte';
  let { moment, onDismiss, client = null, presentation = 'banner', view = null, sessionKey = null,
    overflow = 0, showAlertsAction = false, dismissFocusTarget = null, onPauseChange = () => {} }: {
    moment: { id: string; kind: AlertKind; title: string; context: string } | null;
    onDismiss: () => void;
    client?: Client | null;
    presentation?: 'toast' | 'banner';
    view?: AlertView | null;
    sessionKey?: string | null;
    overflow?: number;
    showAlertsAction?: boolean;
    /** Optional caller-owned target; otherwise return focus to the overlay Alerts link. */
    dismissFocusTarget?: HTMLElement | null;
    onPauseChange?: (paused: boolean, hidden: boolean) => void;
  } = $props();
  // Keep the outgoing toast's width until its transition has finished.
  let lastPresentation = $state<'toast' | 'banner'>('banner');
  $effect(() => { if (moment) lastPresentation = presentation; });
  let portal = $state<DungeonPortalSprite | null>(null);
  let dungeon = $derived(moment?.kind === 'key_popped' && view?.details?.type === 'key_popped' ? view.details.dungeon : null);
  $effect(() => {
    portal = null;
    if (!client || dungeon === null) return;
    const lookup = createDungeonPortalSprite(client, value => { portal = value; });
    lookup.update(dungeon);
    return () => lookup.dispose();
  });
  function enter(node: Element) {
    // Without a motion preference API, choose the accessible no-motion default.
    const duration = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === false ? 180 : 0;
    return fly(node, { y: -8, duration });
  }
  // Host lifetime is the display session. Keep mounted across gaps between moments.
  const announcedIds = new Set<string>();
  let announcedSession: string | null = null;
  let momentElement = $state<HTMLDivElement>();
  let alertsAction = $state<HTMLAnchorElement>();
  let actionFocused = $state(false);
  let hovered = false, focused = false, hidden = false;
  const pause = () => onPauseChange(hidden || hovered || focused, hidden);
  function dismiss() {
    // Arrival and background expiry never move focus. Only the removed control does.
    if (momentElement?.contains(document.activeElement)) (dismissFocusTarget ?? alertsAction)?.focus({ preventScroll: true });
    onDismiss();
  }
  onMount(() => {
    const visibility = () => { hidden = document.hidden; pause(); };
    visibility();
    document.addEventListener('visibilitychange', visibility);
    return () => { document.removeEventListener('visibilitychange', visibility); onPauseChange(false, false); };
  });
  let announcement = $state('');
  let celebrate = $state(false);
  // Presentation-only updates to the same ID must not repeat an announcement.
  let currentId = $derived(moment?.id ?? null);
  $effect(() => {
    const eventId = currentId, session = sessionKey;
    untrack(() => {
      if (session !== announcedSession) { announcedIds.clear(); announcedSession = session; }
      const current = moment;
      const unseen = eventId !== null && !announcedIds.has(eventId);
      celebrate = unseen && current?.kind !== 'death';
      if (unseen && current) {
        announcedIds.add(current.id);
        if (announcedIds.size > 256) announcedIds.delete(announcedIds.values().next().value!);
        announcement = `${bannerLabels[current.kind]}: ${current.title}`;
      } else {
        announcement = '';
      }
      if (!current) { hovered = false; focused = false; pause(); }
    });
  });
</script>

<!-- This region remains in the DOM before, between and after visual banners. -->
<div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{announcement}</div>
<div class="alert-overlay" class:toast-overlay={(moment ? presentation : lastPresentation) === 'toast'}>
  {#if moment}
    {#key moment.id}
      <div transition:enter|global class="moment" role="group" aria-label="Alert" bind:this={momentElement}
        onpointerenter={() => { hovered = true; pause(); }}
        onpointerleave={() => { hovered = false; pause(); }}
        onfocusin={() => { focused = true; pause(); }}
        onfocusout={(event) => { focused = event.relatedTarget instanceof Node && !!momentElement?.contains(event.relatedTarget); pause(); }}>
        <Banner {...moment} onDismiss={dismiss} {celebrate} {presentation} {view} {portal} />
      </div>
    {/key}
  {/if}
  {#if overflow > 0 || (showAlertsAction && (moment !== null || actionFocused))}
    <a class="alerts-action" href="#/alerts" bind:this={alertsAction}
        onfocus={() => { actionFocused = true; }} onblur={() => { actionFocused = false; }}>{overflow > 0 ? 'More Alerts' : 'Alerts'}</a>
  {/if}
</div>

<style>
  /* Reserve 20 CSS px for a classic scrollbar without measuring the pane. */
  .alert-overlay { position: absolute; top: 0; left: 0; right: 20px; z-index: 20; display: grid; min-width: 0; pointer-events: none; }
  .toast-overlay { left: auto; width: min(24rem, calc(100% - 20px)); padding: var(--space-2); box-sizing: border-box; }
  .moment { grid-area: 1 / 1; min-width: 0; pointer-events: auto; }
  /* Svelte applies inert at runtime while an outro retains the element. */
  .moment:global([inert]) { pointer-events: none; }
  .alerts-action { grid-area: 2 / 1; justify-self: end; pointer-events: auto; padding: 0 var(--space-3); background: var(--surface-panel); border-radius: var(--radius-control); display: inline-flex; align-items: center; align-self: flex-start; min-height: var(--target-min); color: var(--text-secondary); font-size: var(--text-label); }
</style>

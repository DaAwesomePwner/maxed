<script lang="ts">
  import { isPresentationVisible, observePresentationVisibility } from '../bridge/visibility';
  import { getContext, tick } from 'svelte';
  import type { Client, PartyChangedNotice } from '../bridge/client';
  import type { PartyMember } from '../bindings/PartyMember';
  import type { PartyView } from '../bindings/PartyView';
  import type { RequirementPreset } from '../bindings/RequirementPreset';
  import type { RunView } from '../bindings/RunView';
  import type { EventEnvelope } from '../bindings/EventEnvelope';
  import type { Route } from '../navigation/routes';
  import { breadcrumbs, routeHref } from '../navigation/routes';
  import Breadcrumbs from '../runs/Breadcrumbs.svelte';
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { spriteUrl } from '../bridge/visuals';
  import { invalidatesParty, partyNoticeInvalidates } from './invalidation';
  import { historyControl, historyWaiting } from '../runs/policy';
  import { failingNamesText, filterMembers, sortMembers, type MemberFilter } from './party-screen';
  import PartyContextBar from './PartyContextBar.svelte';
  import PartySummary from './PartySummary.svelte';
  import RosterTable from './RosterTable.svelte';
  import PlayerSidePanel from './PlayerSidePanel.svelte';
  import '../styles/controls.css';
  let { client, runId }: { client: Client; runId?: string } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  let view = $state.raw<PartyView | null>(null);
  let presets = $state.raw<RequirementPreset[]>([]), recentRun = $state.raw<RunView | null>(null);
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let filter = $state<MemberFilter>('all'), selectedId = $state<string | null>(null);
  let message = $state('Loading party'), error = $state(''), copied = $state(''), picking = $state(false);
  let subscriptionError = $state('');
  let refresh = $state<() => void>(() => {});
  let runLink: HTMLAnchorElement;
  let region: HTMLElement;
  let saved = $derived(runId !== undefined);
  let route = $derived<Route>(runId === undefined ? { kind: 'party-live' } : { kind: 'party-run', runId });
  let members = $derived(sortMembers(filterMembers(view?.members ?? [], filter)));
  let selected = $derived(view?.members.find(member => (member.name_key ?? member.player_id) === selectedId) ?? null);
  let filters = $derived(([
    ['all', 'All'], ...(view?.preset && view.assets_available ? [['failing', 'Failing'], ['check', 'Check'], ['not-seen', 'Not seen']] : []),
    ...(view?.area_kind === 'dungeon' ? [['flagged', 'Flagged']] : []), ['marked', 'Marked'],
  ] as [MemberFilter, string][]));
  $effect(() => { if (!filters.some(([value]) => value === filter)) filter = 'all'; });
  $effect(() => {
    const owner = client, id = runId;
    const route: Route = id === undefined ? { kind: 'party-live' } : { kind: 'party-run', runId: id };
    let disposed = false, reading = false, dirty = false, lastRead = -Infinity, busyRetried = false;
    let scopeChanged = false, pendingScope: string | null = null;
    let required: { streamId: string; required: string } | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let subscribing = false, subscribed = false;
    const releases: (() => void)[] = [];
    view = null; selectedId = null; filter = 'all'; message = 'Loading party'; error = ''; subscriptionError = ''; copied = '';
    async function read() {
      if (disposed || reading || !isPresentationVisible()) return;
      reading = true; dirty = false; scopeChanged = false; lastRead = Date.now();
      try {
        if (required) {
          const pending = required;
          const waiting = historyWaiting({ ...pending, store: await owner.queryStore() });
          if (disposed) return;
          if (waiting) { message = waiting; dirty = true; return; }
          if (required === pending) required = null;
        }
        const reply = id === undefined ? await owner.getPartyLive() : await owner.getPartyRun(id);
        if (disposed) return;
        if (reply.status === 'ready') {
          if ((reply.value?.scope_id ?? null) === pendingScope) scopeChanged = false;
          // The scope can change while the page remains mounted. Never carry a selection across maps.
          if (view?.scope_id !== reply.value?.scope_id) { void closePanel(false); if (view) readRecentRun(); }
          else if (selectedId && !reply.value?.members.some(member => (member.name_key ?? member.player_id) === selectedId)) void closePanel(false);
          view = reply.value; message = reply.value ? '' : 'Party run not found'; error = ''; busyRetried = false;
        } else {
          const control = historyControl(reply.status, busyRetried);
          message = control.message;
          if (control.retry) { busyRetried = true; dirty = true; }
        }
      } catch { if (!disposed) { error = 'Could not load party.'; message = ''; } }
      finally { reading = false; if (!disposed && dirty) schedule(); }
    }
    function schedule(notice?: PartyChangedNotice) {
      if (disposed) return;
      dirty = true;
      // Preserve scope changes during a read and replace any throttled trailing refresh.
      if (notice && notice.scope_id !== (view?.scope_id ?? null)) { pendingScope = notice.scope_id; scopeChanged = true; }
      if (scopeChanged) { clearTimeout(timer); timer = undefined; }
      if (reading || timer !== undefined || !isPresentationVisible()) return;
      const delay = scopeChanged ? 0 : Math.max(0, 1000 - (Date.now() - lastRead));
      if (!delay) void read();
      else timer = setTimeout(() => { timer = undefined; void read(); }, delay);
    }
    refresh = () => { busyRetried = false; subscribe(); readPresets(); schedule(); };
    function subscribe() {
      if (disposed || subscribing || subscribed) return;
      subscribing = true;
      const receive = (event: EventEnvelope) => {
        if (invalidatesParty(route, event)) { required = { streamId: event.stream_id, required: event.sequence }; schedule(); }
      };
      const subscription = id === undefined
        ? owner.onPartyChanged(notice => { if (partyNoticeInvalidates(route, notice)) schedule(notice); })
        : connection?.onEvent ? Promise.resolve(connection.onEvent(receive))
        : owner.onEngineEvent(receive, batch => batch.events.forEach(receive));
      void subscription.then(release => {
        if (disposed) release();
        else { releases.push(release); subscribed = true; subscriptionError = ''; }
      }).catch(() => { if (!disposed) subscriptionError = 'Party updates unavailable. Retry to reconnect.'; })
        .finally(() => { subscribing = false; });
    }
    subscribe();
    function readPresets() {
      void owner.querySettings().then(settings => { if (!disposed) presets = settings.party.presets; })
        .catch(() => { if (!disposed) error = 'Could not load presets.'; });
    }
    function readRecentRun() {
      // Include an open run and the latest ended run, without retaining the whole run history.
      void owner.listRuns({ dungeon: null, outcome: null, range: null, sort: 'newest', cursor: null, limit: 2 })
        .then(reply => { if (!disposed) recentRun = reply.status === 'ready' ? reply.value.rows.find(run => run.ended != null) ?? null : null; })
        .catch(() => { if (!disposed) recentRun = null; });
    }
    readPresets();
    readRecentRun();
    const releaseVisibility = observePresentationVisibility(visible => {
      if (visible) schedule();
      else { clearTimeout(timer); timer = undefined; }
    });
    return () => { disposed = true; releaseVisibility(); clearTimeout(timer); releases.forEach(release => release()); };
  });
  $effect(() => {
    const controller = createHomeVisuals(client, next => { visuals = next; });
    visualController = controller;
    return () => controller.dispose();
  });
  $effect(() => {
    visualController?.update((view?.members ?? []).flatMap(member => [member.class_id,
      ...member.equipment.map(item => item.item_type)]).filter((type): type is number => type != null));
  });
  async function pick(id: string | null) {
    if (picking) return;
    picking = true;
    try { await client.setPartyPresetPick(id); refresh(); }
    catch { error = 'Could not pick preset.'; }
    finally { picking = false; }
  }
  async function selectMember(member: PartyMember, keyboard = false) {
    const key = member.name_key ?? member.player_id;
    selectedId = key;
    if (!keyboard) return;
    await tick();
    if (selectedId === key) region?.querySelector<HTMLElement>('aside h3')?.focus({ preventScroll: true });
  }
  async function closePanel(userInitiated = true) {
    const key = selectedId;
    const refocus = userInitiated || document.activeElement === document.body
      || region?.querySelector('aside')?.contains(document.activeElement);
    selectedId = null;
    if (key === null || !refocus) return;
    await tick();
    if (!region?.isConnected || selectedId !== null) return;
    const row = [...region.querySelectorAll<HTMLElement>('[data-row-id]')].find(row => row.dataset.rowId === key);
    (row ?? document.querySelector<HTMLElement>('h1'))?.focus({ preventScroll: true });
  }
  async function copyFailing() {
    if (!view || !navigator.clipboard?.writeText) return;
    try { await navigator.clipboard.writeText(failingNamesText(view.members)); copied = ''; await tick(); copied = 'Copied'; }
    catch { /* Clipboard may be denied by the host. */ }
  }
  function openRun(member: PartyMember) {
    if (view?.run_id && member.name && member.name_key) {
      runLink.href = routeHref({ kind: 'run-player', runId: view.run_id, playerId: member.player_id });
      runLink.click();
    }
  }
</script>
<section bind:this={region} aria-label={saved ? 'Party run' : 'Party live'}>
  <!-- Use the Shell's link navigation so double-click preserves the history stack. -->
  <a bind:this={runLink} href="#/party" hidden tabindex="-1">Run view</a>
  {#if message}<p role="status">{message}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if subscriptionError}<p role="alert">{subscriptionError}</p>{/if}
  {#if error || subscriptionError || (message && message !== 'Loading party' && message !== 'Party run not found')}<button class="ctl" onclick={() => refresh()}>Retry</button>{/if}
  {#if view}
    {#if saved}<Breadcrumbs items={breadcrumbs(route, view.map ?? undefined)} />{/if}
    <PartyContextBar {view} {saved} {presets} {recentRun} onPick={pick} busy={picking} />
    {#if !view.assets_available}<p class="notice">Unavailable, game assets not extracted</p>{/if}
    {#if saved && !view.gear_recorded}<p class="notice">Gear not recorded for this run</p>{/if}
    <PartySummary {view} {saved} {visuals} />
    <div class="layout" class:with-panel={selected !== null}>
      <section class="roster" aria-label="Roster">
        <header><h2>Roster</h2><div class="filters" aria-label="Roster filters">
          {#each filters as [value, label]}<button class="ctl" aria-pressed={filter === value} onclick={() => { filter = value; }}>{label} {filterMembers(view.members, value).length}</button>{/each}
        </div>
        {#if !saved && view.preset && view.assets_available}<button class="ctl" onclick={copyFailing}>Copy failing names</button>{/if}
        <span role="status">{copied}</span></header>
        <RosterTable {view} {members} {saved} selected={selectedId} {visuals} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)}
          onSelect={selectMember} onRun={openRun} />
        <p class="hint">Sorted by parse, then damage. Click a row for details.{#if view.run_id}{' '}A double-click opens this run's player view.{/if}</p>
      </section>
      {#if selected && selected.name && selected.name_key}
        {#key selected.name_key}<PlayerSidePanel member={selected} runId={view.run_id ?? null} {client} {visuals}
          resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} onChanged={() => refresh()} onClose={closePanel} />{/key}
      {/if}
    </div>
  {/if}
</section>
<style>
  .layout { display: grid; gap: var(--space-3); align-items: start; } .with-panel { grid-template-columns: minmax(0, 1fr) 290px; }
  .roster { min-width: 0; padding: var(--space-3); border: 1px solid var(--border); border-radius: var(--radius-card); background: var(--surface-panel); }
  header, .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; } header { gap: var(--space-2); margin-bottom: var(--space-2); }
  h2 { font-size: 12px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-secondary); margin: 0 auto 0 0; }
  .filters button { border-radius: 12px; } button[aria-pressed=true] { border-color: var(--gold); color: var(--text-primary); }
  .hint, .notice { font-size: 12px; color: var(--text-secondary); } .hint { margin-bottom: 0; } [role=alert] { color: var(--danger); }
  @media (max-width: 1050px) { .with-panel { grid-template-columns: minmax(0, 1fr); } }
</style>

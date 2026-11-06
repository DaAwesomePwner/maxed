<script lang="ts">
  import { itemHover } from '../components/item-hover';
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { getContext, tick, untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { AppSettings } from '../bindings/AppSettings';
  import type { DropCursor } from '../bindings/DropCursor';
  import { emptyListState, normalizeListState, type ListState } from '../navigation/state';
  import { createRunHistory, type HistoryController, type HistoryState } from '../runs/history';
  import { createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { objectVisualTypes, spriteUrl } from '../bridge/visuals';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import EmptyState from '../components/EmptyState.svelte';
  import PinButton from './PinButton.svelte';
  import '../styles/controls.css';
  import ItemSideCards from './ItemSideCards.svelte';
  import KpiStrip from '../home/v3/KpiStrip.svelte';
  import Breadcrumbs from '../runs/Breadcrumbs.svelte';
  import { itemKpis } from './item-screen';
  import { tierClass, clockWhen, shortWhen } from './loot-format';
  import { routeHref } from '../navigation/routes';
  import ItemTile from '../components/ItemTile.svelte';
  import LootBagTable from './LootBagTable.svelte';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { itemType, client, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true }: {
    itemType: number; client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const restored = untrack(() => normalizeListState(savedState));
  // Item state stores the visited cursor stack, including the current page at its tail.
  const initial = untrack(() => restored.lootPosition?.item?.type === itemType ? restored.lootPosition.item.previous : []);
  let cursors = $state<Array<DropCursor | null>>(initial.length ? initial : [null]);
  let cursor = $derived(cursors[cursors.length - 1]);
  let settings = $state.raw<AppSettings | null>(null);
  let settingsError = $state(''), settingsRevision = $state(0);
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading item history.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let region: HTMLDivElement;
  let paging = $state(false), missingRow = $state('');
  let item = $derived(history.value?.kind === 'item-history' ? history.value.history : null);
  let page = $derived(item?.bags ?? null);
  let visual = $derived(visuals.objects.find(object => object.object_type === itemType));
  const now = new Date();
  let name = $derived(visual?.name ?? `#${itemType}`);
  let available = $derived(!client || page !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  let releasedEntry: string | undefined;
  let activeClient = $derived(enabled ? client : undefined);
  $effect(() => {
    const owner = activeClient;
    if (!owner) {
      controller = undefined;
      history = { phase: 'loading', value: null, message: 'Loading item history.', retryAvailable: false };
      return;
    }
    const instance = createRunHistory(owner, next => {
      history = next;
      if (next.phase === 'ready' || next.phase === 'error' || next.phase === 'superseded' || (next.phase === 'busy' && next.retryAvailable)) paging = false;
    }, { cache: historyCache });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller;
    const input = { query: { kind: 'item-history' as const, request: { item_type: itemType, cursor: $state.snapshot(cursor), limit: 50 } }, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  $effect(() => {
    const owner = activeClient;
    if (!owner) { visualController = undefined; visuals = emptyHomeVisuals(); return; }
    const instance = createHomeVisuals(owner, next => { visuals = next; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const types = objectVisualTypes([itemType, ...(item?.by_source.map(source => source.source_enemy) ?? []), ...(page?.rows.flatMap(({ drop }) =>
      [drop.bag_type, drop.source_enemy, ...drop.contents.map(item => item.item_type)]) ?? [])]);
    visualController?.update(types);
  });
  $effect(() => {
    const owner = activeClient; settingsRevision;
    settings = null; settingsError = '';
    if (!owner) return;
    let current = true;
    void owner.querySettings().then(value => { if (current) settings = value; }, () => {
      if (current) settingsError = "Couldn't load the chase list";
    });
    return () => { current = false; };
  });
  $effect.pre(() => {
    const rows = page?.rows;
    if (!rows || !region) return;
    const active = document.activeElement;
    const id = active instanceof HTMLElement && region.contains(active) ? active.closest<HTMLElement>('[data-row-id]')?.dataset.rowId : null;
    if (!id) return;
    void tick().then(() => {
      if (!region?.isConnected || active?.isConnected) return;
      const pane = region.closest('main');
      const replacement = [...region.querySelectorAll<HTMLElement>('[data-row-id]')].find(row => row.dataset.rowId === id);
      if (replacement) { replacement.focus({ preventScroll: true }); return; }
      pane?.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
      if (pane) pane.scrollTop = Math.max(0, Math.min(pane.scrollTop, pane.scrollHeight - pane.clientHeight));
      missingRow = 'Row Removed';
      onStateChange?.({ focusedRowId: null, ...(pane ? { scrollTop: pane.scrollTop } : {}) });
    });
  });
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready || (client && !enabled) || releasedEntry === key) return;
    let current = true;
    void tick().then(() => {
      if (current) { releasedEntry = key; onRowsReady?.(key); }
    });
    return () => { current = false; };
  });
  function persist() {
    missingRow = '';
    const position = normalizeListState(savedState).lootPosition ?? restored.lootPosition;
    onStateChange?.({ lootPosition: {
      feed: { filter: { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 }, previous: [] },
      range: null, statsRange: null, collectionDungeon: null, ...position,
      item: { type: itemType, previous: $state.snapshot(cursors) },
    }, scrollTop: 0, focusedRowId: null });
    const pane = region?.closest('main');
    if (pane) pane.scrollTop = 0;
  }
  function next() {
    if (paging || !page?.next) return;
    paging = true; cursors = [...cursors, page.next].slice(-100); persist();
  }
  function back() {
    if (paging || cursors.length < 2) return;
    paging = true; cursors = cursors.slice(0, -1); persist();
  }
  function first() {
    if (paging || cursor === null) return;
    paging = true; cursors = [null]; persist();
  }
</script>

<BusyStatus message={client ? historyStatus(history, 'item history') : null} />
<div class="item-history" bind:this={region}>
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{missingRow}</p>
  <Breadcrumbs items={[{ label: 'Loot', href: '#/loot' }, { label: name }]} />
  <header>
    <span class="header-tile {tierClass(visual?.item_tier)}" use:itemHover={{ type: itemType, name, tier: visual?.item_tier }}><ItemTile sprite={visual?.sprite ?? null} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} fit={40} alt="" /></span>
    <div class="identity"><h2 class={tierClass(visual?.item_tier)} use:itemHover={{ type: itemType, name, tier: visual?.item_tier }}>{name}</h2>
      {#if item && item.totals.appearances > 0}<p class="subline"><span class={tierClass(visual?.item_tier)}>{visual?.item_tier ?? '—'}</span> · first seen {item.totals.first_at === null ? '—' : shortWhen(item.totals.first_at, now)} · last seen {item.totals.last_at === null ? '—' : clockWhen(item.totals.last_at, now)}</p>{/if}
    </div>
    <div class="actions"><a class="ctl ghost" href={`#/loot/with/${itemType}`}>Show in feed</a>
      {#if client}<PinButton {client} {itemType} {name} {settings} onChange={chase => { if (settings) settings = { ...settings, loot: { ...settings.loot, chase } }; }} />{/if}
      {#if settingsError}<span role="alert">{settingsError}</span><button class="ctl" type="button" aria-label="Retry chase list" onclick={() => settingsRevision++}>Retry</button>{/if}
    </div>
  </header>
  {#if !client}
    <EmptyState state="unavailable" heading="Loot unavailable" description="" />
  {:else}
    {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
    {#if history.retryAvailable && (history.phase === 'error' || history.phase === 'busy')}<div class="error" role="alert"><span>Couldn't load bags.</span><button class="ctl" type="button" onclick={() => controller?.retry()}>Retry</button></div>{/if}
    {#if history.phase === 'error'}<EmptyState state="error" heading="Loot Error" description="" />
    {:else if history.phase === 'busy' && history.retryAvailable}<EmptyState state="unavailable" heading="Loot history busy" description="" />
    {:else if !page}<EmptyState state="loading" heading="Loading item history" description="" />{/if}
    {#if item && page}
      {#if item.totals.appearances === 0 && history.phase !== 'error' && history.phase !== 'busy'}
        <EmptyState heading="Not seen yet" /><p>Maxed hasn't seen this item in any bag yet.</p>
      {:else if item.totals.appearances > 0}
        <div class="kpis"><KpiStrip cells={itemKpis(item, settings?.loot.chase ?? [])} /></div>
        <div class="layout">
          <section class="bags-card"><div class="card-head"><h3>Bags with this item</h3><span>{item.totals.bags}</span></div>
            <div class="table-scroll"><LootBagTable rows={page.rows} {visuals} {now} columns="item" {itemType} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} rowHref={row => row.drop.run_id ? routeHref({ kind: 'run', id: row.drop.run_id }) : null} /></div>
            <footer><span>Click a row to open the run</span><nav aria-label="Item bag pages">
              <button class="ctl ghost" type="button" aria-disabled={paging || cursor === null} onclick={first}>Latest</button>
              <button class="ctl" type="button" aria-disabled={paging || cursors.length < 2} onclick={back}>‹ Newer</button>
              <button class="ctl" type="button" aria-disabled={paging || !page.next} onclick={next}>Older ›</button>
            </nav></footer>
          </section>
          <ItemSideCards {item} {visuals} {now} />
        </div>
      {/if}
    {/if}
    <p class="honesty">Observed bag contents, not pickups. Taken means removal was observed; who took the item is unknown.</p>
  {/if}
</div>

<style>
  .item-history { overflow-anchor: none; display: grid; gap: 10px; min-width: 0; }
  header { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .identity { flex: 1; min-width: 0; }
  h2 { margin: 0; font-size: 22px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .subline { font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .header-tile { flex: none; border: 1px solid var(--border); border-radius: 8px; overflow: hidden; }
  .ut { color: var(--violet-text); } .st { color: var(--bag-orange); }
  .header-tile.ut, .header-tile.st { border-color: currentColor; box-shadow: 0 0 8px #b69aff33; }
  .actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .layout { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: 10px; }
  .bags-card { min-width: 0; padding: 10px 6px 6px; background: var(--surface-panel); border: 1px solid var(--border); border-radius: 8px; display: flex; flex-direction: column; }
  .card-head { display: flex; justify-content: space-between; padding: 2px 6px 10px; color: var(--text-secondary); font-size: 12px; }
  h3 { margin: 0; font-size: 11px; font-weight: 500; text-transform: uppercase; letter-spacing: .07em; }
  .table-scroll { overflow-x: auto; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-top: auto; padding-top: 10px; color: var(--text-muted); font-size: 11px; }
  nav, .error { display: flex; gap: 6px; align-items: center; } .error { color: var(--warning); }
  p { margin: 0; color: var(--text-secondary); } .honesty { font-size: 11.5px; }
  .kpis { min-width: 0; } .kpis :global(.value) { overflow: hidden; text-overflow: ellipsis; }
  .kpis :global([data-kpi="most"] .value) { font-size: 20px; line-height: 27.6px; }
  @media (max-width: 1200px) { .layout { grid-template-columns: minmax(0, 1fr); } }
  @media (max-width: 700px) { header { flex-wrap: wrap; } .actions { width: 100%; } }
</style>

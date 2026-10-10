<script lang="ts">
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { getContext, tick, untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { AppSettings } from '../bindings/AppSettings';
  import type { ChaseItem } from '../bindings/ChaseItem';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import { emptyListState, normalizeListState, type ListState } from '../navigation/state';
  import { createRunHistory, type HistoryController, type HistoryState } from '../runs/history';
  import { createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { objectVisualTypes } from '../bridge/visuals';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import { historyStatus } from '../status/registry';
  import EmptyState from '../components/EmptyState.svelte';
  import CollectionChaseList from './CollectionChaseList.svelte';
  import CollectionGrid from './CollectionGrid.svelte';
  import ItemSearch from './ItemSearch.svelte';
  import PinButton from './PinButton.svelte';
  import KpiStrip from '../home/v3/KpiStrip.svelte';
  import { collectionKpis, chaseOrder, type CollectionSort } from './collection-screen';
  import '../styles/controls.css';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { client, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true }: {
    client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const restored = untrack(() => normalizeListState(savedState));
  let dungeon = $state(restored.lootPosition?.collectionDungeon ?? null);
  let settings = $state.raw<AppSettings | null>(null);
  let saving = $state(false);
  let sort = $state<CollectionSort>(restored.lootPosition?.collectionSort ?? 'seen');
  let resultDungeons = $state<Record<number, string>>({});
  let portals = $state.raw<Map<string, number>>(new Map());
  const nameOf = (type: number) => visuals.objects.find(object => object.object_type === type)?.name ?? `#${type}`;
  let kpiModel = $derived.by(() => {
    const value = readable ? collection : null;
    const { cells, lastType } = collectionKpis(value, settings?.loot.chase ?? [], dungeon);
    const rows = (value?.chase ?? []).filter(row => settings?.loot.chase.some(item => item.item_type === row.item.item_type && item.dungeon === row.item.dungeon));
    const dry = chaseOrder(rows)[0];
    if (dry) cells[1].sub = `runs · ${nameOf(dry.item.item_type)}${dry.item.dungeon === null ? '' : ` in ${dry.item.dungeon}`}`;
    if (lastType !== null) cells[2].value = nameOf(lastType);
    return { cells, lastTier: visuals.objects.find(object => object.object_type === lastType)?.item_tier ?? null };
  });
  let kpis = $derived(kpiModel.cells);
  let settingsError = $state(''), settingsRevision = $state(0);
  let dungeons = $state<string[]>([]), catalogError = $state(''), catalogReady = $state(false), catalogRevision = $state(0);
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading collection.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let region: HTMLDivElement;
  let collection = $derived(history.value?.kind === 'collection' ? history.value.collection : null);
  let readable = $derived(collection !== null && history.phase !== 'loading' && history.phase !== 'error' && history.phase !== 'busy');
  let available = $derived(!client || settingsError !== '' || catalogError !== '' || (catalogReady && collection !== null)
    || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  let releasedEntry: string | undefined;
  let activeClient = $derived(enabled ? client : undefined);
  $effect(() => {
    const position = normalizeListState(savedState).lootPosition;
    untrack(() => {
      dungeon = position?.collectionDungeon ?? dungeons[0] ?? null;
      sort = position?.collectionSort ?? 'seen';
    });
  });
  $effect(() => {
    const owner = activeClient;
    if (!owner) {
      controller = undefined;
      history = { phase: 'loading', value: null, message: 'Loading collection.', retryAvailable: false };
      return;
    }
    const instance = createRunHistory(owner, next => { history = next; }, { cache: historyCache });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller, chase = settings?.loot.chase;
    const input = { query: { kind: 'collection' as const, request: { dungeon, chase: chase ?? [] } }, scopeKey, streamId, required, store };
    if (enabled && chase && (catalogReady || catalogError)) untrack(() => instance?.update(input));
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
  $effect(() => {
    const owner = activeClient; catalogRevision;
    dungeons = []; catalogError = ''; catalogReady = false;
    if (!owner) return;
    let current = true;
    void owner.listCatalogDungeons().then(value => { if (current) { dungeons = value; catalogReady = true; if (dungeon === null && value.length) chooseDungeon(value[0]); } }, () => {
      if (current) catalogError = 'Dungeon choices unavailable.';
    });
    return () => { current = false; };
  });
  $effect(() => {
    const owner = activeClient;
    portals = new Map();
    if (!owner) return;
    let current = true, revision = 0;
    async function load() {
      const request = ++revision;
      try {
        const options = await owner!.listCatalogDungeonOptions();
        if (current && revision === request) portals = new Map(options.filter(row => row.portal_type != null).map(row => [row.name, row.portal_type!]));
      } catch { if (current && revision === request) portals = new Map(); }
    }
    void load();
    const release = owner.onVisualsChanged(() => { void load(); });
    return () => { current = false; release(); };
  });
  $effect(() => {
    const owner = activeClient;
    if (!owner) { visualController = undefined; visuals = emptyHomeVisuals(); return; }
    const instance = createHomeVisuals(owner, next => { visuals = next; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    visualController?.update(objectVisualTypes([...portals.values(), ...(collection?.chase.map(row => row.item.item_type) ?? []), ...(collection?.grid.map(cell => cell.item_type) ?? [])]));
  });
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready || (client && !enabled) || releasedEntry === key) return;
    let current = true;
    void tick().then(() => { if (current) { releasedEntry = key; onRowsReady?.(key); } });
    return () => { current = false; };
  });
  function updateChase(chase: ChaseItem[]) {
    if (settings) settings = { ...settings, loot: { ...settings.loot, chase } };
  }
  function chooseDungeon(next: string | null) {
    dungeon = next;
    persist();
  }
  function changeSort(next: CollectionSort) { sort = next; persist(); }
  function persist() {
    const current = normalizeListState(savedState);
    const position = current.lootPosition ?? restored.lootPosition;
    const scrollTop = region?.closest('main')?.scrollTop ?? current.scrollTop;
    onStateChange?.({ lootPosition: {
      feed: { filter: { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 }, previous: [] },
      range: null, item: null, statsRange: null, ...position, collectionDungeon: dungeon, collectionSort: sort,
    }, scrollTop, focusedRowId: null });
  }
</script>

<BusyStatus message={client && settings ? historyStatus(history, 'collection') : null} />
<div class="collection" bind:this={region}>
  {#if settings}<HistoryStatus {history} />{/if}
  <div class="kpis" data-last-tier={kpiModel.lastTier}><KpiStrip cells={kpis} valueTitles /></div>
  {#if !client}<EmptyState state="unavailable" heading="Loot unavailable" description="" />
  {:else}
    {#if settingsError}<div class="error" role="alert">{settingsError}<button class="ctl" onclick={() => settingsRevision++}>Retry chase list</button></div>
    {:else if !settings}<EmptyState state="loading" heading="Loading chase list" description="" />{/if}
    {#if settings}
      {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
      {#if history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable)}
        <div class="error" role="alert">{history.phase === 'error' ? "Couldn't load collection." : 'Loot history busy'}<button class="ctl" onclick={() => controller?.retry()}>Retry</button></div>
      {:else if !collection}<EmptyState state="loading" heading="Loading collection" description="" />{/if}
    {/if}
    <section class="card chase-card" aria-label="Chase items">
      <div class="card-head"><h3>Chase list</h3>
        <ItemSearch {client} {enabled} placeholder="Add an item to chase">
          {#snippet resultExtra(item)}
            <select class="ctl dungeon-select" aria-label={`Dungeon for ${item.name}`} value={resultDungeons[item.object_type] ?? ''} disabled={saving} onchange={event => { resultDungeons[item.object_type] = event.currentTarget.value; }}>
              <option value="">Any dungeon</option>{#each dungeons as name}<option value={name}>{name}</option>{/each}
            </select>
            <PinButton {client} itemType={item.object_type} dungeon={resultDungeons[item.object_type] || null} name={item.name} variant="icon" {settings} onSaved={updateChase} disabled={saving} onSaving={value => { saving = value; }} />
          {/snippet}
        </ItemSearch>
      </div>
      {#if readable && collection && settings}<CollectionChaseList {client} {settings} rows={collection.chase} {visuals} {portals} onSaved={updateChase} {saving} onSaving={value => { saving = value; }} />{/if}
    </section>
    <section class="card" aria-label="Dungeon collection">
      {#if catalogError}<div class="error" role="alert">{catalogError}<button class="ctl" onclick={() => catalogRevision++}>Retry dungeon choices</button></div>
      {:else if !catalogReady}<p>Loading dungeon choices…</p>
      {:else}<CollectionGrid {dungeon} {dungeons} cells={collection?.grid ?? []} {visuals} {portals} {sort} onSort={changeSort} chase={settings?.loot.chase ?? []} ready={readable && settings !== null && collection?.grid_dungeon === dungeon} onChange={chooseDungeon} />{/if}
    </section>
  {/if}
  <p class="footer">Observed bag contents, not pickups or possessions.</p>
</div>

<style>
  .collection { overflow-anchor: none; display: grid; gap: 10px; min-width: 0; }
  .card { min-width: 0; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); padding: 10px 12px; }
  .chase-card { padding: 8px 6px 6px; }
  .card-head { display: flex; justify-content: space-between; align-items: center; gap: 10px; padding: 2px 6px 8px; }
  h3 { margin: 0; font-size: 11px; font-weight: 500; text-transform: uppercase; letter-spacing: .06em; }
  p { margin: 0; color: var(--text-secondary); font-size: 12px; }
  .error { display: flex; gap: 10px; align-items: center; color: var(--warning); font-size: 12px; }
  .dungeon-select { width: 120px; min-width: 0; }
  .card-head :global(.search ul) { left: auto; right: 0; }
  .kpis :global([data-kpi="last"] .value) { font-size: 20px; line-height: 28px; overflow: hidden; text-overflow: ellipsis; }
  .kpis :global([data-kpi="dry"] .value) { color: var(--estimated); }
  .kpis :global(dt) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kpis[data-last-tier="UT"] :global([data-kpi="last"] .value) { color: var(--violet-text); }
  .kpis[data-last-tier="ST"] :global([data-kpi="last"] .value) { color: var(--bag-orange); }
</style>

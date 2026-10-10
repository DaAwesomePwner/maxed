<script lang="ts">
  import { SHELL_CONNECTION, type ShellConnection } from '../lib/shell/context';
  import BusyStatus from '../lib/status/BusyStatus.svelte';
  import { historyStatus } from '../lib/status/registry';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { getContext, onDestroy, tick, untrack } from 'svelte';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { RunCursor } from '../lib/bindings/RunCursor';
  import type { RunListSummary } from '../lib/bindings/RunListSummary';
  import type { RunFilter } from '../lib/bindings/RunFilter';
  import type { RunSort } from '../lib/bindings/RunSort';
  import { emptyListState, defaultFilters, normalizeListState, type ListState } from '../lib/navigation/state';
  import { routeHref } from '../lib/navigation/routes';
  import { validRunFilter, withRunFilter } from '../lib/runs/filters';
  import { createRunHistory, type HistoryController, type HistoryState } from '../lib/runs/history';
  import HistoryStatus from '../lib/runs/HistoryStatus.svelte';
  import { HISTORY_CATCH_UP } from '../lib/runs/policy';
  import { clearAndDeath } from '../lib/runs/story';
  import KpiStrip from '../lib/home/v3/KpiStrip.svelte';
  import '../lib/home/v3/area-home.css';
  import { dayGroups, listKpis, presetFor, presetRange, runDate, shortDate, signedFame, clearTime, vsBest, type RangePreset } from '../lib/runs/runs-list';
  import { formatStat, formatShare } from '../lib/styles/format';
  import { createDungeonPortal, createHomeVisuals, emptyHomeVisuals } from '../lib/home/visuals';
  import { objectVisual } from '../lib/bridge/visual-index';
  import { spriteUrl } from '../lib/bridge/visuals';
  import SpriteImage from '../lib/components/SpriteImage.svelte';
  import EmptyState from '../lib/components/EmptyState.svelte';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { client, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true }: {
    client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void;
    enabled?: boolean;
  } = $props();
  const restored = untrack(() => normalizeListState(savedState));
  let filter = $state(restored.filters);
  let previous = $state<Array<RunCursor | null>>(restored.runsPosition?.previous ?? []);
  let pageOffset = $state(restored.runsPosition?.offset ?? 0);
  let dungeon = $state(restored.filters.dungeon ?? '');
  let summary = $state.raw<RunListSummary | null>(restored.runsPosition?.summary ?? null);
  let now = $state(new Date());
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => clearTimeout(searchTimer));
  let validation = $state('');
  let missingRow = $state('');
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading saved runs.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let region: HTMLDivElement;
  let page = $derived(history.value?.kind === 'runs' ? history.value.page : null);
  let details = $derived(new Map((page?.details ?? []).map(detail => [detail.run_id, detail])));
  let groups = $derived(dayGroups(page?.rows ?? [], details, now, filter.sort));
  let preset = $derived(presetFor(filter.range, now));
  let total = $derived(summary ? (filter.outcome ? summary[filter.outcome] : summary.runs) : null);
  let visuals = $state.raw(emptyHomeVisuals());
  let portals = $state.raw(new Map<string, number>());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  $effect(() => {
    const owner = client;
    if (!owner || !enabled) return;
    const instance = createHomeVisuals(owner, value => { visuals = value; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const owner = client, names = [...new Set(page?.rows.map(row => row.dungeon) ?? [])];
    if (!owner || !enabled) return;
    portals = new Map();
    const lookups = untrack(() => names.map(name => {
      const lookup = createDungeonPortal(owner, type => {
        const next = new Map(portals);
        if (type === null) next.delete(name); else next.set(name, type);
        portals = next;
      });
      lookup.update(name); return lookup;
    }));
    return () => lookups.forEach(lookup => lookup.dispose());
  });
  $effect(() => {
    const types = [...portals.values(), ...(page?.details ?? []).flatMap(detail => [detail.class_id, ...detail.loot_preview.slice(0, 3).map(bag => bag.bag_type)])];
    visualController?.update(types.filter((type): type is number => type !== null));
  });
  let paging = $state(false);
  let available = $derived(!client || page !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  $effect(() => {
    const owner = client;
    if (!owner || !enabled) {
      controller = undefined;
      history = { phase: 'loading', value: null, message: 'Loading saved runs.', retryAvailable: false };
      return;
    }
    const instance = createRunHistory(owner, next => {
      history = next;
      if (next.phase === 'ready' && next.value?.kind === 'runs') {
        now = new Date();
        if (filter.cursor === null) {
          summary = next.value.page.summary ?? null;
          onStateChange?.({ runsPosition: { previous: $state.snapshot(previous), offset: pageOffset, summary } });
        }
      }
      if (next.phase === 'ready' || next.phase === 'error' || next.phase === 'superseded' || (next.phase === 'busy' && next.retryAvailable)) paging = false;
    }, { cache: historyCache });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller;
    const input = { query: { kind: 'runs' as const, filter: $state.snapshot(filter) }, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  $effect.pre(() => {
    const rows = page?.rows;
    if (!rows || !region) return;
    const active = document.activeElement;
    const id = active instanceof HTMLElement && region.contains(active) ? active.closest<HTMLElement>('[data-row-id]')?.dataset.rowId : null;
    if (!id) return;
    const index = rows.findIndex(row => row.id === id);
    void tick().then(() => {
      if (!region?.isConnected || active?.isConnected) return;
      const pane = region.closest('main');
      if (!pane) return;
      if (index >= 0) {
        [...region.querySelectorAll<HTMLElement>('[data-row-id]')].find(row => row.dataset.rowId === id)?.focus({ preventScroll: true });
        return;
      }
      pane.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
      pane.scrollTop = Math.max(0, Math.min(pane.scrollTop, pane.scrollHeight - pane.clientHeight));
      missingRow = 'Row Removed';
      onStateChange?.({ focusedRowId: null, scrollTop: pane.scrollTop });
    });
  });
  $effect(() => {
    const ready = available, key = entryKey;
    page;
    if (!ready) return;
    let current = true;
    void tick().then(() => { if (current) onRowsReady?.(key); });
    return () => { current = false; };
  });
  function persist() {
    missingRow = '';
    onStateChange?.({ runsWindow: 0, filters: $state.snapshot(filter), runsPosition: { previous: $state.snapshot(previous), offset: pageOffset, summary }, scrollTop: 0, focusedRowId: null });
    const pane = region?.closest('main');
    if (pane) pane.scrollTop = 0;
  }
  function apply(patch: Partial<RunFilter> = {}) {
    clearTimeout(searchTimer);
    now = new Date();
    const next = withRunFilter(filter, { dungeon: dungeon.trim() || null, ...patch });
    if (!validRunFilter(next)) { validation = 'Invalid dungeon'; return; }
    dungeon = next.dungeon ?? ''; validation = ''; previous = []; pageOffset = 0;
    const same = JSON.stringify(next) === JSON.stringify($state.snapshot(filter));
    if (!same) summary = null;
    filter = next; persist();
  }
  function search() { clearTimeout(searchTimer); searchTimer = setTimeout(() => apply(), 300); }
  function searchKey(event: KeyboardEvent) {
    if (event.key === 'Enter') { event.preventDefault(); apply(); }
    if (event.key === 'Escape') { event.preventDefault(); dungeon = ''; apply(); }
  }
  function reset() {
    clearTimeout(searchTimer);
    dungeon = ''; validation = ''; previous = []; pageOffset = 0; summary = null;
    const same = JSON.stringify(filter) === JSON.stringify(defaultFilters());
    filter = defaultFilters(); persist(); if (same) controller?.retry();
  }
  function next() {
    if (paging || !page?.next) return;
    paging = true;
    if (previous.length === 100) pageOffset++;
    previous = [...previous, filter.cursor].slice(-100);
    filter = { ...filter, cursor: page.next }; persist();
  }
  function back() {
    if (paging || !previous.length) return;
    paging = true;
    const cursor = previous[previous.length - 1]; previous = previous.slice(0, -1);
    filter = { ...filter, cursor }; persist();
  }
</script>

{#snippet icon(type: number | null | undefined, alt: string)}
  {@const visual = objectVisual(visuals.objects, type ?? null)}
  <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={20} {alt} />
{/snippet}
<BusyStatus message={client ? historyStatus(history, 'runs') : null} />
<div class="runs area-home" bind:this={region}>
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{missingRow}</p>
  <header class="area-header">
    <div><h2>Runs</h2><p>{formatStat(summary?.total_runs_all_time ?? null)} saved runs · since {summary?.first_started ? shortDate(summary.first_started) : '—'}</p></div>
    <div class="seg ranges" role="group" aria-label="Date range">
      {#each [['all', 'All time'], ['30d', '30 days'], ['7d', '7 days'], ['today', 'Today']] as [value, label]}
        <button type="button" class:on={preset === value} aria-pressed={preset === value} onclick={() => apply({ range: presetRange(value as RangePreset, new Date()) })}>{label}</button>
      {/each}
    </div>
  </header>
  <div class="area-kpis"><KpiStrip cells={listKpis(summary)} /></div>
  <div class="toolbar" role="group" aria-label="Run filters">
    <input class="ctl search" type="search" aria-label="Dungeon" placeholder="Filter by dungeon" bind:value={dungeon} oninput={search} onkeydown={searchKey} />
    <div class="seg outcomes" role="group" aria-label="Outcome">
      {#each [null, 'cleared', 'died', 'left', ...(summary && summary.unknown > 0 ? ['unknown'] as const : [])] as value}
        <button type="button" class:on={filter.outcome === value} aria-pressed={filter.outcome === value} onclick={() => apply({ outcome: value as RunFilter['outcome'] })}>
          {value === null ? 'All' : value[0].toUpperCase() + value.slice(1)} <span>{formatStat(summary ? summary[(value ?? 'runs') as 'runs' | 'cleared' | 'died' | 'left' | 'unknown'] : null)}</span>
        </button>
      {/each}
    </div>
    <select class="ctl characters" aria-label="Character" value={filter.character_id ?? ''} onchange={event => apply({ character_id: event.currentTarget.value || null })}>
      <option value="">Any character</option>
      {#each summary?.characters ?? [] as character}<option value={character.id}>{character.name ?? '—'}</option>{/each}
      {#if filter.character_id && !summary?.characters.some(character => character.id === filter.character_id)}<option value={filter.character_id}>Selected character</option>{/if}
    </select>
    <label class="sort">Sort <select class="ctl" value={filter.sort} onchange={event => apply({ sort: event.currentTarget.value as RunSort })}>
      <option value="newest">Newest first</option><option value="oldest">Oldest first</option><option value="duration_ascending">Shortest first</option><option value="duration_descending">Longest first</option>
    </select></label>
  </div>
  {#if validation}<p role="alert">{validation}</p>{/if}
  {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<DebugNote text={history.message} />{/if}
  {#if history.retryAvailable && history.phase !== 'ready'}<button class="ctl retry" type="button" onclick={() => controller?.retry()}>Retry</button>{/if}
  <section class="area-card list-card" aria-label="Saved runs">
    {#if !client}<EmptyState state="unavailable" heading="No runs" description="" />
    {:else if history.phase === 'error'}<EmptyState state="error" heading="Run error" description="" />
    {:else if !page}<EmptyState state="loading" heading="Loading runs" description="" />
    {:else if !page.rows.length}
      <div class="empty"><p>No runs match these filters</p><button class="ctl" type="button" onclick={reset}>Clear filters</button></div>
    {/if}
    {#if page?.rows.length}
      <div class="table" aria-label="Runs">
        <div class="row labels" aria-hidden="true"><span>Dungeon</span><span>Result</span><span class="number">Clear time</span><span class="number comparison">vs best</span><span>Character</span><span>Your share</span><span class="number">Fame</span><span class="loot">Loot</span><span class="number">Started</span></div>
        {#each groups as group (group.key)}
          {#if group.label !== null}<div class="day"><b>{group.label}</b> · {group.runs} runs · {group.cleared} cleared · {signedFame(group.fame)} fame</div>{/if}
          {#each group.rows as run (run.id)}
            {@const detail = details.get(run.id)}
            {@const comparison = vsBest(detail)}
            {@const loot = detail?.loot_preview.slice(0, 3) ?? []}
            <a class="row" data-row-id={run.id} href={routeHref({ kind: 'run', id: run.id })}>
              <span class="cell" title={run.dungeon}>{@render icon(portals.get(run.dungeon), '')}<strong class="name">{run.dungeon}</strong></span>
              <span class="result" title={clearAndDeath(run) ? 'Clear and death' : undefined} class:good={run.outcome === 'cleared'} class:bad={run.outcome === 'died'}>
                {run.outcome === null ? (run.ended === null ? 'In progress' : '—') : run.outcome[0].toUpperCase() + run.outcome.slice(1)}
                {#if clearAndDeath(run)}<small class="warning tag">Clear and death</small>{/if}
              </span>
              <span class="number">{#if detail?.is_personal_best}<span class="pb">PB</span>{/if}{clearTime(detail?.clear_micros)}</span>
              <span class="number comparison" class:good={comparison?.tone === 'good'}>{comparison?.text ?? '—'}</span>
              <span class="cell" title={detail?.character_name ?? undefined}>{@render icon(detail?.class_id, '')}<span class="name">{detail?.character_name ?? '—'}</span></span>
              <span class="share">{#if detail?.local_share != null}<i aria-hidden="true"><b style:width={`${Math.max(0, Math.min(1, detail.local_share)) * 100}%`}></b></i>{/if}{formatShare(detail?.local_share ?? null)}{#if detail?.share_partial}<small class="tag" title="Partial share">partial</small>{/if}</span>
              <span class="number" class:gold={detail?.fame_delta != null}>{signedFame(detail?.fame_delta ?? null)}</span>
              <span class="loot bags">{#if loot.length}{#each loot as bag}{@render icon(bag.bag_type, 'Observed bag')}{/each}{:else}—{/if}{#if detail && detail.observed_drop_count > loot.length}<small>+{detail.observed_drop_count - loot.length}</small>{/if}</span>
              <span class="number started">{runDate(run.started).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })}{#if !run.complete_capture || run.issues.length}<small class="tag">partial</small>{/if}</span>
            </a>
          {/each}
        {/each}
      </div>
    {/if}
    <footer><span>Click a row to open the run</span><nav aria-label="Run Pages">
      <span>{page?.rows.length ? `${(pageOffset + previous.length) * 25 + 1}–${(pageOffset + previous.length) * 25 + page.rows.length}` : '0'} of {formatStat(total)}</span>
      <button class="ctl sq" type="button" aria-label="Previous" aria-disabled={paging || previous.length === 0} onclick={back}>‹</button>
      <button class="ctl sq" type="button" aria-label="Next" aria-disabled={paging || !page?.next} onclick={next}>›</button>
    </nav></footer>
    {#if pageOffset > 0 || previous.length >= 100}<p class="cap">Only the 100 previous pages are retained.</p>{/if}
  </section>
</div>
<style>
  .runs.area-home { overflow-anchor: none; overflow-wrap: anywhere; height: auto; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .ranges { margin-left: auto; }
  .toolbar { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .ctl.search { width: 230px; min-width: 90px; flex: 0 1 230px; }
  .outcomes { flex: none; }
  .outcomes span { opacity: .6; margin-left: 3px; }
  .characters { min-width: 0; max-width: 150px; text-overflow: ellipsis; }
  .sort { display: flex; gap: 6px; align-items: center; margin-left: auto; color: var(--text-muted); }
  .sort select { max-width: 130px; }
  .retry { align-self: start; }
  .runs .list-card { padding: 8px 6px 6px; }
  .table { min-width: 0; }
  .row { display: grid; grid-template-columns: minmax(0, 2.2fr) minmax(0, 1.25fr) 70px 58px minmax(0, 1.35fr) 84px 54px 78px 116px; gap: 8px; align-items: center; min-height: 30px; padding: 0 6px; font-variant-numeric: tabular-nums; }
  a.row { height: 30px; color: var(--text-primary); text-decoration: none; border-bottom: 1px solid #ffffff06; }
  a.row:hover, a.row:focus-visible { background: var(--surface-hover); }
  .row > span { min-width: 0; }
  .labels { min-height: 23px; color: var(--text-muted); font-size: 10px; text-transform: uppercase; white-space: nowrap; }
  .day { padding: 9px 6px 4px; color: var(--text-muted); font-size: 11px; }
  .day b { color: var(--text-secondary); }
  .cell { display: flex; align-items: center; gap: 6px; overflow: hidden; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 400; }
  .number { text-align: right; white-space: nowrap; }
  .good { color: var(--success); } .bad { color: var(--danger); } .warning.tag { color: var(--warning); font-size: 9px; }
  .result { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pb { color: var(--gold); border: 1px solid #f2c14e45; background: #f2c14e14; padding: 0 3px; border-radius: 3px; font-size: 9px; margin-right: 4px; }
  .share { display: flex; gap: 4px; align-items: center; font-size: 11px; white-space: nowrap; }
  .share i { width: 30px; min-width: 10px; flex: 0 1 30px; height: 4px; background: var(--surface-raised); border-radius: 2px; overflow: hidden; }
  .share b { display: block; height: 100%; background: var(--violet); }
  .bags { display: flex; align-items: center; gap: 1px; }
  .bags small, .started { color: var(--text-muted); }
  .tag { color: var(--text-muted); font-size: 9px; border: 1px solid var(--border); border-radius: 3px; padding: 0 3px; margin-left: 3px; }
  footer, footer nav { display: flex; align-items: center; gap: 6px; }
  footer { margin-top: 8px; padding: 6px 6px 0; color: var(--text-muted); font-size: 11px; }
  footer nav { margin-left: auto; font-variant-numeric: tabular-nums; }
  footer nav > span { margin-right: 4px; }
  .empty { padding: 20px; text-align: center; color: var(--text-muted); }
  .cap { color: var(--text-muted); font-size: 11px; margin: 6px; }
  @media (max-width: 1200px) {
    .row { grid-template-columns: minmax(0, 2.2fr) minmax(0, 1.25fr) 70px 58px minmax(0, 1.35fr) 84px 52px 116px; gap: 5px; }
    .loot { display: none; }
    .ctl.search { flex-basis: 160px; width: 160px; }
    .characters { max-width: 115px; }
    .toolbar { gap: 5px; }
  }
  @media (max-width: 1000px) {
    .toolbar { gap: 4px; }
    .ctl.search { min-width: 80px; }
    .characters { max-width: 100px; }
    .sort { gap: 4px; }
    .sort select { max-width: 104px; }
    .outcomes :global(button) { padding-inline: 5px; gap: 3px; }
    .row { grid-template-columns: minmax(0, 1.6fr) minmax(0, .9fr) 64px minmax(0, 1fr) 78px 48px 110px; gap: 4px; }
    .comparison { display: none; }
  }
</style>

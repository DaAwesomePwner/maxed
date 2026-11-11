<script lang="ts">
  import BusyStatus from '../lib/status/BusyStatus.svelte';
  import { historyStatus } from '../lib/status/registry';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { getContext, tick, untrack } from 'svelte';
  import { readable } from 'svelte/store';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { AlertCursor } from '../lib/bindings/AlertCursor';
  import type { AlertKind } from '../lib/bindings/AlertKind';
  import type { CaptureStatus } from '../lib/bindings/CaptureStatus';
  import type { LiveSnapshot } from '../lib/bindings/LiveSnapshot';
  import type { AlertSession } from '../lib/alerts/session';
  import { SHELL_CONNECTION, type ShellConnection } from '../lib/shell/context';
  import { emptyListState, normalizeListState, type ListState } from '../lib/navigation/state';
  import { createAlertHistory, type AlertHistoryController, type AlertHistoryState } from '../lib/alerts/history';
  import { alertKindDisplayOrder, alertKindLabels, defaultAlertFilter, withAlertFilter, dateRange, filterDays } from '../lib/alerts/filters';
  import { DENSE_ALERT_ROWS } from '../lib/phase2/contracts';
  import { createHomeVisuals, emptyHomeVisuals } from '../lib/home/visuals';
  import AlertLog from '../lib/alerts/AlertLog.svelte';
  import AlertPreferences from '../lib/alerts/AlertPreferences.svelte';
  import EmptyState from '../lib/components/EmptyState.svelte';
  let { onHistoryApplied, client, preview = false, showPreferences = true, enabled = true, streamId = null, scopeKey = '', required = null, store = null,
    savedState = emptyListState(), onStateChange, entryKey = '', onRowsReady, alertSession }: {
    onHistoryApplied?: (key: string, required: string | null) => void;
    client?: Client; preview?: boolean; showPreferences?: boolean; enabled?: boolean; streamId?: string | null;
    scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; alertSession?: AlertSession;
  } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  const capture = connection?.capture ?? readable<CaptureStatus | null>(null);
  const snapshot = connection?.snapshot ?? readable<LiveSnapshot | null>(null);
  const restored = untrack(() => normalizeListState(savedState));
  const initialFilter = restored.alertsPosition?.filter ?? defaultAlertFilter();
  let filter = $state(initialFilter);
  let previous = $state<Array<AlertCursor | null>>(restored.alertsPosition?.previous ?? []);
  let window = $state(restored.alertsPosition?.window ?? 0);
  let kind = $state<AlertKind | ''>(initialFilter.kind ?? ''), query = $state(initialFilter.query);
  const days = filterDays(initialFilter.range);
  let from = $state(days[0]), through = $state(days[1]), validation = $state('');
  let history = $state.raw<AlertHistoryState>({ phase: 'loading', page: null, message: '', retryAvailable: false });
  let readRequired: string | null = null;
  let controller = $state.raw<AlertHistoryController>();
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let visuals = $state.raw(emptyHomeVisuals());
  let region: HTMLDivElement;
  let announcement = $state(''), missingRow = $state('');
  let failed = false, restorationPending = true;
  let rows = $derived(history.page?.rows ?? []);
  let lastWindow = $derived(Math.max(0, Math.ceil(rows.length / DENSE_ALERT_ROWS) - 1));
  let visible = $derived(rows.slice(window * DENSE_ALERT_ROWS, (window + 1) * DENSE_ALERT_ROWS));
  let paging = $derived(history.phase !== 'ready');
  let ready = $derived(!client || !enabled || history.page !== null || history.phase === 'error'
    || history.phase === 'superseded' || (history.phase === 'stale' && store?.state === 'failed')
    || (history.phase === 'busy' && history.retryAvailable));
  let live = $derived(enabled && $capture?.state === 'Live' && $snapshot !== null
    && $snapshot.stream_id === streamId && streamId.startsWith('live-') && /^\d+$/.test($snapshot.at) && store?.state !== 'failed');
  $effect(() => {
    const owner = client, active = enabled;
    if (!owner || !active) { controller = undefined; history = { phase: 'loading', page: null, message: '', retryAvailable: false }; return; }
    const instance = createAlertHistory(owner, next => { history = next; if (next.phase === 'ready') onHistoryApplied?.(entryKey, readRequired); });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller, input = { filter: $state.snapshot(filter), scopeKey, streamId, required, store };
    if (enabled) untrack(() => { readRequired = input.required; instance?.update(input); });
  });
  $effect(() => {
    const owner = client, active = enabled, namespace = scopeKey, stream = streamId;
    void namespace; void stream;
    visuals = emptyHomeVisuals(); visualController = undefined;
    if (!owner || !active) return;
    const instance = createHomeVisuals(owner, next => { visuals = next; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const types = visible.flatMap(row => row.details?.type === 'white_bag'
      ? [row.details.bag_type, ...(row.details.source_enemy === null ? [] : [row.details.source_enemy])]
      : row.details?.type === 'personal_best' ? [row.details.boss_type] : []);
    const instance = visualController;
    untrack(() => instance?.update(types));
  });
  $effect.pre(() => {
    const page = history.page;
    if (!page) return;
    untrack(() => {
      let selected = restorationPending ? restored.focusedRowId : null;
      const active = document.activeElement;
      if (!selected && active instanceof HTMLElement && region?.contains(active)) selected = active.closest<HTMLElement>('[data-row-id]')?.dataset.rowId ?? null;
      const index = selected ? page.rows.findIndex(row => row.id === selected) : -1;
      window = index >= 0 ? Math.floor(index / DENSE_ALERT_ROWS) : Math.min(window, Math.max(0, Math.ceil(page.rows.length / DENSE_ALERT_ROWS) - 1));
      restorationPending = false;
      if (selected && index < 0 && active instanceof HTMLElement && region?.contains(active)) {
        void tick().then(() => {
          if (!region?.isConnected || active.isConnected || (document.activeElement !== document.body && document.activeElement !== active)) return;
          region.closest('main')?.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
          missingRow = 'Alert removed. Heading focused.';
          onStateChange?.({ focusedRowId: null });
        });
      }
      onStateChange?.({ alertsPosition: { filter: $state.snapshot(filter), previous: $state.snapshot(previous), window } });
    });
  });
  $effect(() => {
    const available = ready, key = entryKey, page = history.page, position = window, namespace = scopeKey, stream = streamId;
    const receive = onRowsReady;
    void page; void position; void namespace; void stream;
    if (!available) return;
    let current = true;
    void tick().then(() => { if (current) receive?.(key); });
    return () => { current = false; };
  });
  $effect(() => {
    const failure = history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable)
      || (history.phase === 'stale' && history.message.startsWith('Saving failed.'));
    if (failure && !failed) { failed = true; announcement = 'Couldn’t load alerts. Retry.'; }
    else if (failed && history.phase === 'ready') { failed = false; announcement = 'Alerts Loaded'; }
  });
  function persist() {
    missingRow = ''; restorationPending = false;
    onStateChange?.({ alertsPosition: { filter: $state.snapshot(filter), previous: $state.snapshot(previous), window }, focusedRowId: null, scrollTop: 0 });
  }
  function apply(event: SubmitEvent) {
    event.preventDefault();
    try {
      const next = withAlertFilter(filter, { kind: kind || null, query, range: dateRange(from, through) });
      const same = JSON.stringify(next) === JSON.stringify($state.snapshot(filter));
      filter = next; query = next.query; previous = []; window = 0; validation = ''; persist();
      if (same) controller?.retry();
    } catch (error) {
      // Filter helpers throw RangeError with app-written guidance; anything else gets fixed text.
      validation = error instanceof RangeError ? error.message : 'Check filters.';
    }
  }
  function reset() {
    const same = JSON.stringify(filter) === JSON.stringify(defaultAlertFilter());
    filter = defaultAlertFilter(); kind = ''; query = ''; from = ''; through = ''; previous = []; window = 0; validation = ''; persist();
    if (same) controller?.retry();
  }
  function moveWindow(delta: number) {
    window = Math.max(0, Math.min(lastWindow, window + delta)); persist();
  }
  function nextPage() {
    if (paging || window !== lastWindow || !history.page?.next) return;
    previous = [...previous, filter.cursor].slice(-100); filter = { ...filter, cursor: history.page.next }; window = 0; persist();
  }
  function previousPage() {
    if (paging || window !== 0 || !previous.length) return;
    filter = { ...filter, cursor: previous[previous.length - 1] }; previous = previous.slice(0, -1); window = 0; persist();
  }
  function firstPage() {
    if (paging || filter.cursor === null) return;
    filter = { ...filter, cursor: null }; previous = []; window = 0; persist();
  }
</script>

<BusyStatus message={client && enabled ? historyStatus(history, 'alerts') : null} />

<div class="alerts" class:history-only={!showPreferences} bind:this={region}>
  <p class="sr-only" role="status" aria-atomic="true">{announcement}</p>
  <p class="sr-only" role="status">{missingRow}</p>
  <section class="history" aria-label="Alert Log">
    <h2>History</h2>
    <form onsubmit={apply} aria-label="Alert Filters">
      <label>Search<input type="search" bind:value={query} placeholder="Search alerts" /></label>
      <div class="filter-actions"><label>Kind<select bind:value={kind}><option value="">All kinds</option>{#each alertKindDisplayOrder as value}<option value={value}>{alertKindLabels[value]}</option>{/each}</select></label><button type="submit">Apply</button></div>
      <details open={!!validation || !!filter.range}><summary>Dates</summary><div class="dates"><label>From<input type="date" bind:value={from} /></label><label>Through<input type="date" bind:value={through} /></label><button type="button" onclick={reset}>Reset</button></div></details>
      {#if validation}<p role="alert">{validation}</p>{/if}
    </form>
    {#if !client || !enabled}<EmptyState state="unavailable" heading="Disconnected" description="" />
    {:else}
      {#if history.phase === 'error'}<p class="message">Couldn’t load alerts</p>
      {:else if history.phase === 'stale' && store?.state === 'failed'}<p class="message">Couldn’t save</p>
      {:else if history.phase === 'superseded'}<p class="message">Waiting</p>
      {:else if history.phase === 'busy' && history.retryAvailable}<p class="message">Busy</p>
      {:else if !history.page && history.phase === 'loading'}<EmptyState state="loading" heading="Loading alerts" description="" />{/if}
      {#if history.phase !== 'ready' && !historyStatus(history, 'alerts')}<DebugNote text={history.message} />{/if}
      {#if history.page && rows.length === 0}<EmptyState heading="No alerts" description="" />
      {:else if rows.length}<AlertLog rows={visible} {visuals} />{/if}
      <div class="rows-footer"><span>{rows.length ? `${window * DENSE_ALERT_ROWS + 1}-${Math.min(rows.length, (window + 1) * DENSE_ALERT_ROWS)} of ${rows.length}` : '0 rows'}</span><nav aria-label="Alert Rows"><button type="button" disabled={window === 0 || !rows.length} onclick={() => moveWindow(-1)}>Previous rows</button><button type="button" disabled={window >= lastWindow || !rows.length} onclick={() => moveWindow(1)}>Next rows</button></nav></div>
      <details><summary>Pages</summary><button type="button" disabled={history.phase !== 'ready'} onclick={() => controller?.retry()}>Refresh</button><nav aria-label="Alert Pages"><button type="button" disabled={paging || filter.cursor === null} onclick={firstPage}>First page</button><button type="button" disabled={paging || window !== 0 || !previous.length} onclick={previousPage}>Previous page</button><button type="button" disabled={paging || window !== lastWindow || !history.page?.next} onclick={nextPage}>Next page</button></nav></details>
      {#if history.phase !== 'ready' && history.retryAvailable && !historyStatus(history, 'alerts')}<button class="retry" type="button" onclick={() => controller?.retry()}>Retry</button>{/if}
    {/if}
  </section>
  {#if showPreferences}
    {#key `${scopeKey}:${streamId}:${preview}`}
      <AlertPreferences {client} session={alertSession ?? connection?.alerts} {preview} {enabled} {live} />
    {/key}
  {/if}
</div>

<style>
  .alerts { display: grid; grid-template-columns: minmax(0, 1.05fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; min-width: 0; overflow-anchor: none; }
  .history-only { grid-template-columns: minmax(0, 1fr); }
  .history { padding: var(--space-5); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); min-width: 0; overflow-wrap: anywhere; }
  h2 { margin: 0 0 var(--space-3); font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); }
  form { display: grid; gap: var(--space-2); }
  label { display: grid; gap: var(--space-1); min-width: 0; color: var(--text-secondary); font-size: var(--text-caption); }
  input, select, button { min-width: 0; max-width: 100%; }
  button:not(:disabled), summary { cursor: pointer; }
  .filter-actions, .dates, nav { display: flex; gap: var(--space-2); flex-wrap: wrap; align-items: end; }
  .filter-actions label { flex: 1; }
  .rows-footer { display: grid; gap: var(--space-2); margin-top: var(--space-2); color: var(--text-muted); font-size: var(--text-caption); font-variant-numeric: tabular-nums; }
  .rows-footer nav { justify-content: space-between; }
  .message { font-size: var(--text-label); color: var(--text-secondary); margin: var(--space-1) 0; }
  summary { min-height: var(--target-min); align-content: center; font-size: var(--text-label); color: var(--text-secondary); }
  summary:hover { color: var(--text-primary); }
  .retry { margin-top: var(--space-1); justify-self: start; }
  @media (max-width: 900px) { .alerts { grid-template-columns: minmax(0, 1fr); } }
</style>

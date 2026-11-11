<script lang="ts">
  import { isPresentationVisible, observePresentationVisibility } from '../bridge/visibility';
  import { getContext } from 'svelte';
  import type { SetupStatus } from '../bindings/SetupStatus';
  import type { Client } from '../bridge/client';
  import type { PartyPlayerRow } from '../bindings/PartyPlayerRow';
  import type { PartyPlayerFilter } from '../bindings/PartyPlayerFilter';
  import type { PartyPlayerSort } from '../bindings/PartyPlayerSort';
  import type { PartyPlayersCursor } from '../bindings/PartyPlayersCursor';
  import type { PartyMember } from '../bindings/PartyMember';
  import type { EventEnvelope } from '../bindings/EventEnvelope';
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { objectVisual } from '../bridge/visual-index';
  import { spriteUrl } from '../bridge/visuals';
  import { routeHref } from '../navigation/routes';
  import { formatShare } from '../styles/format';
  import { historyWaiting } from '../runs/policy';
  import { invalidatesParty } from './invalidation';
  import { partyDate } from './party-screen';
  import SpriteImage from '../components/SpriteImage.svelte';
  import Badge from '../components/Badge.svelte';
  import PlayerSidePanel from './PlayerSidePanel.svelte';
  import ClearHistoryDialog from './ClearHistoryDialog.svelte';
  import '../styles/controls.css';
  let { client, playerKey, preview = false }: { client: Client; playerKey?: string; preview?: boolean } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  let setup = $state<SetupStatus | null>(null), setupError = $state('');
  $effect(() => {
    const owner = client;
    let disposed = false;
    setup = null; setupError = '';
    void owner.querySetup().then(reply => { if (!disposed) setup = reply; })
      .catch(() => { if (!disposed) setupError = 'Couldn’t check demo mode. Reload to try again.'; });
    return () => { disposed = true; };
  });
  let clearHint = $derived(preview ? 'Clearing is not available in preview.' : setupError
    || (setup?.demo ? 'Turn off demo mode to clear your party history.' : ''));
  const droppedRows = new Map<string, PartyPlayerRow>();
  const filters: [PartyPlayerFilter, string][] = [['all', 'All'], ['watched', 'Watched'], ['banned', 'Banned'], ['with_notes', 'With notes']];
  const columns: [PartyPlayerSort, string][] = [['runs_together', 'Runs together'], ['deaths', 'Deaths'], ['left_early', 'Left early'], ['avg_share', 'Avg share'], ['last_seen', 'Last seen']];
  let search = $state(''), query = $state(''), filter = $state<PartyPlayerFilter>('all'), sort = $state<PartyPlayerSort>('last_seen');
  let rows = $state.raw<PartyPlayerRow[]>([]), next = $state.raw<PartyPlayersCursor | null>(null);
  let busy = $state(false), error = $state(''), waiting = $state(''), clearOpen = $state(false);
  let revision = $state(0), detailRevision = $state(0), detailReset = $state(0);
  let loadMore = $state<() => void>(() => {}), retryHistory = $state<() => void>(() => {});
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let navigationLink: HTMLAnchorElement, clearButton: HTMLButtonElement;
  let clearError = $state('');
  let previousQuery = '', previousClient: Client | undefined, pages = 0;
  function closeClear() { clearOpen = false; clearButton?.focus(); }
  let selected = $derived.by((): PartyMember | null => {
    if (!playerKey) return null;
    const row = rows.find(row => row.key === playerKey);
    return { player_id: playerKey, name_key: playerKey, name: row?.name, class_id: row?.last_class_id,
      maxed: row?.last_maxed, mark: row?.mark, runs_together: row?.runs_together ?? 0, equipment: [], is_local: false, flags: [] };
  });
  $effect(() => {
    const value = search;
    const timer = setTimeout(() => { query = value; }, 250);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    const owner = client, term = query, chosenFilter = filter, chosenSort = sort; revision;
    const signature = JSON.stringify([term, chosenFilter, chosenSort, detailReset]);
    const refreshing = previousClient === owner && previousQuery === signature;
    previousClient = owner; previousQuery = signature;
    let disposed = false, reading = false;
    if (!refreshing) { rows = []; next = null; pages = 0; droppedRows.clear(); }
    error = '';
    async function read(cursor: PartyPlayersCursor | null) {
      if (reading || disposed) return;
      reading = true; busy = true; error = '';
      try {
        const reply = await owner.getPartyPlayers(term, chosenFilter, chosenSort, cursor);
        if (disposed) return;
        if (reply.status !== 'ready') { error = 'Player history is busy. Retry to refresh it.'; return; }
        if (!cursor) for (const row of reply.value.rows) droppedRows.delete(row.key);
        if (!cursor && refreshing && pages > 1) {
          // Keep loaded pages visible. Rows beyond page 1 keep their previous values until a full reload.
          const refreshed = new Map(reply.value.rows.map(row => [row.key, row]));
          rows = [...reply.value.rows, ...rows.filter(row => !refreshed.has(row.key))];
        } else {
          rows = [...new Map([...(cursor ? rows : []), ...reply.value.rows].map(row => [row.key, row])).values()];
          next = reply.value.next ?? null;
          pages = cursor ? pages + 1 : 1;
        }
      } catch { if (!disposed) error = 'Couldn’t load players. Retry to refresh them.'; }
      finally { reading = false; if (!disposed) busy = false; }
    }
    loadMore = () => { if (next) void read(next); };
    void read(null);
    return () => { disposed = true; };
  });
  $effect(() => {
    const owner = client;
    let disposed = false, checking = false, retries = 0;
    let pending: { streamId: string; required: string } | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let release: (() => void) | undefined;
    async function check() {
      if (disposed || checking || !pending || !isPresentationVisible()) return;
      checking = true;
      const required = pending;
      try {
        const message = historyWaiting({ ...required, store: await owner.queryStore() });
        if (disposed) return;
        waiting = message ?? '';
        if (!message && pending === required) { pending = null; revision++; detailRevision++; }
      } catch { if (!disposed) waiting = 'Couldn’t check saved history. Retry to refresh it.'; }
      finally {
        checking = false;
        if (!disposed && isPresentationVisible() && pending && ++retries <= 3) timer = setTimeout(() => { timer = undefined; void check(); }, 1000);
      }
    }
    const receive = (event: EventEnvelope) => {
      if (!invalidatesParty({ kind: 'party-players' }, event)) return;
      pending = { streamId: event.stream_id, required: event.sequence }; retries = 0;
      clearTimeout(timer); void check();
    };
    retryHistory = () => { retries = 0; clearTimeout(timer); if (pending) void check(); else { revision++; detailRevision++; } };
    if (connection?.onEvent) release = connection.onEvent(receive);
    else void owner.onEngineEvent(receive, batch => batch.events.forEach(receive)).then(unsubscribe => {
      if (disposed) unsubscribe(); else release = unsubscribe;
    }).catch(() => { if (!disposed) waiting = 'Player updates unavailable. Retry to refresh history.'; });
    const releaseVisibility = observePresentationVisibility(visible => {
      clearTimeout(timer); timer = undefined;
      if (visible) void check();
    });
    return () => { disposed = true; releaseVisibility(); clearTimeout(timer); release?.(); };
  });
  $effect(() => {
    const controller = createHomeVisuals(client, next => { visuals = next; }); visualController = controller;
    return () => controller.dispose();
  });
  $effect(() => { visualController?.update(rows.map(row => row.last_class_id).filter((id): id is number => id != null)); });
  function navigate(key?: string) {
    navigationLink.href = routeHref(key ? { kind: 'party-player', key } : { kind: 'party-players' }); navigationLink.click();
  }
  function matchesFilter(row: Pick<PartyPlayerRow, 'mark' | 'note'>) {
    if (filter === 'watched' || filter === 'banned') return row.mark === filter;
    if (filter === 'with_notes') return !!row.note;
    return true;
  }
</script>
<section aria-label="Party players">
  <a bind:this={navigationLink} href="#/party/players" hidden tabindex="-1">Player history</a>
  <header>
    <label>Find a player<input class="ctl" type="search" bind:value={search} /></label>
    <div class="filters seg" role="group" aria-label="Player filters">{#each filters as [value, label]}<button aria-pressed={filter === value} onclick={() => { filter = value; }}>{label}</button>{/each}</div>
    <button class="ctl clear" bind:this={clearButton} disabled={preview || !setup || setup.demo}
      aria-describedby={clearHint ? 'players-clear-hint' : undefined} onclick={() => { clearError = ''; clearOpen = true; }}>Clear party history…</button>
  </header>
  {#if clearHint}<p class="hint" id="players-clear-hint">{clearHint}</p>{/if}
  {#if clearError && !clearOpen}<p role="alert">{clearError}</p>{/if}
  {#if waiting}<p role="status">{waiting}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if error || waiting}<button class="ctl" onclick={() => retryHistory()}>Retry</button>{/if}
  <div class="layout" class:with-panel={selected !== null}>
    <div class="panel">
      <div class="table-scroll"><table aria-label="Party players" aria-busy={busy}>
        <thead><tr><th>Player</th><th>Last class</th><th>Last maxed</th>
          {#each columns as [value, label]}<th aria-sort={sort === value ? 'descending' : 'none'}><button class="sort" onclick={() => { sort = value; }}>{label}</button></th>{/each}<th>Note</th>
        </tr></thead>
        <tbody>{#each rows as row (row.key)}
          {@const visual = objectVisual(visuals.objects, row.last_class_id)}
          <tr tabindex="0" aria-selected={playerKey === row.key} onclick={() => navigate(row.key)} onkeydown={event => { if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); navigate(row.key); } }}>
            <td><span class="player">{#if visual?.sprite}<SpriteImage sprite={visual.sprite} sheetUrl={spriteUrl(visuals.client_fingerprint, visual.sprite)} alt={visual.name ?? 'Class'} fit={18} />{/if}<strong>{row.name}</strong>
              {#if row.mark}<Badge label={row.mark === 'watched' ? 'Watched' : 'Banned'} tone={row.mark === 'watched' ? 'warning' : 'danger'} />{/if}</span></td>
            <td>{visual?.name ?? 'Unknown class'}</td><td>{row.last_maxed == null ? 'Not seen' : `${row.last_maxed}/8`}</td>
            <td class="number">{row.runs_together}</td><td class="number">{row.deaths}</td><td class="number">{row.left_early}</td><td class="number">{formatShare(row.avg_share ?? null)}</td>
            <td>{partyDate(row.last_seen)}<small>{row.last_map ?? 'Map not seen'}</small></td><td class="note" title={row.note ?? undefined}>{row.note ?? ''}</td>
          </tr>
        {/each}</tbody>
      </table></div>
      {#if busy}<p role="status">Loading players</p>{:else if !rows.length && !error}<p>No players match this filter.</p>{/if}
      {#if next}<button class="ctl" disabled={busy} onclick={() => loadMore()}>Load more</button>{/if}
      <p class="hint">Matched by name; a renamed player starts a new row. Runs together counts dungeon runs.</p>
    </div>
    {#if selected}{#key `${playerKey}:${detailReset}`}<PlayerSidePanel member={selected} runId={null} {client} {visuals} fullRuns refreshToken={detailRevision}
      resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} onChanged={change => {
        const missing = !rows.some(row => row.key === change.key);
        const previous = rows.find(row => row.key === change.key) ?? droppedRows.get(change.key);
        if (previous) {
          const updated = { ...previous, ...change };
          if (matchesFilter(updated)) {
            droppedRows.delete(change.key);
            rows = missing ? [...rows, updated] : rows.map(row => row.key === change.key ? updated : row);
          } else {
            droppedRows.set(change.key, updated);
            rows = rows.filter(row => row.key !== change.key);
          }
        }
        // Refresh page 1 so a newly matching player returns in the server's search and sort order.
        if (missing && matchesFilter(change)) revision++;
      }} onClose={() => navigate()} />{/key}{/if}
  </div>
  {#if clearOpen}<ClearHistoryDialog {client} onCancel={closeClear} onError={message => { clearError = message; }} onCleared={() => { closeClear(); clearError = ''; droppedRows.clear(); revision++; detailReset++; }} />{/if}
</section>
<style>
  header, .player { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); } header { margin-bottom: var(--space-4); }
  header label { display: grid; gap: var(--space-1); color: var(--text-secondary); font-size: var(--text-caption); } .clear { margin-left: auto; }
  section { min-width: 0; } .layout { display: grid; grid-template-columns: minmax(0, 1fr); min-width: 0; gap: var(--space-4); align-items: start; } .with-panel { grid-template-columns: minmax(0, 1fr) 290px; }
  .panel { min-width: 0; padding: var(--space-4); background: var(--surface-panel); border: 1px solid var(--hairline); border-radius: var(--radius-card); } .table-scroll { max-width: 100%; overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-size: var(--text-caption); line-height: var(--leading-caption); font-variant-numeric: tabular-nums; }
  th, td { text-align: left; padding: var(--space-2); white-space: nowrap; } td { border-top: 1px solid var(--hairline); }
  th { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-medium); } .number { text-align: right; }
  .sort { min-height: var(--target-min); padding: 0; color: inherit; background: none; border: 0; font: inherit; font-weight: inherit; cursor: pointer; }
  .sort:hover:enabled { background: none; color: var(--text-primary); }
  th[aria-sort=descending] .sort { color: var(--text-primary); } th[aria-sort=descending] .sort::after { content: ' ↓'; }
  tr[tabindex] { cursor: pointer; transition: background-color var(--motion-fast) var(--motion-ease); } tr[tabindex]:hover { background: var(--surface-hover); }
  tr[aria-selected=true], tr[aria-selected=true]:hover { background: var(--surface-selected); box-shadow: var(--selected-edge); } tr:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .player strong { font-weight: var(--weight-medium); }
  .note { max-width: 160px; overflow: hidden; text-overflow: ellipsis; } small { display: block; } small, .hint { color: var(--text-muted); } .hint { font-size: var(--text-meta); } small { font-size: var(--text-meta); }
  [role=alert] { color: var(--danger); }
  @media (max-width: 1050px) { .with-panel { grid-template-columns: minmax(0, 1fr); } }
</style>

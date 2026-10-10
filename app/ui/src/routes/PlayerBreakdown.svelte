<script lang="ts">
  import BusyStatus from '../lib/status/BusyStatus.svelte';
  import { historyStatus } from '../lib/status/registry';
  import { formatShare, formatStat } from '../lib/styles/format';
  import { debugMode } from '../lib/debug/debug';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { tick, untrack } from 'svelte';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { ListState } from '../lib/navigation/state';
  import type { FightView } from '../lib/bindings/FightView';
  import type { PlayerDamage } from '../lib/bindings/PlayerDamage';
  import { routeHref, type FightRelationship } from '../lib/navigation/routes';
  import { createRunHistory, type HistoryController, type HistoryState } from '../lib/runs/history';
  import HistoryStatus from '../lib/runs/HistoryStatus.svelte';
  import { HISTORY_CATCH_UP } from '../lib/runs/policy';
  import { coverageExplanation, damageRank, reconstructionDefaults } from '../lib/runs/damage';
  import SourceBreakdown from '../lib/runs/SourceBreakdown.svelte';
  import { createHomeVisuals, emptyHomeVisuals } from '../lib/home/visuals';
  import SpriteImage from '../lib/components/SpriteImage.svelte';
  import { spriteUrl } from '../lib/bridge/visuals';
  import type { RunFightStripRow } from '../lib/bindings/RunFightStripRow';
  import { objectVisual } from '../lib/bridge/visual-index';
  import { parseObjectId } from '../lib/navigation/routes';
  import Breadcrumbs from '../lib/runs/Breadcrumbs.svelte';
  import KpiStrip from '../lib/home/v3/KpiStrip.svelte';
  import PlayerSeriesChart from '../lib/runs/PlayerSeriesChart.svelte';
  import { playerKpis, playerNeighbors, playerRankKey, sourceRows } from '../lib/runs/player-screen';
  import '../lib/home/v3/area-home.css';
  import EmptyState from '../lib/components/EmptyState.svelte';
  let { client, fightId, objectId, runId, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady, onRelationship, enabled = true }: {
    client?: Client; fightId: string; objectId: number; runId?: string; streamId?: string | null; scopeKey?: string;
    required?: string | null; store?: StoreStatus | null; entryKey?: string;
    onRowsReady?: (key: string) => void; enabled?: boolean;
    onRelationship?: (relationship: FightRelationship) => void;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
  } = $props();
  type Selection = { kind: 'player'; player: PlayerDamage } | { kind: 'missing' | 'ambiguous' | 'relationship' };
  // Metadata is tied to one accepted fight object, never to an independently refreshed row.
  const selections = new WeakMap<FightView, Selection>();
  const strips = new WeakMap<FightView, RunFightStripRow[] | null>();
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading player context.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let retryShown = $state(false);
  let coherenceWarning = $state('');
  let fight = $derived(history.value?.kind === 'fight' ? history.value.fight : null);
  let selection = $derived(fight === null ? undefined : selections.get(fight));
  let player = $derived(selection?.kind === 'player' ? selection.player : null);
  let visuals = $state.raw(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let visual = $derived(objectVisual(visuals.objects, player?.class_id ?? null));
  let strip = $derived(fight ? strips.get(fight) ?? null : null);
  let name = $derived(player?.name ?? 'Unknown player');
  let bossName = $derived(strip?.find(row => row.fight_id === fight?.id)?.target_name ?? objectVisual(visuals.objects, fight?.target_type ?? fight?.boss_type ?? null)?.name ?? 'Unknown target');
  let neighbors = $derived(fight ? playerNeighbors(fight, objectId) : { previous: null, next: null });
  let cells = $derived.by(() => {
    if (!player || !fight) return [];
    const best = sourceRows(player, visuals).filter(row => row.bestHit !== null).sort((a, b) => b.bestHit! - a.bestHit!)[0];
    return playerKpis(player, fight).map(cell => cell.key === 'best_hit' ? { ...cell, sub: best?.name ?? '—' }
      : cell.key === 'rank' ? { ...cell, value: damageRank(player!.rank), of: String(fight!.players.length) } : cell);
  });
  const fightHref = (id: string) => routeHref({ kind: 'fight', id, runId: fight?.run_id ?? undefined });
  const playerHref = (row: PlayerDamage | null) => row && fight && parseObjectId(String(row.object_id)) !== null
    && fight.players.filter(value => value.object_id === row.object_id).length === 1
    ? routeHref({ kind: 'player', fightId: fight.id, objectId: row.object_id, runId: fight.run_id ?? undefined }) : undefined;
  const stripName = (row: RunFightStripRow) => row.target_name ?? objectVisual(visuals.objects, row.target_type ?? row.boss_type)?.name ?? 'Unknown target';
  let selectionLink: HTMLAnchorElement | undefined = $state();
  function selectPlayer(event: Event) {
    const row = fight?.players.find(value => String(value.object_id) === (event.currentTarget as HTMLSelectElement).value);
    const href = playerHref(row ?? null);
    // Use the shell's existing link interception so Back/Forward keeps its route state.
    if (href && selectionLink) { selectionLink.setAttribute('href', href); selectionLink.click(); }
  }
  $effect(() => {
    if (!client || !enabled) return;
    const instance = createHomeVisuals(client, next => { visuals = next; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = visualController;
    const entries = player ? [player] : selection?.kind === 'ambiguous'
      ? fight?.players.filter(row => row.object_id === objectId) ?? [] : [];
    const types = [fight?.target_type ?? fight?.boss_type ?? null, ...(strip ?? []).map(row => row.target_type ?? row.boss_type), ...entries.flatMap(row => [row.class_id, ...row.sources.map(source => source.item_type)])]
      .filter((type): type is number => type !== null);
    untrack(() => instance?.update(types));
  });
  let available = $derived(!client || history.value !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  $effect(() => {
    const report = { entryKey, scopeKey, streamId, fightId: fight?.id ?? null, runId: fight?.run_id ?? null };
    const receive = onRelationship;
    untrack(() => receive?.(report));
  });
  $effect(() => {
    const owner = client, id = fightId, selectedId = objectId, parentId = runId;
    // Retire all stages when the route namespace changes, including while fight awaits SQL.
    const routeScope = scopeKey, routeStream = streamId;
    if (!owner || !enabled) return;
    let current = true;
    coherenceWarning = '';
    history = { phase: 'loading', value: null, message: 'Loading player context.', retryAvailable: false };
    // No cache: this wrapper also fills component-local player selections and strips.
    const instance = createRunHistory({ ...owner, getFight: async () => {
      const context = await owner.getFight(id);
      if (!current) return { status: 'superseded' };
      if (context.status !== 'ready' || context.value === null) return context;
      const value = context.value;
      if (value.id !== id || (parentId !== undefined && value.run_id !== parentId)) {
        selections.set(value, { kind: 'relationship' });
        return context;
      }
      const matches = value.players.filter(row => row.object_id === selectedId);
      if (matches.length !== 1) {
        selections.set(value, { kind: matches.length > 1 ? 'ambiguous' : 'missing' });
        coherenceWarning = '';
        return context;
      }
      // One logical controller request: await context/control before admitting player detail.
      const reply = await owner.getPlayerBreakdown(id, selectedId);
      if (!current) return { status: 'superseded' };
      if (reply.status !== 'ready') return reply;
      if (reply.value !== null && (reply.value.object_id !== selectedId || reply.value.player_id !== matches[0].player_id)) {
        coherenceWarning = 'Saved player revisions did not match. Keeping the last coherent view while awaiting a saved revision.';
        return { status: 'superseded' };
      }
      let runStrip: RunFightStripRow[] | null = null;
      if (reply.value !== null && value.run_id !== null) {
        try {
          const response = await owner.getRunFightStrip(value.run_id, reply.value.player_id);
          if (!current) return { status: 'superseded' };
          if (response.status === 'ready') runStrip = response.value;
        } catch { /* Optional context must not fail a coherent player view. */ }
        if (!current) return { status: 'superseded' };
      }
      strips.set(value, runStrip);
      selections.set(value, reply.value === null ? { kind: 'missing' } : { kind: 'player', player: reply.value });
      coherenceWarning = '';
      return context;
    } }, next => {
      history = next;
      if (next.phase === 'error' || (next.phase === 'busy' && next.retryAvailable)) retryShown = true;
    });
    controller = instance;
    untrack(() => instance.update({ query: { kind: 'fight', id }, scopeKey: routeScope, streamId: routeStream, required, store }));
    return () => { current = false; instance.dispose(); };
  });
  $effect(() => {
    const instance = controller, input = { query: { kind: 'fight' as const, id: fightId }, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready) return;
    let current = true;
    void tick().then(() => { if (current) onRowsReady?.(key); });
    return () => { current = false; };
  });
</script>

<BusyStatus message={client ? historyStatus(history, 'player') : null} />

<div class="player">
  <HistoryStatus {history} />
  {#if coherenceWarning}<DebugNote text={coherenceWarning} />
  {:else if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<DebugNote text={history.message} />{/if}
  {#if retryShown}<button class="ctl" type="button" aria-disabled={!history.retryAvailable} onclick={() => { if (history.retryAvailable) controller?.retry(); }}>Retry</button>{/if}
  {#if !client}
    <EmptyState state="unavailable" heading="No Players" description="" />
  {:else if selection?.kind === 'relationship'}
    <EmptyState state="unavailable" heading="Player Unavailable" description="" />
  {:else if fight && selection}
    <div class="area-home player-screen" class:ambiguous={selection.kind === 'ambiguous'}>
    <div class="crumbs"><Breadcrumbs items={[{ label: 'Runs', href: '#/runs' },
      ...(fight.run_id !== null ? [{ label: 'Run', href: routeHref({ kind: 'run', id: fight.run_id }) }] : []),
      { label: bossName, href: fightHref(fight.id) }, { label: player ? name : 'Player' }]} /></div>
    {#if selection.kind === 'ambiguous'}
      <DebugNote text="Player identity is ambiguous: a reused object ID refers to multiple saved player entries. Each entry's sources are shown separately." />
      <!-- Reused object IDs keep their saved entries separate. -->
      {#each fight.players.filter(row => row.object_id === objectId) as entry (entry.player_id)}
        <SourceBreakdown player={entry} {visuals} rowPrefix={`source:${entry.player_id}`} label={`${entry.name ?? 'Unknown Player'} Sources`} />
      {/each}
    {:else if player}
      <header class="area-header">
        <span class="run-icon"><SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={28} alt={visual?.name ?? 'Class'} /></span>
        <div class="title"><h2><span title={name}>{name}</span>{#if player.is_local}<span class="tag you">You</span>{/if}</h2>
          <p title={`${visual?.name ?? '—'} · rank ${damageRank(player.rank)} of ${fight.players.length} against ${bossName}`}>{visual?.name ?? '—'} · rank {damageRank(player.rank)} of {fight.players.length} against {bossName}</p></div>
        {#if !fight.complete_capture || fight.issues.length}<span class="tag">partial</span>{/if}
        <nav class="neighbors" aria-label="Players in this fight">
          <select class="ctl" aria-label="Player" value={String(player.object_id)} onchange={selectPlayer}>
            {#each [...fight.players].sort((a, b) => playerRankKey(a) - playerRankKey(b)) as row (row.player_id)}
              <option value={String(row.object_id)} disabled={!playerHref(row)}>{row.name ?? 'Unknown player'}</option>
            {/each}
          </select>
          <a bind:this={selectionLink} hidden aria-hidden="true" tabindex="-1" href={playerHref(player)}>Open selected player</a>
          <a class="ctl sq" aria-label={neighbors.previous ? `‹ Previous player: ${neighbors.previous.name ?? 'Unknown player'}` : '‹ Previous player'} aria-disabled={!playerHref(neighbors.previous)} href={playerHref(neighbors.previous)}>‹</a>
          <a class="ctl sq" aria-label={neighbors.next ? `› Next player: ${neighbors.next.name ?? 'Unknown player'}` : '› Next player'} aria-disabled={!playerHref(neighbors.next)} href={playerHref(neighbors.next)}>›</a>
        </nav>
      </header>
      <div class="area-kpis"><KpiStrip {cells} /></div>
      <div class="sources"><SourceBreakdown {player} {visuals} rowPrefix={`source:${player.player_id}`} /></div>
      <aside class="side">
        <PlayerSeriesChart {fight} {player} />
        <section class="area-card" aria-label={`${name} this run`}><h3 title={`${name} this run`}>{name} this run</h3>
          {#if strip?.length}<div class="strip-scroll"><table>
            <colgroup><col /><col style="width: 72px" /><col style="width: 38px" /><col style="width: 46px" /></colgroup>
            <thead><tr><th scope="col">Boss</th><th scope="col">Damage</th><th scope="col">Rank</th><th scope="col">Share</th></tr></thead>
            <tbody>{#each strip as row (row.fight_id)}
              {@const boss = objectVisual(visuals.objects, row.target_type ?? row.boss_type)}
              <tr class:current={row.fight_id === fight.id}><th scope="row"><a href={fightHref(row.fight_id)} aria-current={row.fight_id === fight.id ? 'page' : undefined} title={stripName(row)}><span aria-hidden="true"><SpriteImage sprite={boss?.sprite ?? null} sheetUrl={boss?.sprite ? spriteUrl(visuals.client_fingerprint, boss.sprite) : null} fit={24} alt="" /></span><span class="boss-name">{stripName(row)}</span></a></th><td>{formatStat(row.damage)}</td><td class="rank">{damageRank(row.rank)}</td><td>{formatShare(row.share)}</td></tr>
            {/each}</tbody>
          </table></div>{:else}<p class="muted">—</p>{/if}
        </section>
      </aside>
    {:else}
      <EmptyState heading="Player Not Found" description="" />
    {/if}
    <!-- Lifetime reconstruction counters cannot describe this fight's missing damage. -->
    {#if selection.kind !== 'missing' && $debugMode}
      <details class="area-debug" aria-label="Damage Evidence">
        <summary>Damage Evidence</summary>
        <p>{fight.complete_capture && fight.issues.length === 0 ? 'Complete observed capture' : 'Partial capture; observations are incomplete'}.</p>
        {#if fight.issues.length}<ul>{#each fight.issues as issue}<li>{coverageExplanation[issue]}</li>{/each}</ul>{/if}
        <p>Detailed reconstruction counts were not saved for this run.</p>
        <p>Lifetime diagnostic counters can remain after server reconciliation and do not describe this run's hits or missing damage. Known observed zero remains zero; missing attribution is unavailable.</p>
        <details><summary>Reconstruction Defaults</summary>
          <p>These are documented defaults, not evidence that every assumption occurred in this fight. Saved source reasons above describe the retained evidence.</p>
          <ul>{#each reconstructionDefaults as explanation}<li>{explanation}</li>{/each}</ul>
        </details>
      </details>
    {/if}
    </div>
  {:else if history.value !== null}
    <EmptyState heading="Fight Not Found" description="">
      {#snippet action()}<a href="#/runs">Runs</a>{/snippet}
    </EmptyState>
  {:else if history.phase === 'error'}
    <EmptyState state="error" heading="Player Error" description="" />
  {:else}
    <EmptyState state="loading" heading="Loading Player" description="" />
  {/if}
</div>

<style>
  .player { overflow-anchor: none; overflow-wrap: anywhere; min-width: 0; }
  .player-screen { height: auto; grid-template-columns: minmax(0, 1fr) 340px; grid-template-rows: auto auto auto auto; grid-template-areas: 'crumbs crumbs' 'head head' 'kpis kpis' 'src side'; align-content: start; align-items: start; }
  .player-screen.ambiguous { display: flex; flex-direction: column; align-items: stretch; }
  .crumbs { grid-area: crumbs; min-width: 0; }
  .area-header { flex-wrap: wrap; }
  .title { flex: 1; min-width: 0; }
  .title h2 { display: flex; gap: 8px; align-items: center; }
  .title h2 > span:first-child, .title p { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tag { flex: none; font-size: 10px; line-height: 16px; color: var(--text-muted); border: 1px solid var(--border); border-radius: 4px; padding: 0 5px; }
  .you { color: var(--gold); border-color: #f2c14e55; }
  .neighbors { display: flex; align-items: center; gap: 6px; margin-left: auto; min-width: 0; }
  .neighbors select { width: 150px; min-width: 0; text-overflow: ellipsis; }
  .neighbors a { box-sizing: border-box; min-height: 26px; }
  .sources { grid-area: src; min-width: 0; }
  .side { grid-area: side; display: grid; align-self: start; gap: 10px; min-width: 0; padding: 0; border: 0; background: transparent; }
  .side h3 { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .strip-scroll { max-height: 260px; overflow: auto; }
  table { width: 100%; table-layout: fixed; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { text-align: right; height: 30px; padding: 0 4px; font-weight: 400; }
  th:first-child { text-align: left; }
  thead th { font-size: 11px; color: var(--text-muted); }
  tbody tr { border-top: 1px solid var(--border); }
  td { white-space: nowrap; }
  table a { display: flex; align-items: center; gap: 6px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: inherit; text-decoration: none; line-height: 30px; }
  .boss-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  table a > span:first-child { display: flex; flex: none; }
  .current { background: #f2c14e14; } .current th { box-shadow: inset 2px 0 var(--gold); padding-left: 6px; } .current .rank { color: var(--gold); }
  .muted { color: var(--text-muted); }
  summary { cursor: pointer; min-height: 26px; align-content: center; }
  .area-kpis :global(.value) { overflow: hidden; text-overflow: ellipsis; }
  @media (max-width: 950px) { .player-screen { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto; grid-template-areas: 'crumbs' 'head' 'kpis' 'src' 'side'; } }
</style>

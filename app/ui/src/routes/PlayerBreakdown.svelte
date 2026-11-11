<script lang="ts">
  import BusyStatus from '../lib/status/BusyStatus.svelte';
  import { historyStatus } from '../lib/status/registry';
  import { debugMode } from '../lib/debug/debug';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { tick, untrack } from 'svelte';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { ListState } from '../lib/navigation/state';
  import type { FightView } from '../lib/bindings/FightView';
  import type { PlayerDamage } from '../lib/bindings/PlayerDamage';
  import type { PlayerContextView } from '../lib/bindings/PlayerContextView';
  import type { RunPlayerFight } from '../lib/bindings/RunPlayerFight';
  import { routeHref, type FightRelationship } from '../lib/navigation/routes';
  import { createRunHistory, type HistoryController, type HistoryState } from '../lib/runs/history';
  import HistoryStatus from '../lib/runs/HistoryStatus.svelte';
  import { HISTORY_CATCH_UP } from '../lib/runs/policy';
  import { coverageExplanation, reconstructionDefaults } from '../lib/runs/damage';
  import SourceBreakdown from '../lib/runs/SourceBreakdown.svelte';
  import { createHomeVisuals, emptyHomeVisuals } from '../lib/home/visuals';
  import { spriteUrl } from '../lib/bridge/visuals';
  import type { RunFightStripRow } from '../lib/bindings/RunFightStripRow';
  import { objectVisual } from '../lib/bridge/visual-index';
  import { parseObjectId } from '../lib/navigation/routes';
  import Breadcrumbs from '../lib/runs/Breadcrumbs.svelte';
  import KpiStrip from '../lib/home/v3/KpiStrip.svelte';
  import PlayerHeader from '../lib/players/PlayerHeader.svelte';
  import ScopeTabs from '../lib/players/ScopeTabs.svelte';
  import PlayerTimelineChart from '../lib/players/PlayerTimelineChart.svelte';
  import BossFightsTable from '../lib/players/BossFightsTable.svelte';
  import GearCard from '../lib/players/GearCard.svelte';
  import StatsCard from '../lib/players/StatsCard.svelte';
  import ActivityCard from '../lib/players/ActivityCard.svelte';
  import { activityItems, durationText, fightPlayerKpis, gearAt, idleGaps, presenceEnd, sliceTimeline, swapsIn,
    timelineFromFightSeries, timestampMs } from '../lib/players/player-view';
  import { playerNeighbors, playerRankKey, sourceRows } from '../lib/runs/player-screen';
  import '../lib/home/v3/area-home.css';
  import '../lib/players/players.css';
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
  const contexts = new WeakMap<FightView, PlayerContextView | null>();
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading player context.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let retryShown = $state(false);
  let coherenceWarning = $state('');
  let fight = $derived(history.value?.kind === 'fight' ? history.value.fight : null);
  let selection = $derived(fight === null ? undefined : selections.get(fight));
  let player = $derived(selection?.kind === 'player' ? selection.player : null);
  let visuals = $state.raw(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let strip = $derived(fight ? strips.get(fight) ?? null : null);
  let context = $derived(fight ? contexts.get(fight) ?? null : null);
  let profile = $derived(context?.profile ?? null);
  let name = $derived(profile?.name ?? player?.name ?? 'Unknown player');
  let classId = $derived(profile?.class_id ?? player?.class_id ?? null);
  let firstSeen = $derived(timestampMs(profile?.first_seen));
  let from = $derived(timestampMs(fight?.started));
  let until = $derived(timestampMs(fight?.ended ?? fight?.last_hit));
  let fightTimeline = $derived(fight && player ? timelineFromFightSeries(fight, player.player_id)
    ?? timelineFromFightSeries(fight, player.player_id, true) : null);
  let slicedTimeline = $derived(fight && player && !fightTimeline && context?.timeline && !context.timeline.disabled
    && from !== null && until !== null ? sliceTimeline(context.timeline, from, until) : null);
  let timeline = $derived(fight && player ? fightTimeline ?? slicedTimeline ?? context?.timeline ?? null : null);
  let lastSeen = $derived(presenceEnd(profile?.last_seen, timeline, fight?.ended == null));
  let inRunMs = $derived(firstSeen !== null && lastSeen !== null && lastSeen >= firstSeen ? lastSeen - firstSeen : null);
  let present = $derived(firstSeen !== null && lastSeen !== null && from !== null && until !== null
    ? { from: Math.max(firstSeen, from), until: Math.min(lastSeen, until) } : null);
  let equipment = $derived(from !== null ? gearAt(context?.gear_log ?? [], from) : null);
  let baseline = $derived(equipment !== null && from !== null ? { at: from, equipment } : null);
  let swaps = $derived(from !== null && until !== null ? swapsIn(context?.gear_log ?? [], from, until) : []);
  let bossName = $derived(strip?.find(row => row.fight_id === fight?.id)?.target_name ?? objectVisual(visuals.objects, fight?.target_type ?? fight?.boss_type ?? null)?.name ?? 'Unknown target');
  let neighbors = $derived(fight ? playerNeighbors(fight, objectId) : { previous: null, next: null });
  let cells = $derived.by(() => {
    if (!player || !fight) return [];
    const best = sourceRows(player, visuals).filter(row => row.bestHit !== null).sort((a, b) => b.bestHit! - a.bestHit!)[0];
    return fightPlayerKpis(fight, player, context?.deaths ?? [], timeline,
      present ? Math.max(0, present.until - present.from) : null, profile)
      .map(cell => cell.key === 'best_hit' ? { ...cell, sub: best?.name ?? '—' } : cell);
  });
  const fightHref = (id: string) => routeHref({ kind: 'fight', id, runId: fight?.run_id ?? undefined });
  const playerHref = (row: PlayerDamage | null) => row && fight && parseObjectId(String(row.object_id)) !== null
    && fight.players.filter(value => value.object_id === row.object_id).length === 1
    ? routeHref({ kind: 'player', fightId: fight.id, objectId: row.object_id, runId: fight.run_id ?? undefined }) : undefined;
  const stripName = (row: RunFightStripRow) => row.target_name ?? objectVisual(visuals.objects, row.target_type ?? row.boss_type)?.name ?? 'Unknown target';
  let picker = $derived([...(fight?.players ?? [])].sort((a, b) => playerRankKey(a) - playerRankKey(b)
    || (a.name ?? '').localeCompare(b.name ?? ''))
    .map(row => ({ href: playerHref(row) ?? null,
      label: `${row.name ?? 'Unknown player'}${row.is_local ? ' (You)' : ''}`, current: row.player_id === player?.player_id })));
  // The strip selects contributions by this scope's player ID. Object identity is
  // stable for that player; absent contributions cannot produce a player link.
  let fights = $derived((strip ?? []).map((row): RunPlayerFight => ({ ...row, target_name: stripName(row),
    object_id: row.damage !== null ? player?.object_id ?? null : null,
    dps_basis: row.fight_id === fight?.id ? fight.dps_basis : 'whole_target_v0',
    dps: row.fight_id === fight?.id ? player?.dps ?? null : null,
    deaths: row.fight_id === fight?.id ? (context?.deaths.length || player?.deaths) ?? null : null,
  })).sort((a, b) => (timestampMs(a.started) ?? 0) - (timestampMs(b.started) ?? 0)));
  const fightPlayerHref = (row: RunPlayerFight) => row.object_id == null ? null
    : routeHref({ kind: 'player', runId: fight?.run_id ?? undefined, fightId: row.fight_id, objectId: row.object_id });
  let tabs = $derived(fight?.run_id && player ? [{ label: 'Whole run', sub: '', current: false,
    href: routeHref({ kind: 'run-player', runId: fight.run_id, playerId: player.player_id }) }, ...fights.map(row => ({
      label: row.target_name ?? 'Unknown target', sub: durationText(timestampMs(row.started) !== null && timestampMs(row.ended) !== null
        ? Math.max(0, timestampMs(row.ended)! - timestampMs(row.started)!) : null),
      href: fightPlayerHref(row), current: row.fight_id === fight?.id,
    }))] : []);
  let activity = $derived.by(() => {
    const hits = timeline && !timeline.disabled ? timeline.buckets.filter((_, i) => timeline!.points[i] > 0) : [];
    return activityItems({ firstSeen: firstSeen !== null && from !== null && until !== null && firstSeen >= from && firstSeen < until ? firstSeen : null,
      firstHit: timestampMs(hits[0]?.from), lastHit: timestampMs(hits.at(-1)?.until), deaths: context?.deaths ?? [], departedAt: null,
      gaps: timeline && present ? idleGaps(timeline, present) : [] });
  });
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
    const types = [classId, profile?.skin_id, ...(fight?.players ?? []).map(row => row.class_id),
      ...(profile?.equipment ?? []).map(row => row.item_type), ...(context?.gear_log ?? []).flatMap(row => row.equipment.map(item => item.item_type)),
      fight?.target_type ?? fight?.boss_type ?? null, ...(strip ?? []).map(row => row.target_type ?? row.boss_type), ...entries.flatMap(row => [row.class_id, ...row.sources.map(source => source.item_type)])]
      .filter((type): type is number => type != null);
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
      let playerContext: PlayerContextView | null = null;
      if (reply.value !== null && value.scope_id != null) {
        try {
          const response = await owner.getPlayerContext(value.scope_id, reply.value.player_id, value.started, value.ended ?? value.last_hit);
          if (!current) return { status: 'superseded' };
          if (response.status === 'ready') playerContext = response.value;
        } catch { /* Optional context must not fail a coherent player view. */ }
        if (!current) return { status: 'superseded' };
        if (playerContext?.profile && (playerContext.profile.scope_id !== value.scope_id
          || playerContext.profile.player_id !== reply.value.player_id || playerContext.profile.object_id !== selectedId)) {
          coherenceWarning = 'Saved player revisions did not match. Keeping the last coherent view while awaiting a saved revision.';
          return { status: 'superseded' };
        }
      }
      contexts.set(value, playerContext);
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
    <EmptyState state="unavailable" heading="No players" description="" />
  {:else if selection?.kind === 'relationship'}
    <EmptyState state="unavailable" heading="Player unavailable" description="" />
  {:else if fight && selection}
    <div class="area-home player-screen" class:ambiguous={selection.kind === 'ambiguous'}>
    <div class="crumbs"><Breadcrumbs items={[{ label: 'Runs', href: '#/runs' },
      ...(fight.run_id !== null ? [{ label: 'Run', href: routeHref({ kind: 'run', id: fight.run_id }) }] : []),
      { label: bossName, href: fightHref(fight.id) }, { label: player ? name : 'Player' }]} /></div>
    {#if selection.kind === 'ambiguous'}
      <DebugNote text="Player identity is ambiguous: a reused object ID refers to multiple saved player entries. Each entry's sources are shown separately." />
      <!-- Reused object IDs keep their saved entries separate. -->
      {#each fight.players.filter(row => row.object_id === objectId) as entry (entry.player_id)}
        <SourceBreakdown player={entry} {visuals} rowPrefix={`source:${entry.player_id}`} label={`${entry.name ?? 'Unknown player'} sources`} />
      {/each}
    {:else if player}
      <div class="head"><PlayerHeader {name} {classId} isLocal={profile?.is_local ?? player.is_local}
        live={fight.ended == null} partial={!fight.complete_capture || fight.issues.length > 0} {profile} {inRunMs} presenceUntil={lastSeen} {visuals} {picker}
        prevHref={playerHref(neighbors.previous) ?? null} nextHref={playerHref(neighbors.next) ?? null}
        prevName={neighbors.previous?.name ?? null} nextName={neighbors.next?.name ?? null} /></div>
      {#if tabs.length}<div class="scope"><ScopeTabs {tabs} /></div>{/if}
      <div class="kpis"><KpiStrip {cells} /></div>
      <div class="main-column">
        <PlayerTimelineChart {timeline} bands={[]} local={profile?.is_local ?? player.is_local} note={slicedTimeline ? 'Includes damage to other targets during this fight' : null} missingText={fight.scope_id != null && context === null ? 'Timeline unavailable' : 'Not recorded for this run'} unavailable={timeline === null ? 'legacy' : timeline.disabled ? 'limit' : undefined} />
        <SourceBreakdown {player} label="Damage by item" {visuals} rowPrefix={`source:${player.player_id}`} showEstimates />
        <BossFightsTable rows={fights} currentFightId={fight.id} hrefFor={fightPlayerHref} />
      </div>
      <aside class="side-column">
        <GearCard missingText={context !== null ? 'Not seen at fight start' : fight.scope_id != null ? 'Gear unavailable' : 'Not recorded for this run'} {equipment} observedAt={baseline?.at ?? null} {baseline} {swaps} {visuals} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} />
        <StatsCard missingText={fight.scope_id != null && context === null ? 'Stats unavailable' : 'Not recorded for this run'} {profile} {visuals} />
        <ActivityCard items={activity} />
      </aside>
    {:else}
      <EmptyState heading="Player not found" description="" />
    {/if}
    <!-- Lifetime reconstruction counters cannot describe this fight's missing damage. -->
    {#if selection.kind !== 'missing' && $debugMode}
      <details class="area-debug" aria-label="Damage evidence">
        <summary>Damage evidence</summary>
        <p>{fight.complete_capture && fight.issues.length === 0 ? 'Complete observed capture' : 'Partial capture; observations are incomplete'}.</p>
        {#if fight.issues.length}<ul>{#each fight.issues as issue}<li>{coverageExplanation[issue]}</li>{/each}</ul>{/if}
        <p>Detailed reconstruction counts were not saved for this run.</p>
        <p>Lifetime diagnostic counters can remain after server reconciliation and do not describe this run's hits or missing damage. Known observed zero remains zero; missing attribution is unavailable.</p>
        <details><summary>Reconstruction defaults</summary>
          <p>These are documented defaults, not evidence that every assumption occurred in this fight. Saved source reasons above describe the retained evidence.</p>
          <ul>{#each reconstructionDefaults as explanation}<li>{explanation}</li>{/each}</ul>
        </details>
      </details>
    {/if}
    </div>
  {:else if history.value !== null}
    <EmptyState heading="Fight not found" description="">
      {#snippet action()}<a href="#/runs">Runs</a>{/snippet}
    </EmptyState>
  {:else if history.phase === 'error'}
    <EmptyState state="error" heading="Player error" description="" />
  {:else}
    <EmptyState state="loading" heading="Loading player" description="" />
  {/if}
</div>

<style>
  summary { cursor: pointer; min-height: var(--target-min); align-content: center; }
  .player { overflow-anchor: none; overflow-wrap: anywhere; min-width: 0; }
  .player-screen { height: auto; grid-template-columns: minmax(0, 1fr) 300px; grid-template-rows: auto auto auto auto auto auto; grid-template-areas: 'crumbs crumbs' 'head head' 'scope scope' 'kpis kpis' 'main side' 'debug debug'; align-content: start; align-items: start; }
  .player-screen.ambiguous { display: flex; flex-direction: column; align-items: stretch; }
  .crumbs { grid-area: crumbs; min-width: 0; }
  .head { grid-area: head; min-width: 0; }
  .scope { grid-area: scope; min-width: 0; }
  .kpis { grid-area: kpis; min-width: 0; overflow-x: auto; }
  .kpis :global(.kpi-strip) { min-width: 800px; }
  .main-column { grid-area: main; display: grid; gap: var(--space-3); min-width: 0; }
  .side-column { grid-area: side; display: grid; gap: var(--space-3); min-width: 0; }
  @media (max-width: 900px) {
    .player-screen { grid-template-columns: minmax(0, 1fr); grid-template-areas: 'crumbs' 'head' 'scope' 'kpis' 'main' 'side' 'debug'; }
  }
</style>

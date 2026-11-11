<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { ListState } from '../lib/navigation/state';
  import type { PlayerDamage } from '../lib/bindings/PlayerDamage';
  import type { RunPlayerFight } from '../lib/bindings/RunPlayerFight';
  import { routeHref, type FightRelationship } from '../lib/navigation/routes';
  import Breadcrumbs from '../lib/runs/Breadcrumbs.svelte';
  import BusyStatus from '../lib/status/BusyStatus.svelte';
  import { historyStatus } from '../lib/status/registry';
  import { createRunHistory, type HistoryController, type HistoryState } from '../lib/runs/history';
  import HistoryStatus from '../lib/runs/HistoryStatus.svelte';
  import { HISTORY_CATCH_UP } from '../lib/runs/policy';
  import EmptyState from '../lib/components/EmptyState.svelte';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { debugMode } from '../lib/debug/debug';
  import { reasonNotes } from '../lib/home/evidence';
  import { nonnegative, reconstructionDefaults, sourceLabels } from '../lib/runs/damage';
  import SourceBreakdown from '../lib/runs/SourceBreakdown.svelte';
  import { sourceRows } from '../lib/runs/player-screen';
  import { createHomeVisuals, emptyHomeVisuals } from '../lib/home/visuals';
  import { objectVisual } from '../lib/bridge/visual-index';
  import { spriteUrl } from '../lib/bridge/visuals';
  import KpiStrip from '../lib/home/v3/KpiStrip.svelte';
  import PlayerHeader from '../lib/players/PlayerHeader.svelte';
  import ScopeTabs from '../lib/players/ScopeTabs.svelte';
  import PlayerTimelineChart from '../lib/players/PlayerTimelineChart.svelte';
  import BossFightsTable from '../lib/players/BossFightsTable.svelte';
  import GearCard from '../lib/players/GearCard.svelte';
  import StatsCard from '../lib/players/StatsCard.svelte';
  import ActivityCard from '../lib/players/ActivityCard.svelte';
  import { activityItems, durationText, idleGaps, presenceEnd, runPlayerKpis, timestampMs } from '../lib/players/player-view';
  import '../lib/home/v3/area-home.css';
  import '../lib/players/players.css';

  let { client, runId, playerId, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady, onRelationship, enabled = true }: {
    runId: string; playerId: string; client?: Client; enabled?: boolean;
    streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void;
    onRelationship?: (relationship: FightRelationship) => void;
  } = $props();

  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading player context.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let retryShown = $state(false);
  let visuals = $state.raw(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let view = $derived(history.value?.kind === 'run-player' ? history.value.player : null);
  let coherent = $derived(view !== null && view.run.id === runId && view.player_id === playerId);
  let profile = $derived(view?.profile ?? null);
  let roster = $derived(view?.roster ?? []);
  let selected = $derived(roster.find(row => row.player_id === playerId));
  let name = $derived(profile?.name ?? view?.player?.name ?? selected?.name ?? null);
  let classId = $derived(profile?.class_id ?? view?.player?.class_id ?? selected?.class_id ?? null);
  let isLocal = $derived(profile?.is_local ?? view?.player?.is_local ?? selected?.is_local ?? false);
  let firstSeen = $derived(timestampMs(profile?.first_seen));
  let lastSeen = $derived(presenceEnd(profile?.last_seen, view?.timeline, view?.run.ended == null));
  let inRunMs = $derived(firstSeen !== null && lastSeen !== null && lastSeen >= firstSeen ? lastSeen - firstSeen : null);
  let timeline = $derived(view?.timeline ?? null);
  let lastObserved = $derived(timestampMs(view?.run.ended) ?? lastSeen ?? timestampMs(timeline?.buckets.at(-1)?.until));
  const elapsed = (from: string, until: string | null | undefined) => {
    const start = timestampMs(from), end = timestampMs(until) ?? lastObserved;
    return durationText(start !== null && end !== null && end >= start ? end - start : null);
  };
  const runPlayerHref = (id: string) => routeHref({ kind: 'run-player', runId, playerId: id });
  const fightPlayerHref = (row: RunPlayerFight) => row.object_id == null ? null
    : routeHref({ kind: 'player', runId, fightId: row.fight_id, objectId: row.object_id });
  let rosterIndex = $derived(roster.findIndex(row => row.player_id === playerId));
  let picker = $derived(roster.map(row => ({ href: runPlayerHref(row.player_id),
    label: `${row.name ?? 'Not seen'}${row.is_local ? ' (You)' : ''}`, current: row.player_id === playerId })));
  let fights = $derived((view?.fights ?? []).map(row => ({ ...row,
    target_name: row.target_name ?? objectVisual(visuals.objects, row.target_type ?? row.boss_type)?.name ?? null,
  })).sort((a, b) => (timestampMs(a.started) ?? 0) - (timestampMs(b.started) ?? 0)));
  let tabs = $derived(view ? [{ label: 'Whole run', sub: elapsed(view.run.started, view.run.ended),
    href: runPlayerHref(playerId), current: true }, ...fights.map(row => ({
      label: row.target_name ?? 'Unknown target', sub: elapsed(row.started, row.ended), href: fightPlayerHref(row), current: false,
    }))] : []);
  let bands = $derived(fights.flatMap(row => {
    const from = timestampMs(row.started), until = timestampMs(row.ended) ?? lastObserved;
    return from !== null && until !== null && until > from ? [{ from, until, label: row.target_name ?? 'Unknown target' }] : [];
  }));
  // SourceBreakdown uses this shape only for identity, totals and source rows.
  // Legacy totals have no object ID; the fallback is never used as a navigation identity.
  let damagePlayer = $derived.by((): PlayerDamage | null => {
    const p = view?.player;
    if (!p) return null;
    const best = view!.sources.map(row => nonnegative(row.biggest_hit)).filter((value): value is number => value !== null);
    return { ...p, object_id: profile?.object_id ?? 0, name: p.name ?? name, class_id: p.class_id ?? classId,
      dps: p.dps ?? null, share: p.share ?? null, rank: p.rank ?? null,
      biggest_hit: best.length ? Math.max(...best) : null, sources: view!.sources };
  });
  let cells = $derived.by(() => {
    if (!view) return [];
    const best = damagePlayer ? sourceRows(damagePlayer, visuals).filter(row => row.bestHit !== null).sort((a, b) => b.bestHit! - a.bestHit!)[0] : null;
    return runPlayerKpis(view).map(cell => cell.key === 'best_hit' ? { ...cell, sub: best?.name ?? '—' } : cell);
  });
  let activity = $derived.by(() => {
    const hits = timeline && !timeline.disabled ? timeline.buckets.filter((_, i) => timeline!.points[i] > 0) : [];
    return activityItems({ firstSeen, firstHit: timestampMs(hits[0]?.from), lastHit: timestampMs(hits.at(-1)?.until),
      deaths: view?.deaths ?? [], departedAt: timestampMs(view?.departed_at),
      gaps: timeline && firstSeen !== null && lastSeen !== null ? idleGaps(timeline, { from: firstSeen, until: lastSeen }) : [] });
  });
  $effect(() => {
    if (!client || !enabled) return;
    const instance = createHomeVisuals(client, next => { visuals = next; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = visualController;
    const types = [classId, profile?.skin_id, ...roster.map(row => row.class_id),
      ...(profile?.equipment ?? []).map(row => row.item_type), ...(view?.gear_log ?? []).flatMap(row => row.equipment.map(item => item.item_type)),
      ...(view?.sources ?? []).map(row => row.item_type), ...(view?.fights ?? []).map(row => row.target_type ?? row.boss_type)]
      .filter((type): type is number => type != null);
    untrack(() => instance?.update(types));
  });
  $effect(() => {
    const report = { entryKey, scopeKey, streamId, fightId: null, runId: coherent ? view!.run.id : null };
    const receive = onRelationship;
    untrack(() => receive?.(report));
  });
  $effect(() => {
    const owner = client, id = runId, selectedId = playerId, routeScope = scopeKey, routeStream = streamId;
    if (!owner || !enabled) return;
    history = { phase: 'loading', value: null, message: 'Loading player context.', retryAvailable: false };
    const instance = createRunHistory(owner, next => {
      history = next;
      if (next.phase === 'error' || (next.phase === 'busy' && next.retryAvailable)) retryShown = true;
    });
    controller = instance;
    untrack(() => instance.update({ query: { kind: 'run-player', runId: id, playerId: selectedId }, scopeKey: routeScope, streamId: routeStream, required, store }));
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller, input = { query: { kind: 'run-player' as const, runId, playerId }, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  let available = $derived(!client || history.value !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready) return;
    let current = true;
    void tick().then(() => { if (current) onRowsReady?.(key); });
    return () => { current = false; };
  });
</script>

<BusyStatus message={client ? historyStatus(history, 'player') : null} />
<div class="run-player">
  <HistoryStatus {history} />
  {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<DebugNote text={history.message} />{/if}
  {#if retryShown}<button class="ctl" type="button" aria-disabled={!history.retryAvailable} onclick={() => { if (history.retryAvailable) controller?.retry(); }}>Retry</button>{/if}
  {#if !client}<EmptyState state="unavailable" heading="No players" description="" />
  {:else if view && coherent}
    <div class="area-home run-player-screen">
      <div class="crumbs"><Breadcrumbs items={[{ label: 'Runs', href: '#/runs' },
        { label: 'Run', href: routeHref({ kind: 'run', id: runId }) }, { label: name ?? 'Player' }]} /></div>
      <div class="head"><PlayerHeader {name} {classId} {isLocal} live={view.run.ended == null} partial={view.partial}
        {profile} {inRunMs} presenceUntil={lastSeen} {visuals} {picker} prevHref={rosterIndex > 0 ? picker[rosterIndex - 1].href : null}
        prevName={rosterIndex > 0 ? roster[rosterIndex - 1].name : null}
        nextName={rosterIndex >= 0 && rosterIndex < roster.length - 1 ? roster[rosterIndex + 1].name : null}
        nextHref={rosterIndex >= 0 && rosterIndex < roster.length - 1 ? picker[rosterIndex + 1].href : null} /></div>
      <div class="scope"><ScopeTabs {tabs} /></div>
      <div class="kpis"><KpiStrip {cells} /></div>
      <div class="main-column">
        <PlayerTimelineChart {timeline} {bands} local={isLocal} unavailable={timeline === null ? 'legacy' : timeline.disabled ? 'limit' : undefined} />
        {#if damagePlayer}<SourceBreakdown player={damagePlayer} label="Damage by item" {visuals} rowPrefix={`source:${playerId}`} showEstimates />
        {:else}<section class="player-card" aria-label="Damage by item"><h3>Damage by item</h3><p class="player-muted">No damage observed</p></section>{/if}
        <BossFightsTable rows={fights} hrefFor={fightPlayerHref} />
      </div>
      <aside class="side-column">
        <GearCard equipment={profile?.equipment ?? null} observedAt={lastSeen} swaps={view.gear_log} {visuals} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} />
        <StatsCard {profile} {visuals} />
        <ActivityCard items={activity} />
      </aside>
      {#if $debugMode}
        <details class="area-debug" aria-label="Damage evidence">
          <summary>Damage evidence</summary>
          <p>{view.partial ? 'Partial capture; observations are incomplete' : 'Complete observed capture'}.</p>
          <p>Detailed reconstruction counts were not saved for this run.</p>
          <p>Lifetime diagnostic counters do not describe this run's hits or missing damage. Known observed zero remains zero; missing attribution is unavailable.</p>
          {#each view.sources as source}
            {#if source.estimated || source.reasons.length}
              <p>{objectVisual(visuals.objects, source.item_type)?.name ?? (source.item_type == null ? sourceLabels[source.kind] : `Item ${source.item_type}`)}:
                {source.reasons.length ? reasonNotes(source.reasons).join('; ') : 'Estimated damage; reconstruction reasons unavailable.'}</p>
            {/if}
          {/each}
          <details><summary>Reconstruction defaults</summary>
            <p>These are documented defaults. Saved source reasons describe the retained evidence.</p>
            <ul>{#each reconstructionDefaults as explanation}<li>{explanation}</li>{/each}</ul>
          </details>
        </details>
      {/if}
    </div>
  {:else if view}<EmptyState state="unavailable" heading="Player unavailable" description="" />
  {:else if history.value !== null}<EmptyState heading="Player not found" description="">
    {#snippet action()}<a href="#/runs">Runs</a>{/snippet}
  </EmptyState>
  {:else if history.phase === 'error'}<EmptyState state="error" heading="Player error" description="" />
  {:else}<EmptyState state="loading" heading="Loading player" description="" />{/if}
</div>

<style>
  .run-player { overflow-anchor: none; overflow-wrap: anywhere; min-width: 0; }
  .run-player-screen { height: auto; grid-template-columns: minmax(0, 1fr) 300px; grid-template-rows: auto auto auto auto auto auto; grid-template-areas: 'crumbs crumbs' 'head head' 'scope scope' 'kpis kpis' 'main side' 'debug debug'; align-content: start; align-items: start; }
  .crumbs { grid-area: crumbs; min-width: 0; }
  .head { grid-area: head; min-width: 0; }
  .scope { grid-area: scope; min-width: 0; }
  .kpis { grid-area: kpis; min-width: 0; overflow-x: auto; }
  .kpis :global(.kpi-strip) { min-width: 800px; }
  .main-column { grid-area: main; display: grid; gap: var(--space-3); min-width: 0; }
  .side-column { grid-area: side; display: grid; gap: var(--space-3); min-width: 0; }
  @media (max-width: 900px) {
    .run-player-screen { grid-template-columns: minmax(0, 1fr); grid-template-areas: 'crumbs' 'head' 'scope' 'kpis' 'main' 'side' 'debug'; }
  }
</style>

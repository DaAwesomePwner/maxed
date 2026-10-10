<script lang="ts">
  import BusyStatus from '../lib/status/BusyStatus.svelte';
  import { historyStatus } from '../lib/status/registry';
  import { debugMode } from '../lib/debug/debug';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { tick, untrack } from 'svelte';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { FightPosition, ListState } from '../lib/navigation/state';
  import { routeHref, type FightRelationship } from '../lib/navigation/routes';
  import { createRunHistory, type HistoryController, type HistoryState } from '../lib/runs/history';
  import HistoryStatus from '../lib/runs/HistoryStatus.svelte';
  import { HISTORY_CATCH_UP } from '../lib/runs/policy';
  import { coverageExplanation, damageRank, fightEndLabels } from '../lib/runs/damage';
  import type { FightView } from '../lib/bindings/FightView';
  import type { RunFightStripRow } from '../lib/bindings/RunFightStripRow';
  import { objectVisual } from '../lib/bridge/visual-index';
  import { spriteUrl } from '../lib/bridge/visuals';
  import SpriteImage from '../lib/components/SpriteImage.svelte';
  import { parseObjectId } from '../lib/navigation/routes';
  import { bossOutcome } from '../lib/home/dungeon';
  import { formatClock, formatClockRange, formatStat } from '../lib/styles/format';
  import { defaultFightPlotted, fightKpis, fightNeighbors } from '../lib/runs/fight-screen';
  import FightSeriesChart from '../lib/runs/FightSeriesChart.svelte';
  import KpiStrip from '../lib/home/v3/KpiStrip.svelte';
  import Breadcrumbs from '../lib/runs/Breadcrumbs.svelte';
  import '../lib/home/v3/area-home.css';
  import KillEvidence from '../lib/runs/KillEvidence.svelte';
  import FightPlayers from '../lib/runs/FightPlayers.svelte';
  import EmptyState from '../lib/components/EmptyState.svelte';
  import ObjectLabel from '../lib/home/ObjectLabel.svelte';
  import { createHomeVisuals, emptyHomeVisuals } from '../lib/home/visuals';
  let { client, fightId, runId, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady, onRelationship, enabled = true, savedState, onStateChange }: {
    client?: Client; fightId: string; runId?: string; streamId?: string | null; scopeKey?: string;
    required?: string | null; store?: StoreStatus | null; entryKey?: string;
    onRowsReady?: (key: string) => void; enabled?: boolean;
    onRelationship?: (relationship: FightRelationship) => void;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
  } = $props();
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading saved fight.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let retryShown = $state(false);
  let visuals = $state.raw(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let fight = $derived(history.value?.kind === 'fight' ? history.value.fight : null);
  let outcome = $derived(fight ? bossOutcome(fight) : null);
  let position = $state<FightPosition>({ playerWindow: 0, selectedPlayerId: null, sourceWindow: 0 });
  const strips = new WeakMap<FightView, RunFightStripRow[] | null>();
  let strip = $derived(fight ? strips.get(fight) ?? null : null);
  let neighbors = $derived(fightNeighbors(strip ?? [], fightId));
  let self = $derived(fight?.players.filter(row => row.is_local).length === 1 ? fight.players.find(row => row.is_local)! : null);
  let plotted = $derived(position.plotted ?? (fight ? defaultFightPlotted(fight) : []));
  let boss = $derived(objectVisual(visuals.objects, fight?.target_type ?? fight?.boss_type ?? null));
  let bossName = $derived(boss?.name ?? 'Unknown target');
  let selfHref = $derived(fight && self && fight.players.filter(row => row.object_id === self.object_id).length === 1 && parseObjectId(String(self.object_id)) !== null
    ? routeHref({ kind: 'player', fightId: fight.id, runId: fight.run_id ?? undefined, objectId: self.object_id }) : null);
  let sources = $derived([...(self?.sources ?? [])].sort((a, b) => b.damage - a.damage).slice(0, 3));
  const stripName = (row: RunFightStripRow) => row.target_name ?? objectVisual(visuals.objects, row.target_type ?? row.boss_type)?.name ?? 'Unknown boss';
  const fightHref = (id: string) => routeHref({ kind: 'fight', id, runId: fight?.run_id ?? undefined });
  $effect(() => {
    const identity = [entryKey, scopeKey, streamId, fightId];
    untrack(() => {
      void identity;
      position = savedState?.fightPosition ?? { playerWindow: 0, selectedPlayerId: null, sourceWindow: 0 };
    });
  });
  function savePosition(patch: Partial<FightPosition>) {
    position = { ...position, ...patch };
    onStateChange?.({ fightPosition: { ...position } });
  }
  let mismatch = $derived(fight !== null && (fight.id !== fightId || (runId !== undefined && fight.run_id !== runId)));
  let available = $derived(!client || history.value !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  $effect(() => {
    const report = { entryKey, scopeKey, streamId, fightId: fight?.id ?? null, runId: fight?.run_id ?? null };
    const receive = onRelationship;
    untrack(() => receive?.(report));
  });
  $effect(() => {
    const owner = client, id = fightId, parentId = runId, routeScope = scopeKey, routeStream = streamId;
    if (!owner || !enabled) return;
    let current = true;
    history = { phase: 'loading', value: null, message: 'Loading saved fight.', retryAvailable: false };
    // No cache: this wrapper also fills component-local fight strips.
    const instance = createRunHistory({ ...owner, getFight: async () => {
      const reply = await owner.getFight(id);
      if (!current) return { status: 'superseded' };
      if (reply.status !== 'ready' || reply.value === null) return reply;
      const value = reply.value;
      if (value.id !== id || (parentId !== undefined && value.run_id !== parentId) || value.run_id === null) return reply;
      try {
        const result = await owner.getRunFightStrip(value.run_id, null);
        if (!current || result.status === 'superseded') return { status: 'superseded' };
        strips.set(value, result.status === 'ready' ? result.value : null);
      } catch {
        if (!current) return { status: 'superseded' };
        strips.set(value, null);
      }
      return reply;
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
    if (!client || !enabled) return;
    const instance = createHomeVisuals(client, next => { visuals = next; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = visualController;
    const rows = fight && !mismatch ? fight.players : [];
    const types = [!mismatch ? fight?.target_type ?? fight?.boss_type : null,
      ...rows.flatMap(row => [row.class_id, ...row.sources.map(source => source.item_type)]),
      ...(strip ?? []).map(row => row.target_type ?? row.boss_type)]
      .filter((type): type is number => type != null);
    untrack(() => instance?.update(types));
  });
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready) return;
    let current = true;
    void tick().then(() => { if (current) onRowsReady?.(key); });
    return () => { current = false; };
  });
</script>

<BusyStatus message={client ? historyStatus(history, 'fight') : null} />

<div class="fight">
  <HistoryStatus {history} />
  {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<DebugNote text={history.message} />{/if}
  {#if retryShown}<button class="ctl" type="button" aria-disabled={!history.retryAvailable} onclick={() => { if (history.retryAvailable) controller?.retry(); }}>Retry</button>{/if}
  {#if !client}
    <EmptyState state="unavailable" heading="No Fights" description="" />
  {:else if mismatch}
    <EmptyState state="unavailable" heading="Fight Unavailable" description="" />
  {:else if fight}
    <div class="area-home fight-screen">
      <div class="crumbs"><Breadcrumbs items={[{ label: 'Runs', href: '#/runs' },
        ...(fight.run_id !== null ? [{ label: 'Run', href: routeHref({ kind: 'run', id: fight.run_id }) }] : []), { label: bossName }]} /></div>
      <header class="area-header">
        <span class="run-icon"><SpriteImage sprite={boss?.sprite ?? null} sheetUrl={boss?.sprite ? spriteUrl(visuals.client_fingerprint, boss.sprite) : null} fit={32} alt={bossName} /></span>
        <div class="title"><h2>{bossName}</h2><p>{fight.boss_type !== null ? 'Boss' : 'Target'}{#if neighbors.index >= 0}{' · '}fight {neighbors.index + 1} of {neighbors.count}{/if}{' · '}{formatClockRange(fight.started, fight.ended)}</p></div>
        <span class="chip" class:killed={outcome === 'killed'} class:ongoing={outcome === 'live'}>{outcome === 'killed' ? 'Killed' : outcome === 'live' ? 'In progress' : 'Ended'}</span>
        {#if !fight.complete_capture || fight.issues.length || fight.kill?.provisional}<span class="tag">partial</span>{/if}
        <nav class="neighbors" aria-label="Adjacent boss fights">
          <a class="ctl ghost" aria-label={neighbors.previous ? `Previous boss: ${stripName(neighbors.previous)}` : 'Previous boss'} aria-disabled={neighbors.previous === null} href={neighbors.previous ? fightHref(neighbors.previous.fight_id) : undefined}>‹ {neighbors.previous ? stripName(neighbors.previous) : 'Previous boss'}</a>
          <a class="ctl ghost" aria-label={neighbors.next ? `Next boss: ${stripName(neighbors.next)}` : 'Next boss'} aria-disabled={neighbors.next === null} href={neighbors.next ? fightHref(neighbors.next.fight_id) : undefined}>{neighbors.next ? stripName(neighbors.next) : 'Next boss'} ›</a>
        </nav>
      </header>
      <div class="area-kpis"><KpiStrip cells={fightKpis(fight, self)} /></div>
      <FightSeriesChart {fight} {plotted} onToggle={id => savePosition({ plotted: plotted.includes(id) ? plotted.filter(value => value !== id) : [...plotted, id] })} />
      <FightPlayers {fight} {visuals} runId={fight.run_id ?? undefined} />
      <aside class="side">
        <section class="area-card" aria-label="Your damage sources">
          <div class="card-heading"><h3>Your damage sources</h3>{#if selfHref}<a class="all" href={selfHref}>All ›</a>{/if}</div>
          {#each sources as source}
            <div class="source"><div class="source-label"><ObjectLabel type={source.item_type} kind="item" {visuals} enchant={source.enchant} /><span>{formatStat(source.damage)}</span></div>
              {#if source.estimated}<span class="tag">estimated</span>{/if}
              <div class="source-bar"><i style:width={`${self && self.damage > 0 ? Math.max(0, Math.min(100, source.damage / self.damage * 100)) : 0}%`}></i></div>
            </div>
          {:else}<p class="muted">—</p>{/each}
        </section>
        <section class="area-card" aria-label="You this run"><h3>You this run</h3>
          {#if strip?.length}<div class="strip-scroll"><table><thead><tr><th>Boss</th><th>Damage</th><th>Rank</th></tr></thead><tbody>
            {#each strip as row (row.fight_id)}<tr class:current={row.fight_id === fight.id}><th scope="row"><a href={fightHref(row.fight_id)} aria-current={row.fight_id === fight.id ? 'page' : undefined} title={stripName(row)}>{stripName(row)}</a></th><td>{formatStat(row.damage)}</td><td>{damageRank(row.rank)}</td></tr>{/each}
          </tbody></table></div>{:else}<p class="muted">—</p>{/if}
        </section>
        <section class="area-card" aria-label="Fight facts"><h3>Fight facts</h3><dl class="facts">
          <dt>Ended</dt><dd class:good={fight.end_reason === 'killed'}>{fight.end_reason === null ? '—' : fightEndLabels[fight.end_reason].toLowerCase().replace(/^./, letter => letter.toUpperCase())}</dd>
          <dt>Boss HP</dt><dd>{formatStat(fight.max_hp)}</dd>
          <dt>Started</dt><dd>{formatClock(fight.started) ?? '—'}</dd><dt>Last hit</dt><dd>{formatClock(fight.last_hit) ?? '—'}</dd>
          <dt>Capture</dt><dd>{fight.complete_capture && fight.issues.length === 0 ? 'Complete' : 'Partial'}</dd>
        </dl></section>
      </aside>
      {#if $debugMode}<details class="area-debug"><summary>Capture evidence</summary><KillEvidence kill={fight.kill ?? null} />
        {#if fight.issues.length}<ul>{#each fight.issues as issue}<li>{coverageExplanation[issue]}</li>{/each}</ul>{/if}
        <p>Unattributed damage is already part of the total. Unassigned or omitted reconstruction cannot establish missing damage. An encounter end does not prove a defeated target.</p>
      </details>{/if}
    </div>
  {:else if history.value !== null}
    <EmptyState heading="Fight Not Found" description="">
      {#snippet action()}<a href="#/runs">Runs</a>{/snippet}
    </EmptyState>
  {:else if history.phase === 'error'}
    <EmptyState state="error" heading="Fight Error" description="" />
  {:else}
    <EmptyState state="loading" heading="Loading Fight" description="" />
  {/if}
</div>

<style>
  .fight { overflow-anchor: none; min-width: 0; }
  .fight-screen { height: auto; grid-template-columns: minmax(0, 1fr) 300px; grid-template-rows: auto auto auto auto 1fr; grid-template-areas: 'crumbs crumbs' 'head head' 'kpis kpis' 'chart side' 'table side'; align-items: start; }
  .crumbs { grid-area: crumbs; min-width: 0; }
  .area-header { flex-wrap: wrap; } .title { min-width: 0; }
  .title h2 { overflow-wrap: anywhere; }
  .neighbors { margin-left: auto; display: flex; gap: 6px; flex-wrap: wrap; min-width: 0; }
  .neighbors a { height: 26px; min-height: 26px; box-sizing: border-box; max-width: 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chip { border: 1px solid var(--border); border-radius: 99px; padding: 3px 8px; color: var(--text-muted); }
  .chip.killed { color: var(--success); border-color: #75dfa140; background: #75dfa114; }
  .chip.ongoing { color: var(--gold); border-color: #f2c14e55; }
  .tag { font-size: 10px; color: var(--text-muted); border: 1px solid var(--border); border-radius: 4px; padding: 0 5px; }
  .side { grid-area: side; display: grid; align-self: start; align-content: start; gap: 10px; min-width: 0; padding: 0; border: 0; background: transparent; }
  .card-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }
  .all { color: var(--violet-text); text-decoration: none; }
  .source + .source { margin-top: 8px; }
  .source-label { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 4px; font-variant-numeric: tabular-nums; }
  .source-label :global(.object-label) { min-width: 0; }
  .source-label :global(.object-label > span:last-child) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .source-bar { height: 6px; background: #00000040; border-radius: 2px; }
  .source-bar i { display: block; height: 100%; background: var(--gold); border-radius: inherit; }
  .strip-scroll { max-height: 260px; overflow: auto; }
  table { width: 100%; table-layout: fixed; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { text-align: right; height: 30px; padding: 0 4px; font-weight: 400; }
  th:first-child { width: 52%; text-align: left; }
  thead th { font-size: 11px; color: var(--text-muted); }
  tbody tr { border-top: 1px solid var(--border); }
  td { white-space: nowrap; }
  table a { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: inherit; text-decoration: none; line-height: 30px; }
  .current { background: #f2c14e14; } .current th { box-shadow: inset 2px 0 var(--gold); padding-left: 6px; }
  .current td:last-child { color: var(--gold); }
  .facts { margin: 0; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 5px 12px; }
  .good { color: var(--success); }
  .facts dt, .muted { color: var(--text-muted); } .facts dd { margin: 0; text-align: right; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .area-debug { grid-column: 1 / -1; } summary { min-height: 26px; cursor: pointer; }
  @media (max-width: 950px) { .fight-screen { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto; grid-template-areas: 'crumbs' 'head' 'kpis' 'chart' 'table' 'side'; } }
</style>

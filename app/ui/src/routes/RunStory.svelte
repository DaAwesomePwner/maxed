<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { DungeonBest } from '../lib/bindings/DungeonBest';
  import type { SpriteRef } from '../lib/bindings/SpriteRef';
  import { productionFeatures, routeHref, type Features } from '../lib/navigation/routes';
  import { fightPlayerHref, runPlayerHref } from '../lib/players/links';
  import { emptyListState, normalizeListState, type ListState, type StoryPosition } from '../lib/navigation/state';
  import { createRunHistory, type HistoryController, type HistoryState } from '../lib/runs/history';
  import { HISTORY_CATCH_UP } from '../lib/runs/policy';
  import BusyStatus from '../lib/status/BusyStatus.svelte';
  import { historyStatus } from '../lib/status/registry';
  import HistoryStatus from '../lib/runs/HistoryStatus.svelte';
  import EmptyState from '../lib/components/EmptyState.svelte';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import { debugMode } from '../lib/debug/debug';
  import { createDungeonPortal, createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../lib/home/visuals';
  import { objectVisual } from '../lib/bridge/visual-index';
  import { spriteUrl } from '../lib/bridge/visuals';
  import SpriteImage from '../lib/components/SpriteImage.svelte';
  import { coverageLabels } from '../lib/home/evidence';
  import { bossOutcome, type DungeonMarker } from '../lib/home/dungeon';
  import { clearAndDeath, estimatedClear, evidenceText, observedThrough, storyVisualTypes } from '../lib/runs/story';
  import { runCrumbs, runHeaderLine, runKpis, runMarkers, otherTargets } from '../lib/runs/run-screen';
  import { formatClock, formatClockRange, formatDps, formatShare, formatStat } from '../lib/styles/format';
  import Breadcrumbs from '../lib/runs/Breadcrumbs.svelte';
  import ShowMore from '../lib/runs/ShowMore.svelte';
  import { limitRows, renderCount, RENDER_STEP, type RenderLimit } from '../lib/runs/render-limit';
  import KpiStrip from '../lib/home/v3/KpiStrip.svelte';
  import FightTabs from '../lib/home/v3/FightTabs.svelte';
  import DamageTable from '../lib/home/v3/DamageTable.svelte';
  import BossHpBar from '../lib/home/v3/BossHpBar.svelte';
  import FightChart from '../lib/home/v3/FightChart.svelte';
  import BestBagCard from '../lib/home/v3/BestBagCard.svelte';
  import BagList from '../lib/home/v3/BagList.svelte';
  import LootTally from '../lib/home/v3/LootTally.svelte';
  import DungeonTimeline from '../lib/home/v2/DungeonTimeline.svelte';
  import { homeLoot } from '../lib/home/v3/loot-rank';
  import { damageRows, fightDps } from '../lib/home/v3/screen-data';
  import '../lib/home/v3/area-home.css';
  import '../lib/runs/run-screen.css';

  let { runId, point, client, features = productionFeatures, enabled = true, streamId = null, scopeKey = '', required = null, store = null,
    savedState = emptyListState(), onStateChange, entryKey = '', onRowsReady }: {
    features?: Features; runId: string; point?: 'death'; client?: Client; enabled?: boolean; streamId?: string | null; scopeKey?: string;
    required?: string | null; store?: StoreStatus | null; savedState?: ListState;
    onStateChange?: (patch: Partial<ListState>) => void; entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  const restored = untrack(() => normalizeListState(savedState));
  let position = $state<StoryPosition>(restored.storyPosition ?? { allFights: false, otherTargets: false });
  let region: HTMLDivElement;
  let missingRow = $state('');
  let selectedDropId = $state<string | null>(restored.focusedRowId?.startsWith('drop:') ? restored.focusedRowId.slice(5) : null);
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: '', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let portalController = $state.raw<ReturnType<typeof createDungeonPortal>>();
  let portalType = $state<number | null>(null);
  let bests = $state.raw<DungeonBest[] | null>(null);
  let retryShown = $state(false);
  let story = $derived(history.value?.kind === 'run' ? history.value.story : null);
  let loaded = $derived(history.value?.kind === 'run');
  let bosses = $derived(story?.boss_fights ?? []);
  let others = $derived(story ? otherTargets(story) : []);
  let selected = $derived(position.selectedTab ?? 'whole');
  let fight = $derived(bosses.find(row => row.id === selected) ?? null);
  let self = $derived.by(() => { const rows = fight?.players.filter(player => player.is_local) ?? []; return rows.length === 1 ? rows[0] : null; });
  let totals = $derived(story?.totals ?? null);
  let local = $derived(totals?.local ?? null);
  let rows = $derived(fight ? damageRows(fight.players, self) : damageRows(totals?.players ?? [], local));
  let limit = $state<RenderLimit>({ key: '', count: RENDER_STEP });
  let tableKey = $derived(`${runId}:${selected}`);
  let count = $derived(renderCount(limit, tableKey));
  let shownRows = $derived(limitRows(rows, count, row => row.is_local));
  let shownOthers = $derived(limitRows(others, count));
  const showMore = () => { limit = { key: tableKey, count: count + RENDER_STEP }; };
  let loot = $derived(homeLoot(story?.drops ?? [], visuals));
  let markers = $derived(story ? runMarkers(story) : []);
  let names = $derived(Object.fromEntries((story?.fights ?? []).filter(row => row.target_name).map(row => [row.id, row.target_name!])));
  let portal = $derived(objectVisual(visuals.objects, portalType));
  const resolveSpriteUrl = (sprite: SpriteRef) => spriteUrl(visuals.client_fingerprint, sprite);
  const clock = (at: string) => formatClock(at) ?? '—';
  const fightHref = (id: string) => routeHref({ kind: 'fight', id, runId });

  $effect(() => {
    const owner = client; scopeKey; streamId;
    if (!owner || !enabled) { controller = undefined; return; }
    let current = true, revision = 0, bestsKey = '';
    bests = null;
    // One sequential request owns the history scheduler; PB failure must not hide a saved run.
    // No cache: this wrapper also fills component-local personal bests.
    const instance = createRunHistory({ ...owner, getRun: async id => {
      const reply = await owner.getRun(id);
      if (reply.status === 'ready' && reply.value && bestsKey !== id) {
        const attempt = revision;
        try {
          const best = await owner.queryDungeonBests();
          if (current && attempt === revision && best.status === 'ready') { bests = best.value; bestsKey = id; }
        }
        catch { /* Keep the last saved PBs while history is unavailable. */ }
      }
      return current ? reply : { status: 'superseded' };
    } }, next => { history = next; });
    controller = instance;
    let release: (() => void) | undefined;
    void owner.onAccountHistoryChanged(notice => {
      revision++; bestsKey = '';
      if (notice.reason === 'account_switched') bests = null;
      instance.retry(true);
    }).then(stop => { if (current) release = stop; else stop(); }).catch(() => {});
    return () => { current = false; release?.(); instance.dispose(); };
  });
  $effect(() => {
    const instance = controller, input = { query: { kind: 'run' as const, id: runId }, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  $effect(() => {
    if (!client) return;
    const visual = createHomeVisuals(client, next => { visuals = next; });
    const portal = createDungeonPortal(client, next => { portalType = next; });
    visualController = visual; portalController = portal;
    return () => { visual.dispose(); portal.dispose(); };
  });
  $effect(() => { portalController?.update(story?.run.dungeon ?? null); });
  $effect(() => {
    visualController?.update([...storyVisualTypes(story, others, story?.drops ?? []),
      ...bosses.flatMap(fight => [fight.target_type ?? fight.boss_type, ...fight.players.map(player => player.class_id)]).filter((type): type is number => type != null),
      ...(portalType === null ? [] : [portalType])]);
  });
  $effect(() => { if (history.retryAvailable && (history.phase === 'error' || history.phase === 'busy' || (history.phase === 'stale' && history.message.startsWith('Saving failed.')))) retryShown = true; });
  function choose(id: string) {
    position = { ...position, selectedTab: id };
    onStateChange?.({ storyPosition: $state.snapshot(position), selectedFightId: bosses.some(row => row.id === id) ? id : null });
  }
  let initialized = false;
  $effect(() => {
    if (!story) return;
    if (!initialized) {
      initialized = true;
      if (position.selectedTab == null && point === 'death') {
        const at = story.run.died_at;
        const deathFight = at ? [...bosses].reverse().find(row => BigInt(row.started) <= BigInt(at)) : null;
        untrack(() => choose(deathFight?.id ?? 'whole'));
      }
    }
    if (selected !== 'whole' && !(selected === 'others' && others.length) && !bosses.some(row => row.id === selected)) {
      missingRow = 'Row Removed'; untrack(() => choose('whole'));
    }
    if (selectedDropId && !story.drops.some(drop => drop.id === selectedDropId)) {
      selectedDropId = null; missingRow = 'Row Removed'; untrack(() => onStateChange?.({ focusedRowId: null }));
    }
  });
  $effect.pre(() => {
    story; loot;
    if (!region) return;
    const active = document.activeElement;
    if (!(active instanceof HTMLElement) || !region.contains(active) || !active.closest('[data-row-id], [role="tab"]')) return;
    const rowId = active.closest<HTMLElement>('[data-row-id]')?.dataset.rowId;
    void tick().then(() => {
      if (!region?.isConnected || active.isConnected || document.activeElement !== document.body) return;
      const replacement = rowId ? [...region.querySelectorAll<HTMLElement>('[data-row-id]')].find(row => row.dataset.rowId === rowId) : null;
      if (replacement) { replacement.focus({ preventScroll: true }); return; }
      region.querySelector<HTMLElement>('[data-story-header]')?.focus({ preventScroll: true });
      missingRow = 'Row Removed'; onStateChange?.({ focusedRowId: null });
    });
  });
  $effect(() => {
    const ready = !client || loaded || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable), key = entryKey;
    if (!ready) return;
    selected; selectedDropId;
    let current = true;
    void tick().then(() => { if (current) onRowsReady?.(key); });
    return () => { current = false; };
  });
  async function selectMarker(marker: DungeonMarker) {
    if (marker.kind === 'enemy') { choose(marker.id.slice(6)); await tick(); region.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus(); }
    if (marker.kind === 'bag') {
      selectedDropId = marker.id.slice(5);
      onStateChange?.({ focusedRowId: `drop:${selectedDropId}` });
      await tick();
      const row = [...region.querySelectorAll<HTMLElement>('.area-loot [data-drop-id]')].find(row => row.dataset.dropId === selectedDropId);
      if (row) { row.focus({ preventScroll: true }); row.scrollIntoView?.({ block: 'nearest' }); }
    }
  }
</script>

<div class="run-story" bind:this={region}>
  <BusyStatus message={client ? historyStatus(history, 'run') : null} />
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{missingRow}</p>
  {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<DebugNote text={history.message} />{/if}
  {#if retryShown}<button class="ctl" type="button" aria-disabled={!history.retryAvailable} onclick={() => { if (history.retryAvailable) controller?.retry(); }}>Retry</button>{/if}
  {#if !client}<EmptyState state="unavailable" heading="No Runs" description="" />
  {:else if loaded && !story}<EmptyState heading="Run Not Found" description="" />
  {:else if !story && history.phase !== 'error'}<EmptyState state="loading" heading="Loading Run" description="" />
  {:else if !story}<EmptyState state="error" heading="Run Error" description="" />{/if}
  {#if story}
    <div class="area-home run-screen">
      <div class="run-crumbs"><Breadcrumbs items={runCrumbs(story)} />
        {#if features.party}<a class="ctl" href={routeHref({ kind: 'party-run', runId })}>Party</a>{/if}
      </div>
      <header class="area-header" data-story-header tabindex="-1">
        <span class="run-icon">{#if portal?.sprite}<SpriteImage sprite={portal.sprite} sheetUrl={resolveSpriteUrl(portal.sprite)} fit={40} alt={portal.name ?? 'Dungeon'} />{/if}</span>
        <div class="run-heading"><h2>{story.run.dungeon}</h2><p>{runHeaderLine(story, objectVisual(visuals.objects, story.character?.class_id)?.name ?? null)}</p></div>
        <span class="outcome" data-outcome={story.run.ended === null ? 'open' : story.run.outcome}>{story.run.ended === null ? 'In progress' : story.run.outcome === 'cleared' ? 'Cleared' : story.run.outcome === 'died' ? 'Died' : story.run.outcome === 'left' ? 'Left' : '—'}</span>
        {#if clearAndDeath(story.run)}<span class="warning">Clear and death</span>{/if}
        {#if !story.run.complete_capture || story.run.issues.length || totals?.partial}<span class="quiet-tag">partial</span>{/if}
        {#if estimatedClear(story)}<span class="quiet-tag">estimated clear</span>{/if}
        <div class="run-controls">
          {#if features.logs}<nav aria-label="Run logs"><a class="ctl ghost" href={`#/logs/run/${encodeURIComponent(runId)}`}>Chat log</a><a class="ctl ghost" href={`#/logs/events/run/${encodeURIComponent(runId)}`}>Events</a></nav>{/if}
          <a class="ctl sq" href={story.older_run_id ? routeHref({ kind: 'run', id: story.older_run_id }) : undefined} aria-disabled={!story.older_run_id} aria-label="Older run">‹</a>
          <a class="ctl sq" href={story.newer_run_id ? routeHref({ kind: 'run', id: story.newer_run_id }) : undefined} aria-disabled={!story.newer_run_id} aria-label="Newer run">›</a>
        </div>
      </header>
      <div class="area-kpis"><KpiStrip cells={runKpis(story, bests?.find(best => best.dungeon === story.run.dungeon) ?? null)} /></div>
      <section class="area-card fight-panel" aria-label="Fight damage">
        <FightTabs fights={bosses} {selected} {visuals} {resolveSpriteUrl} {names} onselect={choose} saved
          other={others.length ? { label: `${others.length} other ${others.length === 1 ? 'target' : 'targets'}`, sub: `${formatStat(others.reduce((sum, row) => sum + row.total_damage, 0))} damage` } : null} />
        <div class="fight-detail" data-whole-run={selected === 'whole'}>
          {#if selected === 'others'}
            <div class="other-targets" role="table" aria-label="Other targets">
              <div class="other-row" role="row"><span role="columnheader">Target</span><span role="columnheader">Damage</span><span role="columnheader">Your damage</span><span role="columnheader">Rank</span><span role="columnheader">Hit interval</span></div>
              {#each shownOthers as target (target.id)}<div class="other-row" role="row">
                <span role="cell"><a data-row-id={`fight:${target.id}`} href={fightHref(target.id)}>{objectVisual(visuals.objects, target.target_type)?.name ?? target.target_name ?? 'Unknown target'}</a>{#if target.partial}<small>partial</small>{/if}{#if target.estimated}<small>estimated</small>{/if}</span>
                <span role="cell">{formatStat(target.total_damage)}</span><span role="cell">{formatStat(target.local_damage)}</span><span role="cell">{formatStat(target.local_rank)}</span><span role="cell">{formatClockRange(target.first_hit, target.last_hit)}</span>
              </div>{/each}
            </div>
            <ShowMore shown={shownOthers.length} total={others.length} noun="targets" onmore={showMore} />
          {:else}
            {#if fight}
              <BossHpBar saved hp={fight.target_hp ?? null} max={fight.max_hp} ended={fight.ended !== null}
                killedAt={bossOutcome(fight) === 'killed' ? fight.kill && !fight.kill.provisional ? fight.kill.at : fight.ended : null} />
              <p class="fight-meta"><span>{formatStat(fight.total_damage)} damage · {formatDps(fightDps(fight))} DPS · {fight.players.length} players</span><span class="gold">You: best hit {formatStat(self?.biggest_hit ?? null)} · {formatStat(self?.hits ?? null)} hits</span>{#if !fight.complete_capture || fight.issues.length}<span class="quiet-tag">partial</span>{/if}<a class="open-fight" href={fightHref(fight.id)}>Open fight ›</a></p>
            {:else}<p class="fight-meta"><span>{formatStat(totals?.total_damage ?? null)} damage</span><span>{totals ? `${totals.fights} fights · ${totals.kills} kills` : '—'}</span><span class="gold">You: {formatShare(local?.share ?? null)} · rank {formatStat(local?.rank ?? null)}</span></p>{/if}
            {#if fight || totals}<DamageTable basis={fight ? fight.dps_basis : 'whole_run'} scroll rows={shownRows} total={fight?.players.length ?? totals?.players.length ?? 0} otherDamage={fight?.unattributed_damage ?? totals?.unattributed_damage} {visuals} {resolveSpriteUrl}
              rowHref={row => fight ? fightPlayerHref(fight, row, runId) : runPlayerHref(runId, row)} />
              <ShowMore shown={shownRows.length} total={rows.length} noun="players" onmore={showMore} />{:else}<p>No totals available</p>{/if}
            {#if fight}<FightChart buckets={fight.buckets} /><p class="sr-only">{fight.buckets.length} observed damage intervals. Party damage {formatStat(fight.total_damage)}; your DPS {formatDps(self?.dps)}.</p>{/if}
          {/if}
        </div>
      </section>
      <section class="area-card area-loot" aria-label="Loot this run"><h3>Loot this run</h3>
        <BestBagCard selected={selectedDropId !== null && selectedDropId === loot.best?.drop.id} best={loot.best} {visuals} {resolveSpriteUrl} meta={loot.best ? clock(loot.best.drop.at) : ''} scope={runId} />
        <div class="bag-space"><BagList drops={loot.bags} {visuals} {resolveSpriteUrl} {clock} scroll {selectedDropId} /></div>
        <LootTally {...loot.counts} />
      </section>
      <div class="area-timeline"><DungeonTimeline run={story.run} at={story.run.ended ?? observedThrough(story)} {markers} {visuals} partial={!story.run.complete_capture || !!story.events_limited} onselect={selectMarker} saved /></div>
      {#if point === 'death'}<section class="run-death" data-anchor="death" tabindex="-1" aria-label="Death"><h3>Death</h3><p>{story.run.died_at ? `Saved death timestamp: ${clock(story.run.died_at)}` : 'No death observation is saved for this run.'}</p></section>{/if}
      {#if $debugMode}<details class="area-debug"><summary>Run evidence</summary>
        {#each story.run.evidence as evidence}<p>{evidenceText(evidence)}</p>{/each}
        {#each story.run.issues as issue}<p>{coverageLabels[issue]}</p>{/each}
        {#if !totals}<p>Run totals unavailable: this saved story has no persisted aggregate. Fight summaries are not a substitute.</p>{/if}
        {#if story.run.party_size_estimated}<p>Estimated party size.</p>{/if}
        {#if estimatedClear(story)}<p>Estimated clear time.</p>{/if}
        {#if story.run.ended === null}<p>Observed through last saved state: {clock(observedThrough(story))}.</p>{/if}
        <p>Comparisons span all characters. Requires three earlier clears with observed completion times and an observed clear time for this run.</p>
        {#if story.comparison}<p>Clears: {story.comparison.previous_clears}; average: {formatStat(story.comparison.mean_micros / 1000000)}s.</p>{/if}
        {#if story.boss_fights_limited}<p>Some boss fights were omitted from this saved view.</p>{/if}
        {#if story.events_limited}<p>Some events were omitted from this saved view.</p>{/if}
        <p>Instance context describes the observation at receipt, not a historical roster or current population.</p>
        {#each Object.entries(story.run.context ?? {}) as [key, value]}{#if value != null}<p>{key}: {Array.isArray(value) ? value.join(', ') : String(value)}</p>{/if}{/each}
        {#each fight?.issues ?? [] as issue}<p>{coverageLabels[issue]}</p>{/each}
      </details>{/if}
    </div>
  {/if}
</div>

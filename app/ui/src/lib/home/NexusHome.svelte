<script lang="ts">
  import { objectVisual } from '../bridge/visual-index';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import type { HomeScope } from '../bindings/HomeScope';
  import type { LiveSnapshot } from '../bindings/LiveSnapshot';
  import type { ScopeLootPage } from '../bindings/ScopeLootPage';
  import type { HomeHistoryState } from './summary';
  import type { ScopeChoice } from './scope';
  import type { KpiCell } from './v3/types';
  import type { LootHistoryState } from './v3/nexus-history';
  import { emptyHomeVisuals, type HomeVisuals } from './visuals';
  import { spriteUrl } from '../bridge/visuals';
  import { dpsMetric } from '../runs/dps-metric';
  import { formatDps, formatStat } from '../styles/format';
  import { statText } from './evidence';
  import { plural } from '../loot/presentation';
  import { dayCaption, clockTime } from './nexus';
  import { routeHref } from '../navigation/routes';
  import { debugMode } from '../debug/debug';
  import DebugNote from '../debug/DebugNote.svelte';
  import SpriteImage from '../components/SpriteImage.svelte';
  import KpiStrip from './v3/KpiStrip.svelte';
  import BestBagCard from './v3/BestBagCard.svelte';
  import { bestItem } from './v3/loot-rank';
  import ScopeLoot from './v3/ScopeLoot.svelte';
  import NexusCaveats from './v3/NexusCaveats.svelte';
  import DungeonBreakdown from './DungeonBreakdown.svelte';
  import FameSparkline from './FameSparkline.svelte';
  import RecentRuns from './RecentRuns.svelte';
  let { snapshot, statsEnabled = false, history, choice, syntheticClock = false, retry, visuals = emptyHomeVisuals(), scope = null, onScopeChange,
    loot = null, lootPage = 1, lootStatus, onRetryLoot = () => {}, onNextLoot = () => {}, onFirstLoot = () => {} }: {
    snapshot: LiveSnapshot | null; statsEnabled?: boolean; history: HomeHistoryState; visuals?: HomeVisuals; choice: ScopeChoice;
    syntheticClock?: boolean; retry: () => void; scope?: HomeScope | null; onScopeChange?: (choice: ScopeChoice) => void;
    loot?: ScopeLootPage | null; lootPage?: number; lootStatus?: LootHistoryState; onRetryLoot?: () => void; onNextLoot?: () => void; onFirstLoot?: () => void;
  } = $props();
  let summary = $derived(history.summary);
  let period = $derived(choice === 'session' ? 'Session' : 'Today');
  let top = $derived(summary?.top_fight ?? null);
  let locals = $derived(top?.players.filter(player => player.is_local) ?? []);
  let local = $derived(locals.length === 1 ? locals[0] : null);
  let boss = $derived(objectVisual(visuals.objects, (top?.target_type ?? top?.boss_type)));
  let bossName = $derived(boss?.name ?? (top ? `Enemy ${top.target_type ?? top.boss_type ?? '—'}` : '—'));
  let fameCharacters = $derived(summary?.fame_characters ?? []);
  // The tile follows the current character, or the latest observed one outside the game.
  let fameCharacter = $derived(snapshot?.character?.id ?? fameCharacters.at(-1)?.character_id ?? null);
  let characterFame = $derived(fameCharacters.find(row => row.character_id === fameCharacter) ?? null);
  let fameDelta = $derived(characterFame ? characterFame.last_fame - characterFame.first_fame : null);
  let cells = $derived<KpiCell[]>([
    { key: 'runs', label: 'Runs', value: statText(summary?.runs) ?? '—', sub: `${statText(summary?.clears) ?? '—'} cleared` },
    { key: 'deaths', label: 'Deaths', value: statText(summary?.deaths) ?? '—', sub: `in ${statText(summary?.runs) ?? '—'} ${plural(summary?.runs ?? 0, 'run')}` },
    { key: 'fame', label: 'Fame', value: fameDelta !== null ? `${fameDelta > 0 ? '+' : ''}${formatStat(fameDelta)}` : '—', tone: 'fame',
      sub: characterFame ? `${formatStat(characterFame.first_fame)} → ${formatStat(characterFame.last_fame)}` : '—' },
    { key: 'dps', label: 'Top active DPS', tooltip: `${dpsMetric('player_active_window_v1').tooltip} Best among complete, issue-free ended fights with at least 20 seconds from first to last hit.`, value: formatDps(local?.dps), sub: bossName },
  ]);
  let best = $derived.by(() => {
    const drop = summary?.best_drop;
    if (!drop) return null;
    const item = bestItem(drop, visuals);
    return { drop, item };
  });
  let location = $derived(summary?.recent_runs.find(run => run.id === summary?.best_drop?.run_id)?.dungeon ?? null);
  let meta = $derived([location, clockTime(summary?.best_drop?.at)].filter(Boolean).join(' · ') || '—');
  let error = $derived(history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable) || history.message.startsWith('Saving failed.'));
  let scopeKey = $derived(JSON.stringify([snapshot?.stream_id, choice, scope]));
  let sessionButton: HTMLButtonElement | undefined = $state();
  let heading: HTMLHeadingElement | undefined = $state();
  function retryHistory(event: MouseEvent) {
    // Retry disappears outside errors: preserve keyboard focus before removal.
    if (document.activeElement === event.currentTarget) (sessionButton ?? heading)?.focus({ preventScroll: true });
    retry();
  }
  const resolve = (sprite: import('../bindings/SpriteRef').SpriteRef) => spriteUrl(visuals.client_fingerprint, sprite);
</script>
<BusyStatus message={historyStatus(history, 'history')} />
<BusyStatus message={historyStatus(lootStatus, 'loot')} />
<div class="nexus-home" class:debug={$debugMode}>
  <header class="nexus-header">
    <div><h2 tabindex="-1" bind:this={heading}>{snapshot?.map ?? (snapshot?.area_kind === 'realm' || snapshot?.area_kind === 'other_hub' ? 'Realm' : snapshot?.capture.state === 'Live' && snapshot.area_kind === 'unknown' ? 'Live' : 'Nexus')}</h2><p>Session since {clockTime(snapshot?.session?.started) ?? '—'}</p></div>
    {#if onScopeChange}<div class="scope-controls seg" role="group" aria-label="History Period"><button bind:this={sessionButton} aria-pressed={choice === 'session'} onclick={() => { if (choice !== 'session') onScopeChange?.('session'); }}>Session</button><button aria-pressed={choice === 'today'} onclick={() => { if (choice !== 'today') onScopeChange?.('today'); }}>Today</button></div>{/if}
  </header>
  {#if error || history.phase === 'unavailable'}<div class="history-status" role="status">
    <span>{error ? "Couldn't load history" : 'No history'}</span>
    {#if error}<button class="ctl" disabled={!history.retryAvailable} onclick={retryHistory}>Retry</button>{/if}
  </div>{/if}
  {#if $debugMode}<DebugNote>{syntheticClock && choice === 'today' ? 'Demo Day · ' : ''}{choice === 'today' ? dayCaption(scope) : ''}</DebugNote>{/if}
  <div class="kpis" aria-label={`${period} Stats`}>
    <KpiStrip {cells} />
    {#if top}<a class="fight-link" href={routeHref({ kind: 'fight', id: top.id, runId: top.run_id ?? undefined })} aria-label={`Open fight: ${bossName}`}>
      <span class="boss-sprite"><SpriteImage sprite={boss?.sprite ?? null} sheetUrl={boss?.sprite ? resolve(boss.sprite) : null} fit={32} alt={bossName} /></span>
    </a>{/if}
  </div>
  <section class="panel fame"><h3>Observed character fame</h3><FameSparkline points={summary?.fame ?? []} runs={summary?.recent_runs ?? []} {period} partial={(summary?.fame_missing_baselines ?? 0) > 0} sampled={summary?.fame_sampled ?? false} gaps={summary?.issues.includes('stream_gap') ?? false} /></section>
  <section class="panel loot"><h3>Best bag</h3>
    <BestBagCard {best} {visuals} resolveSpriteUrl={resolve} {meta} scope={summary ? scopeKey : null} />
    <h3 class="loot-title">Loot this {choice === 'session' ? 'session' : 'day'}</h3>
    <div class="loot-content" aria-busy={lootStatus?.phase === 'loading'}>
      {#if lootStatus && (lootStatus.phase === 'error' || (lootStatus.phase === 'busy' && lootStatus.retryAvailable))}<div class="loot-status" role="status">
        <span>{lootStatus.phase === 'error' ? "Couldn't load loot" : 'Loot busy'}</span>
        {#if lootStatus.retryAvailable}<button class="ctl" onclick={onRetryLoot}>Retry loot</button>{/if}
        <DebugNote text={lootStatus.message} />
      </div>{/if}
      <ScopeLoot page={loot} pageNumber={lootPage} {visuals} resolveSpriteUrl={resolve} enabled={history.phase === 'ready' && (!lootStatus || lootStatus.phase === 'ready')} next={onNextLoot} first={onFirstLoot} />
    </div>
  </section>
  {#key scopeKey}<div class="tables"><DungeonBreakdown allTimeHref={statsEnabled ? '#/stats/dungeons' : null} rows={summary?.dungeon_stats} /><RecentRuns runs={summary?.recent_runs ?? []} details={summary?.recent_details ?? []} {visuals} /></div>{/key}
  <NexusCaveats {history} {snapshot} />
</div>
<style>
  .nexus-home { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr); gap: var(--space-3); min-width: 0; align-content: start; }
  .nexus-home .nexus-header { grid-column: 1 / -1; display: flex; align-items: center; gap: var(--space-3); }
  .nexus-home h2 { margin: 0; font-size: var(--text-hero); line-height: var(--leading-hero); font-weight: var(--weight-medium); }
  .nexus-home p { margin: 0; color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .nexus-home .scope-controls { margin-left: auto; }
  .nexus-home .history-status, .nexus-home .loot-status { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2) var(--space-3);
    color: var(--text-secondary); font-size: var(--text-caption); line-height: var(--leading-caption); }
  .nexus-home .history-status { grid-column: 1 / -1; }
  .nexus-home .kpis { grid-column: 1 / -1; position: relative; min-width: 0; }
  .nexus-home .fight-link { position: absolute; inset: 0 0 0 75%; border-radius: 0 var(--radius-card) var(--radius-card) 0; }
  .nexus-home .fight-link:hover { background: rgb(255 255 255 / 3%); }
  .nexus-home .fight-link:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .nexus-home .boss-sprite { position: absolute; right: var(--space-2); top: 50%; transform: translateY(-50%); }
  .nexus-home .kpis :global([data-kpi="dps"] dt) { position: relative; z-index: 1; width: fit-content; }
  .nexus-home .kpis :global([data-kpi="dps"]) { padding-right: var(--space-12); }
  /* Shared card style for every Nexus panel and table (DungeonBreakdown and RecentRuns match it). */
  .nexus-home .panel { min-width: 0; min-height: 264px; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); }
  .nexus-home h3 { margin: 0 0 var(--space-3); color: var(--text-primary); font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  .nexus-home .fame { display: flex; flex-direction: column; }
  .nexus-home .loot { display: flex; flex-direction: column; gap: var(--space-1); }
  .nexus-home .loot-title { margin: var(--space-3) 0 var(--space-1); }
  .nexus-home :global(.history-link) { margin-left: auto; color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta);
    text-decoration: none; text-underline-offset: 3px; white-space: nowrap; }
  .nexus-home :global(.history-link:hover), .nexus-home :global(.history-link:focus-visible) { color: var(--text-primary); text-decoration: underline; }
  .nexus-home .tables { grid-column: 1 / -1; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr); gap: var(--space-3); min-width: 0; }
  .nexus-home.debug .panel { height: auto; }
  .nexus-home > :global([data-debug-note]) { grid-column: 1 / -1; }
  @media (max-width: 900px) {
    .nexus-home .panel { padding: var(--space-3); }
    .nexus-home, .nexus-home .tables { gap: var(--space-2); }
  }
  @media (max-width: 650px) {
    .nexus-home, .nexus-home .tables { grid-template-columns: minmax(0, 1fr); }
  }
</style>

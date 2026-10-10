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
  import { formatDps, formatStat } from '../styles/format';
  import { statText } from './evidence';
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
  let points = $derived(summary?.fame.filter(point => /^\d+$/.test(point.at) && Number.isFinite(point.fame) && point.fame >= 0) ?? []);
  let cells = $derived<KpiCell[]>([
    { key: 'runs', label: 'Runs', value: statText(summary?.runs) ?? '—', sub: `${statText(summary?.clears) ?? '—'} cleared` },
    { key: 'deaths', label: 'Deaths', value: statText(summary?.deaths) ?? '—', sub: `in ${statText(summary?.runs) ?? '—'} runs` },
    { key: 'fame', label: 'Fame', value: summary?.fame_delta != null && Number.isFinite(summary.fame_delta) ? `${summary.fame_delta > 0 ? '+' : ''}${formatStat(summary.fame_delta)}` : '—', tone: 'fame',
      sub: points.length ? `${formatStat(points[0].fame)} → ${formatStat(points[points.length - 1].fame)}` : '—' },
    { key: 'dps', label: 'Top DPS', value: formatDps(local?.dps), sub: bossName },
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
    {#if onScopeChange}<div class="scope-controls" role="group" aria-label="History Period"><button bind:this={sessionButton} aria-pressed={choice === 'session'} onclick={() => { if (choice !== 'session') onScopeChange?.('session'); }}>Session</button><button aria-pressed={choice === 'today'} onclick={() => { if (choice !== 'today') onScopeChange?.('today'); }}>Today</button></div>{/if}
  </header>
  {#if error || history.phase === 'unavailable'}<div class="history-status" role="status">
    {error ? "Couldn't load. Retry." : 'No History'}
    {#if error}<button disabled={!history.retryAvailable} onclick={retryHistory}>Retry</button>{/if}
  </div>{/if}
  {#if $debugMode}<DebugNote>{syntheticClock && choice === 'today' ? 'Demo Day · ' : ''}{choice === 'today' ? dayCaption(scope) : ''}</DebugNote>{/if}
  <div class="kpis" aria-label={`${period} Stats`}>
    <KpiStrip {cells} />
    {#if top}<a class="fight-link" href={routeHref({ kind: 'fight', id: top.id, runId: top.run_id ?? undefined })} aria-label={`Open fight: ${bossName}`}>
      <span class="boss-sprite"><SpriteImage sprite={boss?.sprite ?? null} sheetUrl={boss?.sprite ? resolve(boss.sprite) : null} fit={32} alt={bossName} /></span>
    </a>{/if}
  </div>
  <section class="panel fame"><h3>Observed character fame</h3><FameSparkline points={summary?.fame ?? []} {period} partial={(summary?.fame_missing_baselines ?? 0) > 0} sampled={summary?.fame_sampled ?? false} gaps={summary?.issues.includes('stream_gap') ?? false} /></section>
  <section class="panel loot"><h3>Best bag</h3>
    <BestBagCard {best} {visuals} resolveSpriteUrl={resolve} {meta} scope={summary ? scopeKey : null} />
    <h3 class="loot-title">Loot this {choice === 'session' ? 'session' : 'day'}</h3>
    <div class="loot-content" aria-busy={lootStatus?.phase === 'loading'}>
      {#if lootStatus && (lootStatus.phase === 'error' || (lootStatus.phase === 'busy' && lootStatus.retryAvailable))}<div class="loot-status" role="status">
        {lootStatus.phase === 'error' ? "Couldn't load loot." : 'Loot busy'}
        {#if lootStatus.retryAvailable}<button onclick={onRetryLoot}>Retry Loot</button>{/if}
        <DebugNote text={lootStatus.message} />
      </div>{/if}
      <ScopeLoot page={loot} pageNumber={lootPage} {visuals} resolveSpriteUrl={resolve} enabled={history.phase === 'ready' && (!lootStatus || lootStatus.phase === 'ready')} next={onNextLoot} first={onFirstLoot} />
    </div>
  </section>
  {#key scopeKey}<div class="tables"><DungeonBreakdown allTimeHref={statsEnabled ? '#/stats/dungeons' : null} rows={summary?.dungeon_stats} /><RecentRuns runs={summary?.recent_runs ?? []} details={summary?.recent_details ?? []} {visuals} /></div>{/key}
  <NexusCaveats {history} {snapshot} />
</div>
<style>
  .nexus-home { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr); gap: 10px; min-width: 0; align-content: start; }
  .nexus-home .nexus-header { grid-column: 1 / -1; display: flex; align-items: center; gap: 12px; }
  .nexus-home h2 { margin: 0; font-size: 22px; line-height: 1.1; }
  .nexus-home p { margin: 3px 0 0; font-size: 12px; color: var(--text-secondary); }
  .nexus-home .scope-controls { margin-left: auto; display: flex; padding: 3px; gap: 2px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); }
  .nexus-home button { font: inherit; color: var(--text-secondary); border: 1px solid transparent; border-radius: 6px; padding: 5px 14px; background: transparent; }
  .nexus-home button[aria-pressed="true"] { color: var(--gold); background: var(--surface-raised); border-color: var(--border-strong); }
  .nexus-home .history-status { grid-column: 1 / -1; font-size: 12px; }
  .nexus-home .kpis { grid-column: 1 / -1; position: relative; min-width: 0; }
  .nexus-home .fight-link { position: absolute; inset: 0 0 0 75%; border-radius: 0 8px 8px 0; }
  .nexus-home .fight-link:hover { background: #ffffff05; }
  .nexus-home .fight-link:focus-visible { outline: 2px solid var(--violet-text); outline-offset: -2px; }
  .nexus-home .boss-sprite { position: absolute; right: 8px; top: 50%; transform: translateY(-50%); }
  .nexus-home .kpis :global([data-kpi="dps"]) { padding-right: 46px; }
  .nexus-home .kpis :global([data-kpi="dps"] .value) { font-size: 22px; }
  .nexus-home .panel { min-width: 0; min-height: 262px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); }
  .nexus-home h3 { margin: 0 0 8px; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: .07em; color: var(--text-secondary); }
  .nexus-home .fame { display: flex; flex-direction: column; }
  .nexus-home .loot { display: flex; flex-direction: column; gap: 4px; }
  .nexus-home .loot-title { margin: 4px 0 0; }
  .nexus-home .tables { grid-column: 1 / -1; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.25fr); gap: 10px; min-width: 0; }
  .nexus-home.debug .panel { height: auto; }
  .nexus-home > :global([data-debug-note]) { grid-column: 1 / -1; }
  @media (max-width: 900px) {
    .nexus-home .panel { padding: 8px; }
    .nexus-home, .nexus-home .tables { gap: 8px; }
    .nexus-home .kpis :global([data-kpi="dps"] .value) { font-size: 18px; }
  }
  @media (max-width: 650px) {
    .nexus-home, .nexus-home .tables { grid-template-columns: minmax(0, 1fr); }
  }
</style>

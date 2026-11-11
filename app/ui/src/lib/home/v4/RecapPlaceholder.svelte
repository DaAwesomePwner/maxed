<script lang="ts">
  import type { Outcome } from '../../bindings/Outcome';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { routeHref } from '../../navigation/routes';
  import { formatClock, formatDps, formatElapsed, formatStat } from '../../styles/format';
  import { bestItem } from '../v3/loot-rank';
  import type { HomeVisuals } from '../visuals';
  import type { RecapState } from './recap';
  import type { RecapLoad } from './recap-reads';
  import './screens.css';

  let { recap, visuals, resolveSpriteUrl, onDismiss, status, reason, onRetry, message = null }: {
    recap: RecapState; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string; onDismiss: () => void;
    status: 'saving' | 'partial' | 'saved'; onRetry: () => void;
    reason?: Extract<RecapLoad, { status: 'partial' }>['reason'];
    message?: string | null;
  } = $props();
  const labels: Record<Outcome, string> = { cleared: 'Cleared', died: 'Died', left: 'Left early', unknown: 'Outcome not established' };
  const summary = $derived(recap.summary);
  const drop = $derived(summary.best?.drop);
  const item = $derived(drop ? bestItem(drop, visuals) : null);
  const bagVisual = $derived(objectVisual(visuals.objects, drop?.bag_type));
  const itemVisual = $derived(objectVisual(visuals.objects, item?.itemType));
</script>

<section class="v4-card recap-placeholder" aria-label="Recap">
  <header>
    <p class="eyebrow">{summary.kind === 'run' ? 'Run recap' : 'Realm visit recap'}</p>
    <p role="status">{status === 'saving' ? summary.kind === 'visit' ? 'Saving visit…' : 'Saving run…' : status === 'partial' ? 'Partial' : ''}</p>
    {#if status === 'partial' && message !== null}<p>{message}</p>{/if}
    <h2>{summary.title}</h2>
    <p>{labels[summary.outcome ?? 'unknown']}</p>
  </header>
  <dl class="recap-stats">
    <div><dt>Started</dt><dd>{formatClock(summary.startedAt) ?? '—'}</dd></div>
    <div><dt>Ended</dt><dd>{formatClock(summary.endedAt) ?? '—'}</dd></div>
    <div><dt>Duration</dt><dd>{summary.endedAt === null ? '—' : formatElapsed(summary.startedAt, summary.endedAt, true)}</dd></div>
    <div><dt>Your DPS · {summary.kind === 'run' ? 'Whole run' : 'Visit'}</dt><dd>{formatDps(summary.localDps)}</dd></div>
    <div><dt>Rank</dt><dd>{summary.rank === null ? '—' : `#${formatStat(summary.rank)}`}</dd></div>
    <div><dt>Observed players</dt><dd>{formatStat(summary.players)}</dd></div>
  </dl>
  <div class="recap-fights">
    <h3>Observed fights</h3>
    {#if summary.fights === null}<p>—</p>
    {:else if summary.fights.length === 0}<p>No fights observed</p>
    {:else}
      <ul>
        {#each summary.fights.slice(0, 12) as fight (fight.id)}
          <li><a href={routeHref({ kind: 'fight', id: fight.id })}>{objectVisual(visuals.objects, fight.target_type)?.name ?? 'Unknown target'}</a>
            {#if fight.kill && !fight.kill.provisional}<span>Killed</span>{/if}</li>
        {/each}
      </ul>
      {#if summary.fights.length > 12}<p>+{summary.fights.length - 12} more</p>{/if}
    {/if}
  </div>
  <div class="recap-loot">
    <h3>Best bag</h3>
    {#if drop}
      <div class="bag">
        <SpriteImage sprite={bagVisual?.sprite ?? null} sheetUrl={bagVisual?.sprite ? resolveSpriteUrl(bagVisual.sprite) : null} fit={40} alt={item ? bagVisual?.name ?? '—' : ''} />
        {#if item}<SpriteImage sprite={itemVisual?.sprite ?? null} sheetUrl={itemVisual?.sprite ? resolveSpriteUrl(itemVisual.sprite) : null} fit={40} alt="" />{/if}
        <span>{item ? itemVisual?.name ?? '—' : bagVisual?.name ?? '—'}</span>
      </div>
      {#if drop.source_estimated}<p>Source estimated</p>{/if}
      <p>{drop.contents_known ? 'Observed contents' : 'Contents not observed'}</p>
    {:else}<p>—</p>{/if}
  </div>
  <footer>
    <div class="actions">
      {#if status === 'partial' && reason !== 'unavailable'}<button type="button" onclick={onRetry}>Retry</button>{/if}
      {#if summary.kind === 'run' && summary.runId !== null}<a href={routeHref({ kind: 'run', id: summary.runId })}>Open run story ›</a>{/if}
      <button type="button" onclick={onDismiss}>Back</button>
    </div>
    <p>Available from Last recap until the next recap</p>
  </footer>
</section>

<style>
  .recap-placeholder { display: grid; gap: var(--space-6); padding: var(--space-6); }
  h2 { margin: var(--space-1) 0 var(--space-2); font-size: var(--text-hero); line-height: var(--leading-hero); font-weight: var(--weight-medium); overflow-wrap: anywhere; }
  p, dl, dd { margin: 0; }
  .eyebrow, dt, footer p, .recap-loot p { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .recap-stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--space-5); }
  dd { margin-top: var(--space-1); font-size: var(--text-title); line-height: var(--leading-title); font-variant-numeric: tabular-nums; }
  h3 { margin: 0 0 var(--space-2); font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  ul { margin: 0; padding-left: var(--space-5); }
  li + li { margin-top: var(--space-2); }
  li span { margin-left: var(--space-3); color: var(--text-muted); font-size: var(--text-meta); }
  .bag, .actions { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-3); }
  .bag { margin-bottom: var(--space-2); overflow-wrap: anywhere; }
  .actions { margin-bottom: var(--space-3); }
  a { color: var(--text-primary); text-decoration: none; text-underline-offset: 3px; }
  a:hover, a:focus-visible { text-decoration: underline; }
  button { font: inherit; font-size: var(--text-label); line-height: var(--leading-label); min-height: var(--target-min); border: 1px solid var(--border-strong);
    border-radius: var(--radius-control); padding: var(--space-2) var(--space-4); color: var(--text-primary); background: var(--surface-raised); }
  @container (max-width: 540px) { .recap-stats { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>

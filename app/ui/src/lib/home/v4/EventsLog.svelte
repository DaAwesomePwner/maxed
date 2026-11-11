<script lang="ts">
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import Tooltip from '../../components/Tooltip.svelte';
  import { routeHref } from '../../navigation/routes';
  import { formatDps, formatShare, formatStat } from '../../styles/format';
  import { clockTime } from '../nexus';
  import type { HomeVisuals } from '../visuals';
  import type { EventRow } from './realm-events';

  let { rows, visuals, resolveSpriteUrl }: {
    rows: EventRow[]; visuals: HomeVisuals; resolveSpriteUrl: (sprite: SpriteRef) => string;
  } = $props();
  const uid = $props.id();
  const kills = $derived(rows.filter(row => row.state !== 'live').map(row => {
    const bags = new Map<string, EventRow['bags'][number] & { count: number }>();
    for (const bag of row.bags) {
      const key = `${bag.bagType}:${bag.estimated}`, group = bags.get(key);
      if (group) group.count++;
      else bags.set(key, { ...bag, count: 1 });
    }
    return { ...row, bags: [...bags.values()] };
  }));
  const rank = (value: number | null) => value !== null && Number.isInteger(value) && value > 0 ? `#${value}` : '—';
</script>

<section class="v4-card events-log" aria-label="Events this visit">
  <h3>Events this visit <span class="event-count">{kills.length}</span></h3>
  <div class="event-list">
    {#each kills as row (row.key)}
      {@const visual = objectVisual(visuals.objects, row.targetType)}
      <div class="event-row" class:announced={row.state === 'announced'} data-event-id={row.key}>
        <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} fit={32} alt={row.name} />
        <div class="event-detail">
          <div class="event-title"><strong>{row.name}</strong><time>{clockTime(row.at) ?? '—'}</time></div>
          {#if row.state === 'announced'}<p>Announced · you weren't there</p>
          {:else}<p>{formatDps(row.dps)} DPS · {rank(row.rank)} of {formatStat(row.players)} · {formatShare(row.share)}</p>{/if}
          {#if row.bags.length}<div class="event-bags">
            {#each row.bags as bag (bag.dropId)}
              {@const visual = objectVisual(visuals.objects, bag.bagType)}
              {@const name = `${bag.count > 1 ? `${bag.count} × ` : ''}${visual?.name ?? 'Bag'}`}
              <Tooltip id={`${uid}-${bag.dropId}`} text={`${name} · ${row.name}${bag.estimated ? ' · Source estimated' : ''}`}>
                {#snippet trigger(describedby)}
                  <a href={routeHref({ kind: 'loot', view: 'feed' })} aria-label={`${name}${bag.estimated ? ' (estimated)' : ''}`}
                    aria-describedby={describedby} data-drop-id={bag.dropId}>
                    <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} fit={20} alt="" />
                    {#if bag.count > 1}<span class="bag-count">{bag.count}</span>{/if}
                  </a>
                {/snippet}
              </Tooltip>
            {/each}
          </div>{/if}
        </div>
      </div>
    {:else}<p class="empty">No events yet</p>{/each}
  </div>
</section>

<style>
  h3 { display: flex; align-items: center; gap: var(--space-2); }
  .event-count { color: var(--text-muted); font-weight: var(--weight-normal); font-variant-numeric: tabular-nums; }
  .event-list { display: flex; flex-direction: column; gap: var(--space-3); margin-top: var(--space-3); }
  .event-row { display: flex; align-items: start; gap: var(--space-2); min-width: 0; }
  .announced { opacity: .6; }
  .event-detail { flex: 1; min-width: 0; }
  .event-title { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--space-1) var(--space-2); font-size: var(--text-caption); line-height: var(--leading-caption); }
  strong { overflow-wrap: anywhere; font-weight: var(--weight-medium); }
  time { margin-left: auto; white-space: nowrap; color: var(--text-muted); font-size: var(--text-meta); font-variant-numeric: tabular-nums; }
  p { margin: var(--space-1) 0 0; color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta); font-variant-numeric: tabular-nums; }
  .empty { margin: 0; }
  .event-bags { display: flex; flex-wrap: wrap; gap: var(--space-1); margin-top: var(--space-2); }
  a { display: flex; gap: 2px; align-items: center; justify-content: center; min-width: 24px; min-height: 24px; border-radius: var(--radius-tag); color: var(--text-secondary); text-decoration: none; }
  a:hover { background: var(--surface-hover); }
  .bag-count { font-size: var(--text-meta); font-variant-numeric: tabular-nums; }
  a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
</style>

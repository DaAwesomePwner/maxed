<script lang="ts">
  import { rankedDrops } from '../../loot/ranking';
  import { onMount } from 'svelte';
  import type { DropView } from '../../bindings/DropView';
  import type { FightView } from '../../bindings/FightView';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import Tooltip from '../../components/Tooltip.svelte';
  import DebugNote from '../../debug/DebugNote.svelte';
  import { routeHref } from '../../navigation/routes';
  import { formatElapsed } from '../../styles/format';
  import { dungeonPosition } from '../dungeon';
  import type { RealmKillRow } from '../realm';
  import type { HomeVisuals } from '../visuals';
  import type { EventRow } from './realm-events';

  let { rows, kills, liveFight, drops, started, at, visuals, resolveSpriteUrl, partial }: {
    rows: EventRow[]; kills: RealmKillRow[]; liveFight: FightView | null; drops: DropView[];
    started: string | null; at: string; visuals: HomeVisuals; resolveSpriteUrl: (sprite: SpriteRef) => string; partial: boolean;
  } = $props();
  const uid = $props.id();
  let track: HTMLDivElement;
  let width = $state(800);
  onMount(() => {
    const measure = () => { if (track.clientWidth > 0) width = track.clientWidth; };
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(track);
    window.addEventListener('resize', measure);
    return () => { observer?.disconnect(); window.removeEventListener('resize', measure); };
  });
  const x = (time: string | null) => {
    const position = time !== null && started !== null ? dungeonPosition(time, started, at) : null;
    return position === null ? null : 12 + position / 100 * Math.max(0, width - 24);
  };
  const spans = $derived.by(() => {
    const lanes: number[] = [];
    const byKey = new Map(kills.map(kill => [kill.id, kill]));
    return rows.flatMap(row => {
      if (row.state === 'announced') return [];
      const kill = byKey.get(row.key), start = row.state === 'live' ? liveFight?.started : kill?.started;
      const from = x(start ?? null), until = x(row.at ?? at);
      if (from === null || until === null || until < from) return [];
      const size = Math.max(24, until - from), left = Math.min(from, Math.max(0, width - size));
      return [{ row, left, size, fightId: row.state === 'live' ? liveFight?.id : kill?.fightId }];
    }).sort((a, b) => a.left - b.left).map(span => {
      let lane = lanes.findIndex(end => end <= span.left);
      if (lane < 0) lane = lanes.length;
      lanes[lane] = span.left + span.size + 2;
      return { ...span, lane };
    });
  });
  const lanes = $derived(Math.max(1, ...spans.map(span => span.lane + 1)));
  function cluster<T>(points: { value: T; x: number }[]): { values: T[]; x: number }[] {
    const groups: { values: T[]; x: number }[] = [];
    for (const point of points.sort((a, b) => a.x - b.x)) {
      const last = groups.at(-1);
      if (last && point.x - last.x < 24) last.values.push(point.value);
      else groups.push({ values: [point.value], x: point.x });
    }
    return groups;
  }
  const bags = $derived(cluster(drops.flatMap(drop => {
    const position = x(drop.at); return position === null ? [] : [{ value: drop, x: position }];
  })));
  const announced = $derived(cluster(rows.filter(row => row.state === 'announced').flatMap(row => {
    const position = x(row.at); return position === null ? [] : [{ value: row, x: position }];
  })));
  const elapsed = (time: string | null) => started !== null && time !== null ? formatElapsed(started, time) : '—';
</script>

<section class="v4-card visit-timeline" aria-label="Visit timeline">
  <h3>Visit timeline</h3>
  <div class="track" bind:this={track}>
    <div class="point-row">
      {#each bags as group, index}
        {@const best = rankedDrops(group.values)[0]}
        {@const visual = objectVisual(visuals.objects, best.bag_type)}
        <span class="point" style:left={`${group.x - 12}px`}>
          <Tooltip id={`${uid}-bag-${index}`} text={group.values.map(drop => `${objectVisual(visuals.objects, drop.bag_type)?.name ?? 'Bag'} · ${elapsed(drop.at)}${drop.source_estimated ? ' · Source estimated' : ''}`).join('\n')}>
            {#snippet trigger(describedby)}
              <a class="bag-pip" data-drop-id={best.id} href={routeHref({ kind: 'loot', view: 'feed' })} aria-describedby={describedby}
                aria-label={`${group.values.length === 1 ? visual?.name ?? 'Bag' : `${group.values.length} bags`} · ${elapsed(best.at)}`}>
                {#if visual?.sprite}<SpriteImage sprite={visual.sprite} sheetUrl={resolveSpriteUrl(visual.sprite)} fit={16} alt="" />
                {:else}<span class="bag-dot" aria-hidden="true"></span>{/if}
                {#if group.values.length > 1}<span class="count">{group.values.length}</span>{/if}
              </a>
            {/snippet}
          </Tooltip>
        </span>
      {/each}
    </div>
    <div class="span-row" style:height={`${lanes * 24}px`}>
      {#each spans as span (span.row.key)}
        <a class="event-span" class:live={span.row.state === 'live'} style:left={`${span.left}px`} style:width={`${span.size}px`}
          style:top={`${span.lane * 24}px`} href={span.fightId ? routeHref({ kind: 'fight', id: span.fightId }) : undefined}
          aria-label={`${span.row.name} · ${span.row.state === 'live' ? 'Live' : elapsed(span.row.at)}`}
          title={`${span.row.name} · ${span.row.state === 'live' ? 'Live' : elapsed(span.row.at)}`}>{span.row.name}</a>
      {/each}
    </div>
    <div class="point-row">
      {#each announced as group, index}
        <span class="point" style:left={`${group.x - 12}px`}>
          <Tooltip id={`${uid}-announced-${index}`} text={group.values.map(row => `${row.name} · ${elapsed(row.at)} · Announced · you weren't there`).join('\n')}>
            {#snippet trigger(describedby)}
              <button type="button" aria-describedby={describedby} aria-label={`${group.values.map(row => row.name).join(', ')} · Announced`}>
                <span class="announced-diamond" aria-hidden="true"></span>
                {#if group.values.length > 1}<span class="count">{group.values.length}</span>{/if}
              </button>
            {/snippet}
          </Tooltip>
        </span>
      {/each}
    </div>
    {#if x(at) !== null}<span class="now" data-now aria-hidden="true"></span>{/if}
  </div>
  <div class="axis"><span>{started === null ? '—' : '0:00'}</span><span>Now: {elapsed(at)}</span></div>
  <DebugNote text={`${partial ? 'Partial visit timeline. ' : ''}Blank gaps do not imply inactivity; the axis includes time away.`} />
</section>

<style>
  .visit-timeline { padding: var(--space-3) var(--space-4); font-size: var(--text-meta); line-height: var(--leading-meta); }
  h3 { margin-bottom: var(--space-2); }
  .track, .point-row, .span-row { position: relative; min-width: 0; }
  .point-row { height: 24px; }
  .span-row::before { content: ''; position: absolute; inset: 50% 0 auto; height: 1px; background: var(--chart-grid); }
  .point { position: absolute; top: 0; width: 24px; }
  button, .bag-pip { display: flex; position: relative; justify-content: center; align-items: center; width: 24px; height: 24px;
    padding: 0; border: 0; background: transparent; color: var(--text-secondary); cursor: pointer; }
  .bag-dot { width: 8px; height: 8px; border: 1px solid var(--text-secondary); border-radius: var(--radius-pill); background: var(--surface-raised); }
  .announced-diamond { width: 8px; height: 8px; border: 1px solid var(--text-secondary); transform: rotate(45deg); background: transparent; }
  .event-span { position: absolute; box-sizing: border-box; height: 20px; padding: 0 var(--space-1); border: 1px solid var(--hairline-strong);
    border-radius: var(--radius-tag); background: var(--surface-raised); line-height: 18px;
    color: var(--text-secondary); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; text-decoration: none; }
  .event-span:hover { background: var(--surface-hover); color: var(--text-primary); }
  .event-span.live { border-color: var(--success); color: var(--text-primary); }
  .now { position: absolute; top: 0; bottom: 0; right: 11px; border-left: 1px solid var(--text-secondary); pointer-events: none; }
  .axis { display: flex; justify-content: space-between; margin-top: var(--space-1); color: var(--chart-label); font-variant-numeric: tabular-nums; }
  .count { position: absolute; top: -2px; right: -4px; padding: 0 2px; border-radius: var(--radius-tag); background: var(--surface-raised);
    font-size: var(--text-meta); line-height: 1; font-variant-numeric: tabular-nums; }
  a:focus-visible, button:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; z-index: 1; }
</style>

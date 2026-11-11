<script lang="ts">
  import { objectVisual } from '../../bridge/visual-index';
  import DebugNote from '../../debug/DebugNote.svelte';
  import { onMount } from 'svelte';
  import type { RunView } from '../../bindings/RunView';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import Tooltip from '../../components/Tooltip.svelte';
  import { spriteUrl } from '../../bridge/visuals';
  import { formatElapsed } from '../../styles/format';
  import { routeHref } from '../../navigation/routes';
  import { bagTone } from '../../styles/rarity';
  import { MARKER_MERGE_PX, dungeonClusters, dungeonPosition, markerSummary, type DungeonCluster, type DungeonMarker } from '../dungeon';
  import { emptyHomeVisuals, type HomeVisuals } from '../visuals';
  let { run, at, markers, partial, visuals = emptyHomeVisuals(), onselect, saved = false }: {
    onselect?: (marker: DungeonMarker) => void; saved?: boolean;
    run: RunView; at: string; markers: DungeonMarker[]; partial: boolean; visuals?: HomeVisuals;
  } = $props();
  const uid = $props.id();
  let trackWidth = $state(800);
  let track: HTMLDivElement;
  onMount(() => {
    const measure = () => { if (track.clientWidth > 0) trackWidth = track.clientWidth; };
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(track);
    window.addEventListener('resize', measure);
    return () => { observer?.disconnect(); window.removeEventListener('resize', measure); };
  });
  // Point targets are one merge width wide; anchors stay half a target inside the track.
  const target = MARKER_MERGE_PX;
  let end = $derived(run.ended ?? at);
  let clearPosition = $derived(saved && run.completed_at ? dungeonPosition(run.completed_at, run.started, end) : null);
  let width = $derived(Math.max(target, trackWidth));
  const x = (position: number) => target / 2 + position / 100 * (width - target);
  let placed = $derived(markers.map(marker => ({ marker, position: dungeonPosition(marker.at, run.started, end) })));
  let points = $derived(placed.flatMap(({ marker, position }) =>
    position !== null && marker.kind !== 'enemy' ? [{ marker, x: x(position) }] : []));
  let bags = $derived(dungeonClusters(points.filter(point => point.marker.kind === 'bag')));
  let events = $derived(dungeonClusters(points.filter(point => point.marker.kind === null)));
  // Boss spans keep their exact extent; overlapping fights take another lane.
  let spans = $derived.by(() => {
    const lanes: number[] = [];
    return placed.flatMap(({ marker, position }) => {
      if (position === null || marker.kind !== 'enemy') return [];
      const until = dungeonPosition(marker.until ?? end, run.started, end) ?? position;
      const size = Math.min(width, Math.max(target, x(until) - x(position)));
      const left = Math.min(x(position), width - size);
      let lane = lanes.findIndex(right => right <= left);
      if (lane < 0) lane = lanes.length;
      lanes[lane] = left + size + 2;
      return [{ marker, left, size, lane }];
    });
  });
  let spanLanes = $derived(Math.max(1, ...spans.map(span => span.lane + 1)));
  let outside = $derived(placed.filter(item => item.position === null).map(item => item.marker));
  let provisional = $derived(markers.filter(marker => marker.provisional).length);
  const visual = (type: number | null) => type === null ? null : objectVisual(visuals.objects, type) ?? null;
  const sheet = (type: number | null) => {
    const sprite = visual(type)?.sprite;
    return sprite ? spriteUrl(visuals.client_fingerprint, sprite) : null;
  };
  let context = $derived({ start: run.started, end, name: (type: number | null) => visual(type)?.name ?? null });
  function range(cluster: DungeonCluster) {
    const first = formatElapsed(run.started, cluster.markers[0].at);
    const last = formatElapsed(run.started, cluster.markers.at(-1)!.at);
    return first === last ? first : `${first}–${last}`;
  }
  function clusterLabel(cluster: DungeonCluster, noun: string) {
    return cluster.markers.length === 1 ? markerSummary(cluster.markers[0], context).title
      : `${cluster.markers.length} ${noun} · ${range(cluster)}`;
  }
  // A merged bag shows its highest-ranked bag; merged people events lead with a death.
  const rank = (marker: DungeonMarker) => marker.detail.type === 'bag' ? marker.detail.bagRank ?? -1 : -1;
  const bestBag = (cluster: DungeonCluster) => cluster.markers.reduce((best, marker) => rank(marker) > rank(best) ? marker : best);
  const leadEvent = (cluster: DungeonCluster) => cluster.markers.find(marker => marker.event === 'death')
    ?? cluster.markers.find(marker => marker.event === 'left') ?? cluster.markers[0];
</script>

{#snippet details(rows: DungeonMarker[])}
  {#each rows as marker (marker.id)}
    {@const summary = markerSummary(marker, context)}
    <span class="tip-entry" data-tip-marker={marker.id}><strong>{summary.title}</strong>{#each summary.lines as line}<span>{line}</span>{/each}</span>
  {/each}
{/snippet}

{#snippet icon(marker: DungeonMarker)}
  {@const sprite = visual(marker.objectType)?.sprite ?? null}
  {#if sprite}<SpriteImage {sprite} sheetUrl={sheet(marker.objectType)} fit={16} alt="" />
  {:else if marker.kind === 'bag'}<span class="bag-dot" data-bag-tone={bagTone(visual(marker.objectType)?.bag ?? null)}></span>
  {:else if marker.event === 'death'}<svg class="glyph death" viewBox="0 0 14 14"><path d="M3.5 12.5V6a3.5 3.5 0 0 1 7 0v6.5ZM2 12.5h10M7 5v4M5.5 6.5h3" /></svg>
  {:else if marker.event === 'left'}<svg class="glyph" viewBox="0 0 14 14"><path d="M8 2H3v10h5M6 7h6M10 5l2 2-2 2" /></svg>
  {:else}<span class="dot"></span>{/if}
{/snippet}

{#snippet point(cluster: DungeonCluster, index: number, row: 'bag' | 'event')}
  {@const lead = row === 'bag' ? bestBag(cluster) : leadEvent(cluster)}
  <span class="point" style:left={`${cluster.x - target / 2}px`}>
    <Tooltip id={`${uid}-${row}-${index}`}>
      {#snippet trigger(describedby)}
        <a class="marker" class:death={lead.event === 'death'} data-marker-id={cluster.id} data-marker-members={JSON.stringify(cluster.markers.map(marker => marker.id))} data-count={cluster.markers.length}
          href={routeHref({ kind: 'run', id: run.id })} aria-label={clusterLabel(cluster, row === 'bag' ? 'Bags' : 'Events')}
          aria-describedby={describedby} onclick={onselect ? event => { event.preventDefault(); onselect(lead); } : undefined}>
          <span class="icon" aria-hidden="true">{@render icon(lead)}</span>
          {#if cluster.markers.length > 1}<span class="count" aria-hidden="true">{cluster.markers.length}</span>{/if}
        </a>
      {/snippet}
      {#snippet content()}{@render details(cluster.markers)}{/snippet}
    </Tooltip>
  </span>
{/snippet}

<section class="timeline" aria-label="Timeline">
  <div class="heading">
    <h2>Timeline</h2>
    {#if !saved}<a href={routeHref({ kind: 'run', id: run.id })}>Open run</a>{/if}
  </div>
  <div class="track" bind:this={track} role="group" aria-label={`Duration 0:00–${formatElapsed(run.started, end)}`}>
    <div class="row">
      {#each bags as cluster, index (cluster.id)}{@render point(cluster, index, 'bag')}{/each}
    </div>
    <div class="row spans" style:height={`calc(${spanLanes} * var(--span-height))`}>
      {#each spans as span, index (span.marker.id)}
        {@const marker = span.marker}
        {@const summary = markerSummary(marker, context)}
        {@const sprite = visual(marker.objectType)?.sprite ?? null}
        <span class="span-slot" style:left={`${span.left}px`} style:top={`calc(${span.lane} * var(--span-height))`}>
          <Tooltip id={`${uid}-fight-${index}`}>
            {#snippet trigger(describedby)}
              <a class="fight" class:live={marker.detail.type === 'boss' && marker.detail.outcome === 'live'} data-marker-id={marker.id} data-marker-members={JSON.stringify([marker.id])}
                href={routeHref({ kind: 'run', id: run.id })} aria-label={summary.title} aria-describedby={describedby}
                style:width={`${span.size}px`} onclick={onselect ? event => { event.preventDefault(); onselect(marker); } : undefined}>
                {#if sprite}<span class="icon" aria-hidden="true"><SpriteImage {sprite} sheetUrl={sheet(marker.objectType)} fit={16} alt="" /></span>{/if}
                <span class="fight-label">{context.name(marker.objectType) ?? 'Boss'}</span>
              </a>
            {/snippet}
            {#snippet content()}{@render details([marker])}{/snippet}
          </Tooltip>
        </span>
      {/each}
    </div>
    <div class="row">
      {#each events as cluster, index (cluster.id)}{@render point(cluster, index, 'event')}{/each}
    </div>
    {#if clearPosition !== null && run.completed_at}<span class="saved-clear" role="img" aria-label={`Cleared at ${formatElapsed(run.started, run.completed_at)}`} style:left={`${x(clearPosition)}px`}></span>{/if}
    {#if run.ended == null}<span class="now" data-now aria-hidden="true"></span>{/if}
  </div>
  <div class="axis-notes">
    <span class="tick">0:00</span>
    <!-- A blank span has no event evidence; it does not establish inactivity. -->
    <DebugNote text={`${partial ? 'Partial timeline. ' : ''}Blank gaps do not imply inactivity.`} />
    <span class="tick">{run.ended == null ? (saved ? 'Observed' : 'Now') : 'End'}: {formatElapsed(run.started, end)}</span>
  </div>
  {#if !markers.length}<p>No events</p>{/if}
  {#if provisional}<DebugNote text={`${provisional} provisional ${provisional === 1 ? 'marker' : 'markers'}.`} />{/if}
  {#each outside as marker (marker.id)}
    <a data-marker-id={marker.id} data-marker-members={JSON.stringify([marker.id])} href={routeHref({ kind: 'run', id: run.id })} onclick={onselect ? event => { event.preventDefault(); onselect(marker); } : undefined}>{markerSummary(marker, context).title}<DebugNote inline text="Outside available duration axis" /></a>
  {/each}
</section>

<style>
  .saved-clear { position: absolute; top: 0; bottom: 0; width: 1px; background: var(--success); pointer-events: none; }
  .timeline { --span-height: 22px; padding: var(--space-2) var(--space-3) var(--space-3); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); font-size: var(--text-meta); line-height: 16px; color: var(--text-secondary); min-width: 0; }
  h2 { margin: 0; font-size: var(--text-meta); font-weight: var(--weight-medium); color: var(--text-muted); }
  .heading, .axis-notes { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; justify-content: space-between; }
  .heading { min-height: 20px; margin-bottom: var(--space-1); }
  .axis-notes { margin-top: var(--space-1); min-height: 16px; color: var(--chart-label); }
  a { display: inline-flex; align-items: center; min-height: 20px; color: var(--text-secondary); text-decoration: none; }
  .heading a:hover, .heading a:focus-visible, .timeline > a:hover, .timeline > a:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  .track { position: relative; min-width: 0; min-height: 58px; }
  .row { position: relative; height: 18px; }
  /* A hairline baseline under the boss spans. */
  .spans::before { content: ''; position: absolute; inset: 50% 0 auto; border-top: 1px solid var(--hairline); pointer-events: none; }
  .point, .span-slot { position: absolute; top: 0; display: flex; }
  .point { width: 24px; height: 100%; }
  .marker { position: relative; box-sizing: border-box; justify-content: center; width: 24px; height: 18px; min-height: 18px; text-decoration: none; color: var(--text-secondary); border-radius: var(--radius-tag); }
  /* Keep the compact visual row while providing a 24px-tall pointer target. */
  .marker::before { content: ''; position: absolute; inset: -3px 0; }
  .marker:hover, .marker:focus-visible, .fight:hover, .fight:focus-visible { background: var(--surface-hover); z-index: 1; }
  .icon { display: inline-flex; align-items: center; justify-content: center; }
  .count { position: absolute; top: 0; right: -6px; min-width: 1em; padding: 0 2px; border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); background: var(--surface-raised); color: var(--text-primary); font-size: var(--text-meta); line-height: 1.1; font-weight: var(--weight-medium); text-align: center; font-variant-numeric: tabular-nums; }
  .glyph { width: 14px; height: 14px; fill: none; stroke: var(--warning); stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .glyph.death, .death { stroke: var(--danger); color: var(--danger); }
  .dot { width: var(--space-2); height: var(--space-2); border-radius: var(--radius-pill); background: var(--text-secondary); }
  .bag-dot { display: block; width: var(--space-3); height: var(--space-3); border-radius: var(--radius-pill); background: var(--bag-color, var(--bag-unknown)); border: 1px solid var(--text-primary); box-shadow: 0 0 0 2px var(--surface-panel); }
  /* Boss spans are neutral steps; only a live fight takes the live (success) colour. */
  .fight { box-sizing: border-box; gap: var(--space-1); height: calc(var(--span-height) - 2px); min-height: 0; margin-top: 1px; padding-inline: 2px var(--space-1); overflow: hidden; color: var(--text-secondary); text-decoration: none; background: var(--surface-raised); border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); }
  .fight:hover, .fight:focus-visible { color: var(--text-primary); }
  .fight.live { color: var(--success); border-color: color-mix(in srgb, var(--success) 50%, transparent); }
  .fight-label { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .now { position: absolute; top: var(--space-1); bottom: var(--space-1); right: calc(24px / 2 - 1px); border-left: 2px solid var(--success); pointer-events: none; }
  .tip-entry { display: grid; }
  .tip-entry + .tip-entry { margin-top: var(--space-1); padding-top: var(--space-1); border-top: 1px solid var(--hairline); }
  .tip-entry span { color: var(--text-secondary); }
  .timeline :global(.tooltip:has(.tip-entry + .tip-entry)) { max-height: 18rem; overflow-y: auto; }
  .tick { font-variant-numeric: tabular-nums; white-space: nowrap; }
  p { color: var(--text-secondary); margin: 0; }
</style>

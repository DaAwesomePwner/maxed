<script lang="ts">
  import { objectVisual } from '../bridge/visual-index';
  import DebugNote from '../debug/DebugNote.svelte';
  import { debugMode } from '../debug/debug';
  import SpriteImage from '../components/SpriteImage.svelte';
  import Tooltip from '../components/Tooltip.svelte';
  import LootContents from './LootContents.svelte';
  import { spriteUrl } from '../bridge/visuals';
  import { emptyHomeVisuals, type HomeVisuals } from './visuals';
  import type { HomeRunDetail } from '../bindings/HomeRunDetail';
  import type { RunView } from '../bindings/RunView';
  import { routeHref } from '../navigation/routes';
  import { formatShare } from '../styles/format';
  import { coverageLabels, evidenceLabel, reasonLabel } from './evidence';
  import { observedTime, recentRunRows, RECENT_PAGE_SIZE, runResult } from './nexus';
  let { runs, details, visuals = emptyHomeVisuals() }: { runs: RunView[]; details: HomeRunDetail[]; visuals?: HomeVisuals } = $props();
  const id = $props.id();
  let page = $state(0);
  let rows = $derived(recentRunRows(runs, details));
  let pages = $derived(Math.max(1, Math.ceil(rows.length / RECENT_PAGE_SIZE)));
  let current = $derived(Math.min(page, pages - 1));
  let visible = $derived(rows.slice(current * RECENT_PAGE_SIZE, (current + 1) * RECENT_PAGE_SIZE));
</script>
<section class="recent-runs" aria-labelledby={`${id}-title`}>
  <header><h2 id={`${id}-title`}>Recent runs</h2><a href={routeHref({ kind: 'runs' })}>All runs ›</a>
    {#if pages > 1}<nav aria-label="Run Pages"><button aria-label="Previous Runs" aria-disabled={current === 0} onclick={() => { if (current > 0) page = current - 1; }}>‹</button><span>{current + 1}/{pages}</span><button aria-label="Next Runs" aria-disabled={current === pages - 1} onclick={() => { if (current + 1 < pages) page = current + 1; }}>›</button></nav>{/if}
  </header>
  {#if rows.length}
    <div class="table" aria-label="Recent runs">
      <div class="row labels" aria-hidden="true"><span>Dungeon</span><span>Result</span><span>Time</span><span>Share</span><span>Bag</span></div>
      {#each visible as { run, detail, bag, preview } (run.id)}
        {@const selected = bag ?? preview[0] ?? null}
        {@const visual = objectVisual(visuals.objects, selected?.bag_type)}
        {@const result = runResult(run)}
        <a class="row run" data-row-id={run.id} href={routeHref({ kind: 'run', id: run.id })}>
          <span class="name" title={run.dungeon}>{run.dungeon}</span>
          <span class="result" class:died={result.includes('Died')} class:cleared={!result.includes('Died') && result.includes('Cleared')}>{result}</span>
          <span class="number"><span class="sr-only">Time </span>{observedTime(run.started, run.completed_at ?? run.ended)}</span>
          <span class="number"><span class="sr-only">Share </span>{formatShare(detail?.local_share ?? null)}</span>
          <span class="bag">
            {#if selected}<Tooltip id={`${id}-${run.id}-bag`}>
              {#snippet trigger(tooltipId)}<span aria-describedby={tooltipId}><SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={24} alt={visual?.name ?? `Bag ${selected.bag_type}`} /></span>{/snippet}
              {#snippet content()}<LootContents drop={selected} {visuals} textOnly />{/snippet}
            </Tooltip>{:else}<span aria-label={detail ? 'No Bags' : 'Bag unavailable'}>—</span>{/if}
          </span>
        </a>
        {#if $debugMode}<div class="row-debug">
          {#if !run.complete_capture}<DebugNote text="Partial observations" />{/if}
          <DebugNote text={[...new Set([...run.issues, ...(detail?.issues ?? [])])].map(issue => coverageLabels[issue]).concat(run.evidence.map(item => evidenceLabel(item.evidence))).join('; ')} />
          <DebugNote>{run.completed_at !== null ? 'Clear' : 'Elapsed'} · Elapsed: {observedTime(run.started, run.ended)}</DebugNote>
          {#if run.died_at !== null}<a data-row-id={run.id} href={routeHref({ kind: 'run', id: run.id, point: 'death' })}>View Death</a>{/if}
          {#if detail?.share_partial}<DebugNote text="Partial share" />{/if}
          {#if !bag && preview.length}<DebugNote text="Rank unavailable; unranked preview" />{/if}
          {#each preview as drop}<DebugNote text={drop.capture_scope === 'extended' ? 'Extended Scope' : 'RealmShark Scope'} />{#if drop.source_estimated}<DebugNote>Source estimated: {reasonLabel(drop.source_reason)}</DebugNote>{/if}{#each drop.issues as issue}<DebugNote text={coverageLabels[issue]} />{/each}{/each}
          {#if detail?.loot_preview_limited}<DebugNote text="Limited preview" />{/if}
        </div>{/if}
      {/each}
    </div>
  {:else}<p>No Runs</p>{/if}
  <DebugNote text="Up to 10 recent runs; share observed. Best ranked bag in the retained recent preview when available; otherwise unranked previews." />
  {#if pages > 1}<p class="sr-only" role="status">Runs {current * RECENT_PAGE_SIZE + 1}–{Math.min((current + 1) * RECENT_PAGE_SIZE, rows.length)} / {rows.length}</p>{/if}
</section>
<style>
  .recent-runs { min-width: 0; padding: 10px 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); }
  .recent-runs header, .recent-runs nav { display: flex; align-items: center; gap: 6px; }
  .recent-runs header { margin-bottom: 8px; }
  .recent-runs header > a { margin-left: auto; }
  .recent-runs h2 { margin: 0; font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-secondary); }
  .recent-runs .row { display: grid; grid-template-columns: minmax(0, 1fr) 58px 46px 42px 28px; gap: 8px; align-items: center; min-height: 30px; padding: 0 6px; border-radius: 5px; font-size: 12px; }
  .recent-runs .labels { min-height: 22px; font-size: 10px; text-transform: uppercase; color: var(--text-secondary); }
  .recent-runs .run { color: var(--text-primary); text-decoration: none; }
  .recent-runs .run:hover, .recent-runs .run:focus-visible { background: var(--surface-hover); }
  .recent-runs .name { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .recent-runs .number { text-align: right; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .recent-runs .result { color: var(--text-secondary); font-size: 11px; line-height: 1.2; }
  .recent-runs .cleared { color: var(--success); }
  .recent-runs .died { color: var(--danger); }
  .recent-runs .bag { display: inline-flex; justify-content: center; }
  .recent-runs nav { font-size: 11px; }
  .recent-runs button { min-width: 24px; min-height: 24px; font: inherit; color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border); border-radius: 4px; }
  .recent-runs button[aria-disabled="true"] { color: var(--text-muted); }
  .recent-runs header a { font-size: 12px; color: var(--violet-text); }
  .recent-runs .row-debug { font-size: 11px; overflow-wrap: anywhere; }
  .recent-runs p { margin: 0; color: var(--text-secondary); }
  @media (max-width: 900px) {
    .recent-runs .row { gap: 4px; padding-inline: 2px; }
    .recent-runs { padding: 8px; }
  }
</style>

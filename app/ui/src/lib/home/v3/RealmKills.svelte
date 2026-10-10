<script lang="ts">
  import { objectVisual } from '../../bridge/visual-index';
  import { onMount, tick } from 'svelte';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import type { RealmKillRow } from '../realm';
  import type { HomeLiveWindow } from '../v2/contracts';
  import type { HomeVisuals } from '../visuals';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { formatElapsed, formatStat, formatShare } from '../../styles/format';
  import { clockTime } from '../nexus';
  import { knownNumber } from '../evidence';
  let { rows, window, visuals, resolveSpriteUrl }: {
    rows: RealmKillRow[]; window: HomeLiveWindow; visuals: HomeVisuals;
    resolveSpriteUrl: (s: SpriteRef) => string | null;
  } = $props();
  let host: HTMLDivElement;
  let button: HTMLButtonElement | undefined = $state();
  let height = $state(140);
  let expanded = $state(false);
  const rowHeight = 50, rowGap = 4, footerHeight = 24;
  const slotsFor = (available: number) => Math.max(0, Math.floor((available + rowGap) / (rowHeight + rowGap)));
  // Measure the whole list, including its optional footer, to avoid reserving
  // footer space when all rows fit without it. CSS uses the same dimensions.
  let capacity = $derived.by(() => {
    const slots = slotsFor(height);
    return rows.length <= slots ? slots : slotsFor(height - footerHeight - rowGap);
  });
  let shown = $derived(expanded ? rows : rows.slice(0, capacity));
  let hidden = $derived(Math.max(0, rows.length - shown.length));
  onMount(() => {
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(entries => {
      const next = entries[0]?.contentRect.height;
      if (next !== undefined && next >= 0 && !expanded) height = next;
    });
    observer?.observe(host);
    return () => observer?.disconnect();
  });
  const duration = (row: RealmKillRow) => row.started ? formatElapsed(row.started, row.ended ?? row.at, true) : '—';
  const detail = (row: RealmKillRow) => `${clockTime(row.at) ?? '—'} · ${row.evidence === 'realm_announcement'
    ? `last hit ${row.killer ?? '—'}` : `${duration(row)} fight`}`;
  const share = (row: RealmKillRow) => row.self?.share != null && Number.isFinite(row.self.share)
    && row.self.share >= 0 && row.self.share <= 1 ? row.self.share * 100 : 0;
  async function toggle() { expanded = !expanded; await tick(); button?.focus(); }
</script>

<div class="realm-kills" class:expanded data-visit={window.visitKey} bind:this={host}
  style:--kill-row-height={`${rowHeight}px`} style:--kill-gap={`${rowGap}px`} style:--kill-footer-height={`${footerHeight}px`}>
  <div class="kill-space">
    {#each shown as row (row.id)}
      {@const visual = objectVisual(visuals.objects, row.targetType)}
      <div class="kill-row" data-kill-id={row.id}>
        <SpriteImage sprite={visual?.sprite ?? null} fit={40}
          sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} alt={visual?.name ?? 'Boss'} />
        <div class="kill-name"><b title={visual?.name ?? row.bossKey ?? 'Boss'}>{visual?.name ?? row.bossKey?.replaceAll('_', ' ') ?? 'Boss'}</b>
          <span title={detail(row)}>{detail(row)}</span></div>
        {#if row.self && row.evidence !== 'realm_announcement'}
          {@const rank = knownNumber(row.self.rank)}
          <div class="kill-stats"><span><b>{#if rank !== null}{#if Number.isInteger(rank) && rank >= 1 && rank <= 3}<i class="medal" data-rank={rank} aria-hidden="true"></i>{/if}<span class="sr-only">{'Rank '}</span><span aria-hidden="true">#</span>{formatStat(rank)}{:else}—{/if}</b> · {formatShare(row.self.share)}</span>
            <span>{formatStat(knownNumber(row.self.damage))} damage</span><span class="share-track"><i class="share-fill" style:width={`${share(row)}%`}></i></span></div>
        {/if}
      </div>
    {:else}{#if !rows.length}<p>No Kills</p>{/if}{/each}
  </div>
  {#if hidden > 0 || expanded}<button type="button" bind:this={button} onclick={toggle}
    aria-expanded={expanded}>{expanded ? 'Show fewer kills' : `+${hidden} more ${hidden === 1 ? 'kill' : 'kills'} ›`}</button>{/if}
</div>

<style>
  .realm-kills { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: var(--kill-gap); }
  .kill-space { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: var(--kill-gap); }
  .kill-row { box-sizing: border-box; flex: none; height: var(--kill-row-height); display: grid;
    grid-template-columns: 44px minmax(0, 1fr) 128px; gap: 10px; align-items: center; padding: 4px 10px 4px 6px;
    border: 1px solid var(--border); border-radius: 7px; background: var(--surface-raised); }
  .kill-name { min-width: 0; display: grid; gap: 3px; line-height: 1.25; }
  .kill-name b { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 14px; }
  .kill-name > span { color: var(--text-muted); font-size: 11px; font-variant-numeric: tabular-nums;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .kill-stats { display: grid; gap: 2px; justify-items: end; font-size: 11px; line-height: 1.2; color: var(--text-muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .kill-stats b { color: var(--gold); }
  .share-track { width: 100%; height: 5px; overflow: hidden; border-radius: 3px; background: var(--surface-panel); }
  .share-fill { display: block; height: 100%; background: var(--gold); }
  button { flex: none; height: var(--kill-footer-height); border: 0; padding: 0; background: transparent; text-align: left;
    color: var(--violet-text); font: inherit; cursor: pointer; }
  .realm-kills.expanded .kill-space { flex: none; }
  p { margin: 0; color: var(--text-muted); }
  .medal { display: inline-block; width: 6px; height: 6px; margin-right: 5px; background: var(--gold);
    box-shadow: 0 -2px #a06b1b, 0 2px #a06b1b, -2px 0 #a06b1b, 2px 0 #a06b1b; image-rendering: pixelated; }
  .medal[data-rank='2'] { background: #d4d8e2; } .medal[data-rank='3'] { background: #c58a5b; }
</style>

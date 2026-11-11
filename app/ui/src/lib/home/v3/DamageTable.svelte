<script lang="ts">
  import DpsLabel from '../../runs/DpsLabel.svelte';
  import { dpsMetric, type DpsScope } from '../../runs/dps-metric';
  import { objectVisual } from '../../bridge/visual-index';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { formatDps, formatStat, formatShare } from '../../styles/format';
  import type { HomeVisuals } from '../visuals';
  import type { DamageRow } from './types';
  let { basis, rows, total, otherDamage = null, visuals, resolveSpriteUrl, moreHref = null, scroll = false, rowHref = null }: {
    basis: DpsScope; rows: Array<DamageRow | { gap: number }>; total: number; otherDamage?: number | null;
    visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string | null; moreHref?: string | null; scroll?: boolean;
    rowHref?: ((row: DamageRow) => string | null) | null;
  } = $props();
  const known = (n: number | null) => n !== null && Number.isFinite(n) && n >= 0 ? n : null;
  let players = $derived(rows.filter((row): row is DamageRow => !('gap' in row)));
  let top = $derived(Math.max(0, ...players.map(row => known(row.damage) ?? 0)));
  let hidden = $derived(Math.max(0, (Number.isSafeInteger(total) ? total : players.length) - players.length));
  const width = (damage: number) => top > 0 ? Math.min(100, (known(damage) ?? 0) / top * 100) : 0;
</script>

<div class="dps-note"><DpsLabel {basis} /></div>
<div class="damage-table" class:scroll {...scroll ? { tabindex: 0, role: 'region', 'aria-label': 'All player damage' } : {}}>
  <table aria-label="Player Damage">
    <colgroup><col style="width: 2.5rem" /><col style="width: 2rem" /><col style="width: 25%" />
      <col /><col style="width: 4.5rem" /><col style="width: 3.75rem" /></colgroup>
    <thead><tr><th scope="col">#</th><th scope="col"><span class="sr-only">Class</span></th>
      <th scope="col">Player</th><th scope="col">Damage</th><th scope="col">{dpsMetric(basis).label}</th><th scope="col">Share</th></tr></thead>
    <tbody>
      {#each rows as row}
        {#if 'gap' in row}
          <tr class="gap"><td colspan="6">··· {row.gap} {row.gap === 1 ? 'player' : 'players'}</td></tr>
        {:else}
          {@const visual = objectVisual(visuals.objects, row.class_id)}
          {@const href = rowHref?.(row) ?? null}
          <tr class="damage-row" class:self={row.is_local} class:linked={!!href}>
            <td>{row.rank !== null && Number.isInteger(row.rank) && row.rank > 0 ? formatStat(row.rank) : '—'}</td>
            <td><SpriteImage sprite={visual?.sprite ?? null} fit={24}
              sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} alt={visual?.name ?? 'Class'} /></td>
            <th scope="row" title={row.name ?? 'Player'}>
              {#if href}<a href={href}><span class="name">{row.name ?? 'Player'}</span><span aria-hidden="true">›</span></a>
              {:else}<span class="name">{row.name ?? 'Player'}</span>{/if}
              {#if row.is_local}<span class="sr-only">You</span>{/if}</th>
            <td><span class="amount"><span class="track"><span class="damage-fill" style:width={`${width(row.damage)}%`}></span></span>
              <b>{formatStat(known(row.damage))}</b></span></td>
            <td>{formatDps(known(row.dps))}</td><td>{formatShare(row.share)}</td>
          </tr>
        {/if}
      {:else}<tr><td colspan="6">No players</td></tr>{/each}
      {#if (known(otherDamage) ?? 0) > 0}
        <tr class="other" data-unattributed><th scope="row" colspan="3">Other damage</th>
          <td><span class="amount"><span class="track"><span class="damage-fill" style:width={`${width(otherDamage!)}%`}></span></span>
            <b>{formatStat(otherDamage)}</b></span></td><td>—</td><td>—</td></tr>
      {/if}
    </tbody>
  </table>
  {#if hidden > 0 && moreHref}<a class="more" href={moreHref}>+{hidden} more players ›</a>{/if}
</div>

<style>
  .dps-note { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); margin-bottom: var(--space-1); }
  .damage-table { min-width: 0; font-size: var(--text-caption); line-height: var(--leading-label); font-variant-numeric: tabular-nums; }
  .scroll { max-height: 320px; overflow: auto; }
  .scroll thead th { position: sticky; top: 0; z-index: 1; background: var(--surface-panel); }
  .scroll:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  table { width: 100%; table-layout: fixed; border-collapse: collapse; }
  th, td { height: 32px; padding: 0 var(--space-1); text-align: right; white-space: nowrap; }
  thead th { height: 24px; font-size: var(--text-meta); color: var(--text-muted); font-weight: var(--weight-medium); border-bottom: 1px solid var(--hairline-strong); }
  th:nth-child(3), tbody th { text-align: left; }
  tbody th { font-weight: var(--weight-normal); }
  tbody tr + tr { border-top: 1px solid var(--hairline); }
  .name { display: block; overflow: hidden; text-overflow: ellipsis; font-weight: var(--weight-medium); }
  th a { display: flex; align-items: center; gap: var(--space-1); min-height: 32px; color: inherit; text-decoration: none; }
  th a > [aria-hidden="true"] { color: var(--text-muted); }
  th a .name { flex: 1; min-width: 0; }
  th a:hover .name, th a:focus-visible .name { text-decoration: underline; text-underline-offset: 3px; }
  th a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 1px; }
  .linked:hover, .linked:focus-within { background: var(--surface-hover); }
  /* Damage: a thin track, with the figure in its own right-aligned slot. */
  .amount { display: flex; align-items: center; gap: var(--space-2); }
  .track { position: relative; flex: 1; min-width: 0; height: 6px; border-radius: 2px; background: var(--bar-track); overflow: hidden; }
  .damage-fill { position: absolute; inset: 0 auto 0 0; background: var(--bar-party); border-radius: inherit; }
  /* Sized in ch so an eight-digit total (12,345,678) fits; larger figures grow the slot, never overlap DPS. */
  .amount b { flex: none; min-width: 10ch; text-align: right; font-weight: var(--weight-normal); color: var(--text-primary); }
  .self, .linked.self:hover, .linked.self:focus-within { background: var(--accent-soft); }
  .self th { color: var(--gold); } .self .damage-fill { background: var(--bar-you); }
  .other { color: var(--text-muted); } .other .damage-fill { background: var(--bar-party-muted); } .other .amount b { color: var(--text-muted); }
  .gap td { height: 24px; text-align: left; padding-left: 4.5rem; color: var(--text-muted); }
  .more { display: inline-flex; align-items: center; min-height: 24px; color: var(--text-secondary); padding: var(--space-1) 0 0 2.5rem; text-decoration: none; }
  .more:hover, .more:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  @media (max-width: 950px) { .damage-table { font-size: var(--text-meta); } }
</style>

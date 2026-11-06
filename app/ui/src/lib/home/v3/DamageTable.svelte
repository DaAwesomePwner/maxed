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
            <td><span class="amount"><span class="damage-fill" style:width={`${width(row.damage)}%`}></span>
              <b>{formatStat(known(row.damage))}</b></span></td>
            <td>{formatDps(known(row.dps))}</td><td>{formatShare(row.share)}</td>
          </tr>
        {/if}
      {:else}<tr><td colspan="6">No Players</td></tr>{/each}
      {#if (known(otherDamage) ?? 0) > 0}
        <tr class="other" data-unattributed><th scope="row" colspan="3">Other Damage</th>
          <td><span class="amount"><span class="damage-fill" style:width={`${width(otherDamage!)}%`}></span>
            <b>{formatStat(otherDamage)}</b></span></td><td>—</td><td>—</td></tr>
      {/if}
    </tbody>
  </table>
  {#if hidden > 0 && moreHref}<a class="more" href={moreHref}>+{hidden} more players ›</a>{/if}
</div>

<style>
  .dps-note { color: var(--text-muted); font-size: 11px; margin-bottom: 4px; }
  .damage-table { min-width: 0; font-size: 12px; font-variant-numeric: tabular-nums; }
  .scroll { max-height: 320px; overflow: auto; }
  .scroll thead th { position: sticky; top: 0; z-index: 1; background: var(--surface-panel); }
  .scroll:focus-visible { outline: 2px solid var(--gold); outline-offset: -2px; }
  table { width: 100%; table-layout: fixed; border-collapse: collapse; }
  th, td { height: 28px; padding: 2px 3px; text-align: right; white-space: nowrap; }
  thead th { height: 22px; font-size: 10px; color: var(--text-muted); font-weight: 600; }
  th:nth-child(3), tbody th { text-align: left; }
  tbody tr + tr { border-top: 1px solid var(--border); }
  .name { display: block; overflow: hidden; text-overflow: ellipsis; font-weight: 500; }
  th a { display: flex; align-items: center; gap: 4px; min-height: 28px; color: inherit; text-decoration: none; }
  th a .name { flex: 1; min-width: 0; }
  th a:focus-visible { outline: 2px solid var(--gold); outline-offset: 1px; }
  .linked:hover, .linked:focus-within { background: color-mix(in srgb, var(--violet) 10%, transparent); }
  .linked.self:hover, .linked.self:focus-within { background: color-mix(in srgb, var(--gold) 12%, transparent); }
  .amount { position: relative; display: block; height: 18px; background: var(--surface-inset, #0c0b11); border-radius: 3px; }
  .damage-fill { position: absolute; inset: 0 auto 0 0; background: var(--violet); opacity: .7; border-radius: inherit; }
  .amount b { position: absolute; inset: 0 4px 0 0; line-height: 18px; font-size: 11px; color: white;
    text-shadow: 1px 0 #000, -1px 0 #000, 0 1px #000, 0 -1px #000; font-weight: 600; }
  .self { background: color-mix(in srgb, var(--gold) 7%, transparent); }
  .self th { color: var(--gold); } .self .damage-fill { background: var(--gold); }
  .other { color: var(--text-muted); } .other .damage-fill { background: var(--border-strong); }
  .gap td { height: 20px; text-align: left; padding-left: 4.5rem; color: var(--text-muted); }
  .more { display: block; min-height: 24px; color: var(--violet-text); padding: 4px 0 0 2.5rem; }
  @media (max-width: 950px) { .damage-table { font-size: 11px; } }
</style>

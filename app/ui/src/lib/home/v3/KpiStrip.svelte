<script lang="ts">
  import type { KpiCell } from './types';
  import Tooltip from '../../components/Tooltip.svelte';
  let { cells, valueTitles = false }: { cells: KpiCell[]; valueTitles?: boolean } = $props();
  const id = $props.id();
  const progress = (value: number | null | undefined) => value != null && Number.isFinite(value) ? Math.max(0, Math.min(100, value)) : null;
</script>
<dl class="kpi-strip" style:grid-template-columns={`repeat(${Math.max(1, cells.length)}, minmax(0, 1fr))`}>
  {#each cells as cell (cell.key)}
    {@const fill = progress(cell.progress)}
    <div data-kpi={cell.key} data-tone={cell.tone ?? 'plain'}>
      <dt>{#if cell.tooltip}<Tooltip id={`${id}-${cell.key}`} text={cell.tooltip}>
        {#snippet trigger(tooltipId)}<button type="button" class="tooltip-trigger" aria-describedby={tooltipId}>{cell.label}</button>{/snippet}
      </Tooltip>{:else}{cell.label}{/if}</dt>
      <dd>
        <span class="value" title={valueTitles ? cell.value : undefined}>{cell.value}{#if cell.of}<small> / {cell.of}</small>{/if}</span>
        <span class="sub" data-sub-tone={cell.subTone ?? 'plain'} title={cell.sub ?? undefined}>{cell.sub ?? '\u00a0'}</span>
        {#if fill !== null}<span class="progress" role="progressbar" aria-label={cell.label} aria-valuemin="0" aria-valuemax="100" aria-valuenow={fill}><i style:width={`${fill}%`}></i></span>{/if}
      </dd>
    </div>
  {/each}
</dl>
<style>
  .kpi-strip { margin: 0; display: grid; background: var(--surface-panel); border: 1px solid var(--border); border-radius: 8px; min-width: 0; }
  .kpi-strip > div { min-width: 0; padding: 9px 14px 10px; position: relative; }
  .kpi-strip > div + div { border-left: 1px solid var(--border); }
  .kpi-strip dt { font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-secondary); }
  .kpi-strip .tooltip-trigger { padding: 0; border: 0; background: transparent; color: inherit; font: inherit; letter-spacing: inherit; text-transform: inherit; cursor: help; }
  .kpi-strip dd { margin: 0; }
  .kpi-strip .value { display: block; font-size: 24px; line-height: 1.15; font-weight: 700; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .kpi-strip small { font-size: 12px; color: var(--text-secondary); }
  .kpi-strip .sub { display: block; min-height: 16px; font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--text-secondary); font-variant-numeric: tabular-nums; }
  .kpi-strip [data-tone="fame"] .value { color: var(--gold); }
  .kpi-strip [data-tone="bad"] .value { color: var(--danger); }
  .kpi-strip .sub[data-sub-tone="good"] { color: var(--success); }
  .kpi-strip .sub[data-sub-tone="bad"] { color: var(--danger); }
  .kpi-strip .progress { display: block; height: 3px; background: var(--surface-raised); margin-top: 3px; overflow: hidden; }
  .kpi-strip .progress i { display: block; height: 100%; background: var(--gold); }
  @media (max-width: 900px) {
    .kpi-strip > div { padding-inline: 8px; }
    .kpi-strip .value { font-size: 22px; }
  }
</style>

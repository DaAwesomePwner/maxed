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
  .kpi-strip { margin: 0; display: grid; background: var(--surface-panel); border: 1px solid var(--hairline); border-radius: var(--radius-card); min-width: 0; }
  .kpi-strip > div { min-width: 0; padding: var(--space-3) var(--space-4); position: relative; }
  .kpi-strip > div + div { border-left: 1px solid var(--hairline); }
  .kpi-strip dt { font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-normal); color: var(--text-muted); }
  .kpi-strip .tooltip-trigger { padding: 0; border: 0; background: transparent; color: inherit; font: inherit; cursor: help; }
  .kpi-strip dd { margin: 0; }
  .kpi-strip .value { display: block; margin-top: var(--space-1); font-size: var(--text-hero); line-height: var(--leading-hero); font-weight: var(--weight-medium); white-space: nowrap; font-variant-numeric: tabular-nums; color: var(--text-primary); }
  .kpi-strip small { font-size: var(--text-meta); font-weight: var(--weight-normal); color: var(--text-muted); }
  .kpi-strip .sub { display: block; min-height: 16px; font-size: var(--text-meta); line-height: var(--leading-meta); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--text-muted); font-variant-numeric: tabular-nums; }
  .kpi-strip [data-tone="fame"] .value { color: var(--gold); }
  .kpi-strip [data-tone="bad"] .value { color: var(--danger); }
  .kpi-strip .sub[data-sub-tone="good"] { color: var(--success); }
  .kpi-strip .sub[data-sub-tone="bad"] { color: var(--danger); }
  .kpi-strip .progress { display: block; height: 4px; margin-top: var(--space-1); border-radius: 2px; background: var(--bar-track); overflow: hidden; }
  .kpi-strip .progress i { display: block; height: 100%; background: var(--bar-you); }
  @media (max-width: 900px) {
    .kpi-strip > div { padding-inline: var(--space-2); }
    .kpi-strip .value { font-size: var(--text-title); line-height: var(--leading-title); }
  }
</style>

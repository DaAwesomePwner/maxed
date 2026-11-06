<script lang="ts">
  import DpsLabel from '../runs/DpsLabel.svelte';
  import { dpsMetric } from '../runs/dps-metric';
  import type { RunPlayerFight } from '../bindings/RunPlayerFight';
  import { formatDps, formatShare, formatStat } from '../styles/format';
  import { damageRank, nonnegative } from '../runs/damage';
  import './players.css';
  let { rows, currentFightId, hrefFor }: { rows: RunPlayerFight[]; currentFightId?: string; hrefFor: (row: RunPlayerFight) => string | null } = $props();
  let bases = $derived([...new Set(rows.map(row => row.dps_basis ?? 'whole_target_v0'))]);
  let label = $derived(bases.length === 1 ? dpsMetric(bases[0]).label : 'DPS');
</script>
<section class="player-card" aria-label="Boss fights">
  <div class="heading"><h3>Boss fights</h3><span>{#each bases as basis}<DpsLabel {basis} /> {/each}</span></div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="player-table-scroll" tabindex="0" role="region" aria-label="Boss fights table scroll area">
    <table class="player-boss-table" aria-label="Boss fights">
      <thead><tr><th scope="col">Boss</th><th scope="col">Damage</th><th scope="col">{label}</th><th scope="col">Rank</th><th scope="col">Share</th><th scope="col">Deaths</th></tr></thead>
      <tbody>{#each rows as row}
        {@const href = hrefFor(row)}
        {@const name = row.target_name ?? (row.target_type != null || row.boss_type != null ? `Target ${row.target_type ?? row.boss_type}` : '—')}
        <tr data-current={row.fight_id === currentFightId} class:linked={href !== null}>
          <th scope="row">{#if href}<a {href} aria-current={row.fight_id === currentFightId ? 'page' : undefined}>{name}</a>{:else}{name}{/if}</th>
          <td>{formatStat(nonnegative(row.damage ?? null))}</td><td title={dpsMetric(row.dps_basis).tooltip} aria-label={`${dpsMetric(row.dps_basis).label}: ${formatDps(row.dps)}`}>{formatDps(row.dps)}{#if bases.length > 1 && row.dps != null}<small>{dpsMetric(row.dps_basis).label}</small>{/if}</td>
          <td>{damageRank(row.rank ?? null) === '—' ? '—' : `#${damageRank(row.rank ?? null)}`}</td><td>{formatShare(row.share ?? null)}</td>
          <td class:player-danger={row.deaths != null && row.deaths > 0}>{formatStat(nonnegative(row.deaths ?? null))}</td>
        </tr>
      {:else}<tr><td colspan="6" class="player-muted">No boss fights observed</td></tr>{/each}</tbody>
    </table>
  </div>
</section>

<style>
  .heading { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; }
  .heading span, small { color: var(--text-muted); font-size: 11px; }
  small { display: block; }
</style>

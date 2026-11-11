<script lang="ts">
  import type { StatView } from '../bindings/StatView';
  import type { StatPanel } from '../bindings/StatPanel';
  import { statCapUnavailableReason, statPotsLeft } from '../shell/vitals';
  import { amount, reasonText } from './FormulaDetails.svelte';
  let { panel = null, stats = [] }: { panel?: StatPanel | null; stats?: StatView[] } = $props();
  const statLabels = [{ id: 0, label: 'HP' }, { id: 3, label: 'MP' }, { id: 20, label: 'ATT' },
    { id: 21, label: 'DEF' }, { id: 22, label: 'SPD' }, { id: 28, label: 'DEX' },
    { id: 26, label: 'VIT' }, { id: 27, label: 'WIS' }];
</script>
<section aria-labelledby="character-stats">
  <h2 id="character-stats">Stats</h2>
  <p>Observed totals and raw boosts are retained. Base / gear / exalt / Other is a
    {panel?.breakdown_provisional !== false ? 'provisional' : 'reported'} split.
    Cap decisions use each stat's placement evidence; missing or inconsistent values remain unavailable.
    Other is an unexplained remainder, not a named effect.</p>
  <table>
    <thead><tr><th>Stat</th><th>Observed total</th><th>Raw boost</th><th>Base</th><th>Gear</th><th>Exalt</th><th>Other</th><th>Cap / pots left</th></tr></thead>
    <tbody>
      {#each statLabels as stat}
        {@const line = panel?.lines.find(line => line.stat_id === stat.id)}
        {@const pots = statPotsLeft(line)}
        {@const reason = reasonText(line?.split_evidence?.reasons ?? []) || 'Not observed'}
        <tr>
          <th scope="row">{stat.label}</th>
          <td>{amount(line?.total)}{#if line?.total == null}<small>Not observed</small>{/if}
            {#if stat.id === 0 || stat.id === 3}<small>Current: {amount(line?.current)}{line?.current == null ? ' · Not observed' : ''}</small>{/if}</td>
          <td>{amount(line?.boost)}{#if line?.boost == null}<small>Not observed</small>{/if}</td>
          {#each ['base', 'gear', 'exalt', 'other'] as key}
            <td>{amount(line?.[key as 'base' | 'gear' | 'exalt' | 'other'])}
              {#if line?.[key as 'base' | 'gear' | 'exalt' | 'other'] == null}<small>{reason}</small>{/if}</td>
          {/each}
          <td>{#if pots !== null}{amount(line?.cap)}<small>{line?.at_cap ? 'At cap' : 'Below cap'} · {pots} {pots === 1 ? 'pot' : 'pots'} left</small>
            {:else}—<small>{line ? statCapUnavailableReason(line) : 'Split evidence missing'}</small>{/if}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  <details><summary>Raw observations</summary>
    <ul>{#each stats as stat}<li>Stat {stat.stat_id}: {stat.value}</li>
      {:else}<li>— No raw stats observed</li>{/each}</ul>
  </details>
</section>
<style>
  section { min-width: 0; }
  h2 { margin: 0 0 var(--space-3); font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); }
  table { width: 100%; border-collapse: collapse; table-layout: fixed; font-variant-numeric: tabular-nums; }
  th, td { padding: var(--space-2) var(--space-1); text-align: left; vertical-align: top; border-bottom: 1px solid var(--hairline); overflow-wrap: anywhere; font-size: var(--text-label); line-height: var(--leading-label); }
  tbody th { font-weight: var(--weight-medium); }
  thead th { font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-medium); color: var(--text-muted); border-bottom-color: var(--hairline-strong); }
  small { display: block; color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  summary { cursor: pointer; min-height: var(--target-min); align-content: center; font-size: var(--text-label); color: var(--text-secondary); }
  summary:hover { color: var(--text-primary); }
  ul { color: var(--text-secondary); font-size: var(--text-label); }
  p { margin-block: 0 var(--space-3); color: var(--text-secondary); font-size: var(--text-label); line-height: var(--leading-label); }
</style>

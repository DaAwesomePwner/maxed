<script lang="ts">
  import type { FightView } from '../bindings/FightView';
  import { formatDps, formatElapsed } from '../styles/format';
  import { bucketRate, seriesForChart } from './fight-screen';
  let { fight, plotted, onToggle }: { fight: FightView; plotted: string[]; onToggle: (id: string) => void } = $props();
  const uid = $props.id();
  let expandedFor = $state<string | null>(null);
  let expanded = $derived(expandedFor === fight.id);
  let moreButton: HTMLButtonElement | undefined = $state();
  let choices = $derived(seriesForChart(fight, ['party', ...fight.players.map(row => row.player_id)]));
  let series = $derived(choices.filter(row => plotted.includes(row.id))
    .sort((a, b) => Number(a.local) - Number(b.local) || Number(b.id === 'party') - Number(a.id === 'party')));
  let rates = $derived(series.map(row => ({ ...row, points: row.points.map((value, index) =>
    bucketRate(value, fight.buckets[index].from, fight.buckets[index].until)) })));
  let max = $derived(Math.max(1, ...rates.flatMap(row => row.points.filter(Number.isFinite))));
  let first = $derived(fight.buckets[0]?.from ?? null);
  let last = $derived(fight.buckets.at(-1)?.until ?? null);
  let validTime = $derived(first !== null && last !== null && /^\d+$/.test(first) && /^\d+$/.test(last) && BigInt(last) > BigInt(first));
  const x = (at: string) => validTime ? Number(BigInt(at) - BigInt(first!)) / Number(BigInt(last!) - BigInt(first!)) * 800 : 0;
  function paths(points: number[]) {
    const result: string[] = [];
    if (!validTime) return result;
    for (const [i, value] of points.entries()) {
      if (!Number.isFinite(value)) continue;
      const bucket = fight.buckets[i], y = 164 - value / max * 154;
      if (i > 0 && Number.isFinite(points[i - 1]) && fight.buckets[i - 1].until === bucket.from) {
        result[result.length - 1] += `V${y}H${x(bucket.until)}`;
      } else result.push(`M${x(bucket.from)} 170V${y}H${x(bucket.until)}`);
    }
    return result;
  }
</script>
<svelte:window onkeydown={event => {
  if (expanded && event.key === 'Escape') {
    event.preventDefault(); expandedFor = null; moreButton?.focus();
  }
}} />
{#snippet chip(row: (typeof choices)[number])}
  <button class="ctl" class:local={row.local} type="button" aria-pressed={plotted.includes(row.id)} onclick={() => onToggle(row.id)} title={row.label}>
    <i class:dashed={row.dash !== null} style:color={row.color}></i><span>{row.label}</span>
  </button>
{/snippet}
<section class="area-card chart" aria-label="Damage over time">
  <div class="card-heading"><h3>Damage over time</h3>
    <div class="legend" aria-label="Chart series">
      {#each choices.slice(0, 4) as row (row.id)}{@render chip(row)}{/each}
      {#if choices.length > 4}<button bind:this={moreButton} class="ctl more" type="button" aria-expanded={expanded}
        aria-controls={`${uid}-more-series`} onclick={() => expandedFor = expanded ? null : fight.id}>+{choices.length - 4}</button>{/if}
    </div>
  </div>
  {#if expanded}<div class="legend expanded" id={`${uid}-more-series`} role="group" aria-label="More chart series">
    {#each choices.slice(4) as row (row.id)}{@render chip(row)}{/each}
  </div>{/if}
  <svg viewBox="0 0 800 170" preserveAspectRatio="none" role="img" aria-label="Fight DPS" aria-describedby={`${uid}-description`}>
    <path d="M0 42H800M0 85H800M0 128H800" stroke="var(--chart-grid)" vector-effect="non-scaling-stroke" />
    {#each rates as row (row.id)}
      {#each paths(row.points) as path}
        <path d={path} fill="none" stroke={row.color} stroke-width={row.local ? 2 : 1.5} stroke-dasharray={row.dash ?? undefined}
          vector-effect="non-scaling-stroke" />
      {/each}
    {/each}
  </svg>
  <p class="sr-only" id={`${uid}-description`}>Damage per second for the plotted series is listed by interval in the following table. A dash means the rate is unknown.</p>
  <table class="sr-only" aria-label="Fight DPS by interval">
    <thead><tr><th scope="col">Interval</th>{#each rates as row (row.id)}<th scope="col">{row.label} DPS</th>{/each}</tr></thead>
    <tbody>{#each fight.buckets as bucket, index}<tr>
      <th scope="row">{index + 1}: {formatElapsed(first!, bucket.from)} – {formatElapsed(first!, bucket.until)}</th>
      {#each rates as row (row.id)}<td>{formatDps(row.points[index])}</td>{/each}
    </tr>{/each}</tbody>
  </table>
  <div class="axis"><span>{validTime ? '0:00' : '—'}</span><span>{validTime ? formatElapsed(first!, String((BigInt(first!) + BigInt(last!)) / 2n)) : '—'}</span><span>{validTime ? formatElapsed(first!, last!) : '—'}</span></div>
  {#if !fight.buckets.length}<p>No damage series</p>{/if}
</section>
<style>
  .chart { grid-area: chart; align-self: start; }
  .card-heading { display: flex; align-items: center; gap: var(--space-2); min-width: 0; }
  .chart .card-heading h3 { margin: 0; flex: none; white-space: nowrap; }
  .legend { display: flex; gap: var(--space-1); min-width: 0; margin-left: auto; justify-content: flex-end; }
  .legend.expanded { flex-wrap: wrap; justify-content: flex-start; max-height: 96px; overflow: auto; margin-top: var(--space-2); }
  /* Series toggles: ghost when off, a neutral surface step when plotted. Only your label is gold. */
  .legend button { height: 28px; max-width: 140px; min-width: 0; flex: 0 1 auto; padding-inline: var(--space-2); gap: var(--space-1);
    border-color: transparent; background: transparent; color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .legend button[aria-pressed='true'] { border-color: var(--hairline-strong); background: var(--surface-raised); color: var(--text-secondary); }
  .legend button.local { color: var(--gold); }
  .legend .more { flex: none; color: var(--text-secondary); }
  .legend i { width: 12px; height: 0; flex: none; border-top: 2px solid currentColor; }
  .legend i.dashed { border-top-style: dashed; }
  .legend button[aria-pressed='false'] i { opacity: .5; }
  .legend span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  svg { display: block; width: 100%; height: 170px; margin-top: var(--space-2); }
  .axis { display: flex; justify-content: space-between; margin-top: var(--space-1); color: var(--chart-label); font-size: var(--text-meta); line-height: var(--leading-meta); font-variant-numeric: tabular-nums; }
  p { color: var(--text-muted); }
</style>

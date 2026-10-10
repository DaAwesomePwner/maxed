<script lang="ts">
  import type { FightView } from '../bindings/FightView';
  import type { PlayerDamage } from '../bindings/PlayerDamage';
  import { bucketRate, seriesForChart } from './fight-screen';
  import { nonnegative } from './damage';
  import { formatDps, formatElapsed } from '../styles/format';
  let { fight, player }: { fight: FightView; player: PlayerDamage } = $props();
  const uid = $props.id();
  let series = $derived(seriesForChart(fight, [player.player_id]).find(row => row.id === player.player_id));
  let rates = $derived(series?.points.map((value, index) => bucketRate(value, fight.buckets[index].from, fight.buckets[index].until)) ?? []);
  let party = $derived(fight.players.map(row => nonnegative(row.dps)).filter((value): value is number => value !== null));
  let average = $derived(party.length ? party.reduce((sum, value) => sum + value, 0) / party.length : null);
  let maximum = $derived(Math.max(1, average ?? 0, ...rates.filter(Number.isFinite)));
  let first = $derived(fight.buckets[0]?.from ?? null);
  let last = $derived(fight.buckets.at(-1)?.until ?? null);
  let valid = $derived(first !== null && last !== null && /^\d+$/.test(first) && /^\d+$/.test(last) && BigInt(last) > BigInt(first));
  let title = $derived(player.is_local ? 'Your DPS over the fight' : `${player.name ?? 'Unknown player'}'s DPS over the fight`);
  const x = (at: string) => valid ? Number(BigInt(at) - BigInt(first!)) / Number(BigInt(last!) - BigInt(first!)) * 320 : 0;
  const y = (rate: number) => 104 - rate / maximum * 98;
  let paths = $derived.by(() => {
    const result: Array<{ line: string; start: number }> = [];
    if (!valid) return result;
    for (const [index, rate] of rates.entries()) {
      if (!Number.isFinite(rate)) continue;
      const bucket = fight.buckets[index];
      if (index > 0 && Number.isFinite(rates[index - 1]) && fight.buckets[index - 1].until === bucket.from) {
        result[result.length - 1].line += `V${y(rate)}H${x(bucket.until)}`;
      } else result.push({ line: `M${x(bucket.from)} ${y(rate)}H${x(bucket.until)}`, start: x(bucket.from) });
    }
    return result;
  });
</script>
{#if valid && rates.some(Number.isFinite)}
  <section class="area-card chart" aria-label={title}>
    <h3 title={title}>{title}</h3>
    <svg viewBox="0 0 320 110" preserveAspectRatio="none" role="img" aria-label="Player DPS" aria-describedby={`${uid}-description`}>
      <path d="M0 37H320M0 74H320" stroke="var(--border)" />
      {#each paths as path}
        <path d={`${path.line}V110H${path.start}Z`} fill="var(--gold)" opacity=".1" />
        <path d={path.line} fill="none" stroke="var(--gold)" stroke-width="2" vector-effect="non-scaling-stroke" />
      {/each}
      {#if average !== null}<path d={`M0 ${y(average)}H320`} stroke="var(--violet)" stroke-dasharray="4 4" stroke-width="1.4" vector-effect="non-scaling-stroke" />{/if}
    </svg>
    <p class="sr-only" id={`${uid}-description`}>Player damage per second by interval. The dashed line is the mean saved DPS of players with a known DPS: {formatDps(average)}. A dash means unknown.</p>
    <table class="sr-only" aria-label="Player DPS by interval">
      <thead><tr><th scope="col">Interval</th><th scope="col">{player.name ?? 'Unknown player'} DPS</th><th scope="col">Party average</th></tr></thead>
      <tbody>{#each fight.buckets as bucket, index}<tr><th scope="row">{index + 1}: {formatElapsed(first!, bucket.from)} – {formatElapsed(first!, bucket.until)}</th><td>{formatDps(rates[index])}</td><td>{formatDps(average)}</td></tr>{/each}</tbody>
    </table>
    <div class="axis"><span>0:00</span><span class="average">party average{#if average === null} —{/if}</span><span>{formatElapsed(first!, last!)}</span></div>
  </section>
{/if}
<style>
  h3 { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  svg { display: block; width: 100%; height: 110px; }
  .axis { display: flex; justify-content: space-between; gap: 8px; margin-top: 4px; color: var(--text-muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .average { color: var(--violet-text); }
  .average::before { content: ''; display: inline-block; width: 12px; border-top: 1px dashed var(--violet); margin-right: 4px; vertical-align: middle; }
</style>

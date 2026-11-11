<script lang="ts">
  import type { DamageBucket } from '../../bindings/DamageBucket';
  import { partySeries } from './you-vs-party';

  let { buckets, width = 120, height = 28 }: { buckets: DamageBucket[]; width?: number; height?: number } = $props();
  const lines = $derived.by(() => {
    const series = partySeries(buckets);
    const times = buckets.map(bucket => /^\d+$/.test(bucket.from) && /^\d+$/.test(bucket.until) && BigInt(bucket.until) > BigInt(bucket.from)
      ? { from: BigInt(bucket.from), until: BigInt(bucket.until) } : null);
    const valid = times.filter(time => time !== null);
    if (!valid.length) return { you: [], party: [] };
    const start = valid.reduce((a, b) => a < b.from ? a : b.from, valid[0].from);
    const end = valid.reduce((a, b) => a > b.until ? a : b.until, valid[0].until);
    const peak = Math.max(1, ...series.you.filter(value => value !== null), ...series.party.filter(value => value !== null));
    const x = (at: bigint) => 1 + Number((at - start) * 1_000_000n / (end - start)) / 1_000_000 * (width - 2);
    const y = (value: number) => height - 1 - value / peak * (height - 2);
    function segments(values: (number | null)[]): string[] {
      const result: string[] = [];
      let points: string[] = [], previous: bigint | null = null;
      const flush = () => { if (points.length) result.push(points.join(' ')); points = []; };
      for (let i = 0; i < times.length; i++) {
        const time = times[i], value = values[i];
        if (!time || value === null) { flush(); previous = null; continue; }
        if (previous !== time.from) flush();
        points.push(`${x(time.from)},${y(value)}`, `${x(time.until)},${y(value)}`);
        previous = time.until;
      }
      flush(); return result;
    }
    return { you: segments(series.you), party: segments(series.party) };
  });
</script>

<svg class="party-sparkline" {width} {height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
  {#each lines.party as points}<polyline class="party" {points} />{/each}
  {#each lines.you as points}<polyline class="you" {points} />{/each}
</svg>

<style>
  svg { display: block; }
  polyline { fill: none; stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
  .you { stroke: var(--chart-you); }
  .party { stroke: var(--chart-party); stroke-width: 1.5; }
</style>

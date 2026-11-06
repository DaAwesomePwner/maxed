<script lang="ts">
  import type { PlayerTimeline } from '../bindings/PlayerTimeline';
  import { bucketRate } from '../runs/fight-screen';
  import { formatClock, formatDps } from '../styles/format';
  import { timestampMs } from './player-view';
  import './players.css';
  let { timeline, bands, unavailable, note = null, missingText = 'Not recorded for this run' }: {
    note?: string | null; missingText?: string; timeline: PlayerTimeline | null; bands: { from: number; until: number; label: string }[]; unavailable?: 'legacy' | 'limit';
  } = $props();
  const uid = $props.id();
  const W = 800, H = 200;
  let hovered = $state<number | null>(null);
  let buckets = $derived(timeline?.buckets ?? []);
  let bounds = $derived(buckets.map(bucket => ({ from: timestampMs(bucket.from), until: timestampMs(bucket.until) })));
  let first = $derived(bounds[0]?.from ?? null);
  let last = $derived(bounds.at(-1)?.until ?? null);
  let valid = $derived(first !== null && last !== null && last > first);
  let rates = $derived(buckets.map((bucket, i) => bucketRate(timeline?.points[i] ?? NaN, bucket.from, bucket.until)));
  let averages = $derived(buckets.map(bucket => bucket.party_active > 0
    ? bucketRate(bucket.party_total / bucket.party_active, bucket.from, bucket.until) : NaN));
  let maximum = $derived(Math.max(1, ...rates.filter(Number.isFinite), ...averages.filter(Number.isFinite)) * 1.1);
  const x = (at: number) => valid ? (at - first!) / (last! - first!) * W : 0;
  const y = (value: number) => H - value / maximum * (H - 8);
  function paths(values: number[]): { line: string; start: number }[] {
    const result: { line: string; start: number }[] = [];
    if (!valid) return result;
    for (const [i, value] of values.entries()) {
      const { from, until } = bounds[i];
      if (!Number.isFinite(value) || from === null || until === null || until <= from) continue;
      if (i > 0 && Number.isFinite(values[i - 1]) && buckets[i - 1].until === buckets[i].from) result[result.length - 1].line += `V${y(value)}H${x(until)}`;
      else result.push({ start: x(from), line: `M${x(from)} ${y(value)}H${x(until)}` });
    }
    return result;
  }
  let playerPaths = $derived(paths(rates));
  let averagePaths = $derived(paths(averages));
  let shaded = $derived(valid ? bands.flatMap(band => {
    const from = Math.max(first!, band.from), until = Math.min(last!, band.until);
    return until > from ? [{ ...band, from, until }] : [];
  }) : []);
  let selected = $derived(hovered === null ? null : buckets[hovered] ?? null);
  let selectedAt = $derived(hovered === null ? null : bounds[hovered]?.from ?? null);
  let bandLabel = $derived(selectedAt === null ? null : bands.find(band => selectedAt! >= band.from && selectedAt! < band.until)?.label ?? null);
  let limited = $derived(unavailable === 'limit' || timeline?.disabled === true);
  function inspect(event: PointerEvent) {
    if (!valid) return;
    const rect = (event.currentTarget as SVGSVGElement).getBoundingClientRect();
    if (rect.width <= 0) return;
    const at = first! + Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)) * (last! - first!);
    const i = bounds.findIndex(({ from, until }) => {
      return from !== null && until !== null && at >= from && at < until;
    });
    hovered = at === last ? buckets.length - 1 : i >= 0 ? i : null;
  }
</script>
<section class="player-card player-timeline" aria-label="DPS over time">
  <h3>DPS over time {#if timeline?.compacted && !limited && valid && rates.some(Number.isFinite)}<span class="player-card-sub">approx.</span>{/if}
    <span class="player-chart-legend"><span class="player-line-key">Player</span><span class="party-line-key">Party average</span>{#if bands.length}<span class="boss-band-key">Boss fight</span>{/if}</span>
  </h3>
  {#if note}<p class="player-muted"><small>{note}</small></p>{/if}
  {#if limited}<p class="player-muted">Timeline unavailable (resource limit)</p>
  {:else if timeline === null}<p class="player-muted">{missingText}</p>
  {:else if !valid || !rates.some(Number.isFinite)}<p class="player-muted">Not seen</p>
  {:else}
    <div class="player-chart">
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="Player and party average DPS" aria-describedby={`${uid}-description`} onpointermove={inspect} onpointerleave={() => { hovered = null; }}>
        {#each shaded as band}<rect x={x(band.from)} y="0" width={x(band.until) - x(band.from)} height={H} fill="var(--text-primary)" fill-opacity=".04" stroke="var(--border)" stroke-opacity=".4"><title>{band.label}</title></rect>{/each}
        <path d={`M0 ${H / 3}H${W}M0 ${H * 2 / 3}H${W}`} stroke="var(--border)" vector-effect="non-scaling-stroke" />
        {#each playerPaths as path}<path d={`${path.line}V${H}H${path.start}Z`} fill="var(--gold)" opacity=".1" />{/each}
        {#each averagePaths as path}<path d={path.line} fill="none" stroke="var(--violet)" stroke-width="1.5" stroke-dasharray="4 4" vector-effect="non-scaling-stroke" />{/each}
        {#each playerPaths as path}<path d={path.line} fill="none" stroke="var(--gold)" stroke-width="2" vector-effect="non-scaling-stroke" />{/each}
        {#if selectedAt !== null}<line x1={x(selectedAt)} x2={x(selectedAt)} y1="0" y2={H} stroke="var(--border-strong)" vector-effect="non-scaling-stroke" />{/if}
      </svg>
      {#if selected && hovered !== null && selectedAt !== null}
        <div class="player-chart-tip" role="tooltip" style:left={`clamp(0px, ${x(selectedAt) / W * 100}%, max(0px, calc(100% - 240px)))`}>
          <b>{formatClock(selected.from) ?? '—'}{#if bandLabel} · {bandLabel}{/if}</b>
          <span>Player {formatDps(rates[hovered])} DPS</span><span class="party-average">Party average {formatDps(averages[hovered])} DPS</span>
        </div>
      {/if}
      <input class="player-chart-inspect" type="range" aria-label="Inspect timeline" min="0" max={Math.max(0, buckets.length - 1)} step="1" value={hovered ?? 0}
        aria-valuetext={selected ? `${formatClock(selected.from) ?? '—'}${bandLabel ? ` · ${bandLabel}` : ''}, Player ${formatDps(rates[hovered!])} DPS, Party average ${formatDps(averages[hovered!])} DPS` : undefined}
        onfocus={() => { hovered = 0; }} oninput={event => { hovered = Number(event.currentTarget.value); }} onblur={() => { hovered = null; }} />
    </div>
    <div class="player-chart-axis"><span>{formatClock(buckets[0].from) ?? '—'}</span><span>{formatClock(buckets.at(-1)!.until) ?? '—'}</span></div>
    <p class="sr-only" id={`${uid}-description`}>Gold shows player damage per second by interval. Violet dashes show party damage per second divided by the active player count in each interval. Shading marks boss fights. A dash means unknown.{#if timeline.compacted} Bucket resolution is approximate.{/if}</p>
    <table class="sr-only" aria-label="DPS by interval"><thead><tr><th scope="col">Time</th><th scope="col">Player DPS</th><th scope="col">Party average DPS</th></tr></thead>
      <tbody>{#each buckets as bucket, i}<tr><th scope="row">{formatClock(bucket.from) ?? '—'} to {formatClock(bucket.until) ?? '—'}</th><td>{formatDps(rates[i])}</td><td>{formatDps(averages[i])}</td></tr>{/each}</tbody>
    </table>
  {/if}
</section>

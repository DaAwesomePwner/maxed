<script module lang="ts">
  import type { DamageChartProps } from './contracts';

  function micros(value: string): bigint | null { return /^\d+$/.test(value) ? BigInt(value) : null; }
  // DPS uses each positive bucket duration; invalid durations remain unknown.
  function rate(value: number | null, seconds: number): number | null {
    if (value === null || !Number.isFinite(value) || value < 0 || !(seconds > 0)) return null;
    const result = value / seconds;
    return Number.isFinite(result) ? result : null;
  }

  /** Retain original indexes and interval endpoints, including after display sampling. */
  export function damageChartView({ buckets, playerSeries, selectedPlayerId }: DamageChartProps) {
    const matches = selectedPlayerId === null ? [] : playerSeries?.filter(row => row.player_id === selectedPlayerId) ?? [];
    const selected = matches.length === 1 && matches[0].points.length === buckets.length ? matches[0] : null;
    const reasons: string[] = [];
    if (playerSeries === null) reasons.push('Per-player series unavailable; it may be omitted by the snapshot series cap.');
    else if (selectedPlayerId !== null && !selected) reasons.push('Selected-player series unavailable: requires one matching player ID and exact bucket alignment.');
    const indexes = buckets.length <= 120 ? buckets.map((_, index) => index)
      : Array.from({ length: 120 }, (_, index) => Math.round(index * (buckets.length - 1) / 119));
    const displayed = new Set(indexes);
    let max: number | null = null;
    if (buckets.length > 120) reasons.push('Sampled to 120 intervals; omitted intervals remain gaps.');
    const rows = buckets.flatMap((bucket, index) => {
      const from = micros(bucket.from), until = micros(bucket.until);
      const seconds = from !== null && until !== null && until > from ? Number(until - from) / 1_000_000 : 0;
      const row = { index, from, until, seconds, total: rate(bucket.total, seconds), local: rate(bucket.local, seconds),
        player: selected ? rate(selected.points[index], seconds) : null };
      for (const value of [row.total, row.local, row.player]) if (value !== null) max = max === null ? value : Math.max(max, value);
      return displayed.has(index) ? [row] : [];
    });
    if (rows.some(row => !(row.seconds > 0))) reasons.push('Invalid or nonpositive bucket durations are unavailable.');
    if (rows.some(row => row.local === null)) reasons.push('Local DPS is unavailable in some intervals; the local line has gaps.');
    if (rows.some(row => row.total === null)) reasons.push('Party DPS is unavailable in some intervals.');
    if (selected && rows.some(row => row.player === null)) reasons.push('Selected-player DPS is unavailable in some intervals.');
    const timed = rows.filter(row => row.from !== null && row.until !== null && row.seconds > 0);
    const start = timed.reduce<bigint | null>((at, row) => at === null || row.from! < at ? row.from : at, null);
    const end = timed.reduce<bigint | null>((at, row) => at === null || row.until! > at ? row.until : at, null);
    const x = (at: bigint) => start === null || end === null || end <= start ? 12
      : 12 + Number((at - start) * 1_000_000n / (end - start)) / 1_000_000 * 576;
    function path(key: 'total' | 'local' | 'player'): string {
      let result = '', previous: (typeof rows)[number] | null = null;
      for (const row of rows) {
        const value = row[key];
        if (value === null || row.from === null || row.until === null) { previous = null; continue; }
        const y = max === null || max === 0 ? 92 : 92 - value / max * 80;
        const contiguous = previous?.index === row.index - 1 && previous.until === row.from;
        result += `${contiguous ? 'L' : 'M'}${x(row.from)},${y} L${x(row.until)},${y} `;
        previous = row;
      }
      return result.trim();
    }
    return { rows, start, end, max, reasons, selectedAvailable: selected !== null,
      partyPath: path('total'), localPath: path('local'), playerPath: path('player') };
  }
</script>

<script lang="ts">
  import DebugNote from '../../debug/DebugNote.svelte';
  import { formatDps } from '../../styles/format';
  let { buckets, playerSeries, selectedPlayerId }: DamageChartProps = $props();
  const id = $props.id();
  let view = $derived(damageChartView({ buckets, playerSeries, selectedPlayerId }));
  let selectedIndex = $state(0);
  let row = $derived(view.rows[Math.min(selectedIndex, Math.max(0, view.rows.length - 1))]);
  const value = (number: number | null) => number === null ? '—' : formatDps(number);
  function offset(at: bigint | null): string {
    return at === null || view.start === null ? '—' : `${Number(at - view.start) / 1_000_000}s`;
  }
</script>

<div class="damage-chart">
  {#if view.start !== null && view.end !== null}
    <svg viewBox="0 0 600 104" preserveAspectRatio="none" role="img" aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`}>
      <title id={`${id}-title`}>Fight DPS</title>
      <desc id={`${id}-description`}>Party, Local, Player</desc>
      <path class="party" d={view.partyPath} />
      <path class="local" d={view.localPath} />
      {#if selectedPlayerId !== null}<path class="player" d={view.playerPath} />{/if}
    </svg>
    <p><span class="party-key">Party</span>{' \u00b7 '}<span class="local-key">Local</span>{#if selectedPlayerId !== null}{' \u00b7 '}Player{/if}
      {' \u00b7 '}0&ndash;{offset(view.end)}{' \u00b7 '}Peak {value(view.max)} DPS</p>
  {:else}<p>No DPS</p><DebugNote text="Fight DPS unavailable: no positive-duration buckets." />{/if}
  <details>
    <summary>Intervals</summary>
    {#if view.rows.length}
      <label for={`${id}-bucket`}>Interval</label>
      <select id={`${id}-bucket`} value={Math.min(selectedIndex, view.rows.length - 1)} onchange={event => selectedIndex = Number(event.currentTarget.value)}>
        {#each view.rows as bucket, index}<option value={index}>{bucket.index + 1}: {offset(bucket.from)}–{offset(bucket.until)}</option>{/each}
      </select>
      {#if row}<p aria-live="polite">{offset(row.from)}–{offset(row.until)} · {row.seconds > 0 ? `${row.seconds}s` : '—'} · Party {value(row.total)} DPS · Local {value(row.local)} DPS{#if selectedPlayerId !== null}{' · '}Player {value(row.player)} DPS{/if}</p>{/if}
    {/if}
    {#each view.reasons as reason}<DebugNote text={reason} />{/each}
    <!-- Paths preserve gaps rather than interpolating missing observations. -->
    <DebugNote text={`${view.rows.length} of ${buckets.length} supplied intervals. No interpolation across missing observations.`} />
  </details>
</div>

<style>
  .damage-chart { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 0 var(--space-2); min-width: 0; font-size: var(--text-label); line-height: var(--leading-label); }
  svg { display: block; width: 100%; height: var(--space-8); grid-column: 1 / -1; overflow: visible; }
  path { fill: none; stroke-width: 2; vector-effect: non-scaling-stroke; }
  .party { stroke: var(--violet-text); } .local { stroke: var(--gold); stroke-dasharray: 6 3; }
  .player { stroke: var(--information); stroke-dasharray: 2 3; }
  .party-key { color: var(--violet-text); } .local-key { color: var(--gold); }
  p { margin: var(--space-1) 0; color: var(--text-secondary); }
  details[open] { grid-column: 1 / -1; }
  summary { min-height: var(--target-min); align-content: center; cursor: pointer; }
  select { min-height: var(--target-min); max-width: 100%; margin-inline: var(--space-2); font: inherit; color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-control); }
</style>

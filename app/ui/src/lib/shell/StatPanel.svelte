<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import { debugMode } from '../debug/debug';
  import VitalBar from '../components/VitalBar.svelte';
  import type { StatLine } from '../bindings/StatLine';
  import type { StatPanel as ObservedStatPanel } from '../bindings/StatPanel';
  import { statPotsLeft, statCapUnavailableReason, statTone, vitalText, statBoost, statExalt } from './vitals';
  let { panel = null, stale = false }: { panel?: ObservedStatPanel | null; stale?: boolean } = $props();
  const vitals = [{ id: 0, label: 'HP', kind: 'hp' }, { id: 3, label: 'MP', kind: 'mp' }] as const;
  const stats = [{ id: 20, label: 'ATT' }, { id: 21, label: 'DEF' }, { id: 22, label: 'SPD' },
    { id: 28, label: 'DEX' }, { id: 26, label: 'VIT' }, { id: 27, label: 'WIS' }];
  const known = (n: number | null | undefined): n is number => typeof n === 'number' && Number.isFinite(n);
  const exaltDescription = (label: string, exalt: number, line: StatLine | undefined) =>
    `${label} exaltation bonus +${exalt}${['verified_inside_base', 'verified_inside_boost'].includes(line?.split_evidence?.placement ?? '')
      ? '' : ' (placement unverified; may be included in boost)'}`;
  // Tones intentionally use the raw observed boost, even when verified exalt accounts for all of it.
  let capReasons = $derived([...new Set(panel?.lines.map(statCapUnavailableReason).filter(Boolean) ?? [])]);
  let provisional = $derived(panel?.breakdown_provisional || panel?.lines.some(line =>
    statExalt(line) !== null && !['verified_inside_base', 'verified_inside_boost'].includes(line.split_evidence?.placement ?? '')));
  let conditions = $derived(panel?.conditions.filter(c => c.confirmed && c.name.trim()) ?? []);
</script>

<section class="stat-panel" class:stale aria-label="Stats">
  {#each vitals as stat (stat.id)}
    {@const line = panel?.lines.find(l => l.stat_id === stat.id)}
    {@const filled = known(line?.current) && line!.current! >= 0 && known(line?.total) && line!.total! > 0}
    {@const exalt = statExalt(line)}
    {@const pots = statPotsLeft(line)}
    {@const text = known(line?.total) ? vitalText(line?.current ?? null, line.total, pots, statBoost(line))
      : vitalText(line?.current ?? null, null, null, 0) + '/' + vitalText(null, null, pots, statBoost(line))}
    <div data-stat-id={stat.id} class:at-cap={statPotsLeft(line) !== null && line?.at_cap === true}
      title={!filled && $debugMode ? `${stat.label} fill unavailable: current and a positive total are required` : undefined}>
      <VitalBar kind={stat.kind} label={stat.label} {text} fill={filled ? line!.current! / line!.total! * 100 : 0}
        tone={statTone(statPotsLeft(line) !== null && line?.at_cap === true, line?.boost ?? null)} />
      {#if exalt !== null}
        <span class="exalt vital-exalt" title={exaltDescription(stat.label, exalt, line)}><span aria-hidden="true">Exalt +{exalt}</span><span class="sr-only">{exaltDescription(stat.label, exalt, line)}</span></span>
      {/if}
    </div>
  {/each}
  <dl class="stat-grid">
    {#each stats as stat (stat.id)}
      {@const line = panel?.lines.find(l => l.stat_id === stat.id)}
      {@const exalt = statExalt(line)}
      <div class="stat" data-stat-id={stat.id} data-tone={statTone(statPotsLeft(line) !== null && line?.at_cap === true, line?.boost ?? null)} class:at-cap={statPotsLeft(line) !== null && line?.at_cap === true}>
        <dt>{stat.label}</dt>
        <dd>{vitalText(line?.total ?? null, null, statPotsLeft(line), statBoost(line))}{#if exalt !== null}{' '}<span class="exalt" title={exaltDescription(stat.label, exalt, line)}><span aria-hidden="true">Exalt +{exalt}</span><span class="sr-only">{exaltDescription(stat.label, exalt, line)}</span></span>{/if}</dd>
      </div>
    {/each}
  </dl>
  {#if capReasons.length}<p class="cap-reason">Cap and pots unavailable: {capReasons.join('; ')}</p>{/if}
  {#if !panel}<DebugNote text="Stats unavailable · not observed" />
  {:else}<DebugNote text={`Observed boost${provisional ? ' · provisional split' : ''}`} />{/if}
  {#if $debugMode && conditions.length}
    <ul aria-label="Conditions">{#each conditions as condition}<li>{condition.name}</li>{/each}</ul>
  {/if}
</section>

<style>
  .stat-panel { min-width: 0; display: grid; gap: var(--space-1); }
  .cap-reason { margin: 0; color: var(--text-muted); font-size: var(--text-meta); overflow-wrap: anywhere; }
  .stat-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-1); margin: 0;
    container-type: inline-size; container-name: rail-stats; }
  .stat { min-width: 0; text-align: center; padding: var(--space-1) 2px; border: 1px solid var(--hairline); border-radius: var(--radius-sprite); background: var(--surface-inset); }
  dt { font-size: var(--text-meta); font-weight: var(--weight-medium); line-height: var(--leading-meta); color: var(--text-muted); }
  dd { margin: 0; overflow-wrap: anywhere; font: var(--weight-medium) var(--rail-stat-size, 14px)/calc(1em + 4px) var(--font-ui); /* whole-pixel line box tall enough for the glyphs at every stepped size */ font-variant-numeric: tabular-nums; color: var(--text-primary); }
  [data-tone="max"] dd { color: var(--stat-maxed); }
  [data-tone="boosted"] dd { color: var(--stat-boosted); }
  .exalt { display: block; color: var(--text-secondary); font-size: var(--text-meta); overflow-wrap: anywhere; }
  .vital-exalt { text-align: right; font-variant-numeric: tabular-nums; }
  .stale { opacity: .7; }
  ul { margin: 0; padding: 0; list-style: none; color: var(--text-muted); font-size: var(--text-meta); overflow-wrap: anywhere; }
  @media (max-width: 900px) {
    .stat-panel .stat { padding-inline: 1px; }
  }
  /* Classic scrollbars reduce the card width independently of the viewport.
     Size values for cell padding and glyph rounding, wrapping longer values as needed;
     min() preserves the hero's smaller narrow-rail size until space requires less. */
  @container rail-stats (max-width: 212px) {
    dd { font-size: min(var(--rail-stat-size, 14px), 13px); }
  }
  @container rail-stats (max-width: 184px) {
    dd { font-size: min(var(--rail-stat-size, 14px), 12px); }
  }
  /* No step below 12px (the meta size): longer values wrap instead of shrinking further. */
</style>

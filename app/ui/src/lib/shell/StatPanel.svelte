<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import { debugMode } from '../debug/debug';
  import VitalBar from '../components/VitalBar.svelte';
  import type { StatPanel as ObservedStatPanel } from '../bindings/StatPanel';
  import { potsLeft, statTone, vitalText } from './vitals';
  let { panel = null, stale = false }: { panel?: ObservedStatPanel | null; stale?: boolean } = $props();
  const vitals = [{ id: 0, label: 'HP', kind: 'hp' }, { id: 3, label: 'MP', kind: 'mp' }] as const;
  const stats = [{ id: 20, label: 'ATT' }, { id: 21, label: 'DEF' }, { id: 22, label: 'SPD' },
    { id: 28, label: 'DEX' }, { id: 26, label: 'VIT' }, { id: 27, label: 'WIS' }];
  const known = (n: number | null | undefined): n is number => typeof n === 'number' && Number.isFinite(n);
  let conditions = $derived(panel?.conditions.filter(c => c.confirmed && c.name.trim()) ?? []);
</script>

<section class="stat-panel" class:stale aria-label="Stats">
  {#each vitals as stat (stat.id)}
    {@const line = panel?.lines.find(l => l.stat_id === stat.id)}
    {@const filled = known(line?.current) && line!.current! >= 0 && known(line?.total) && line!.total! > 0}
    {@const pots = line?.at_cap ? 0 : potsLeft(stat.id, line?.base ?? null, line?.cap ?? null)}
    {@const text = known(line?.total) ? vitalText(line?.current ?? null, line.total, pots, line?.boost ?? null)
      : vitalText(line?.current ?? null, null, null, 0) + '/' + vitalText(null, null, pots, line?.boost ?? null)}
    <div data-stat-id={stat.id} class:at-cap={line?.at_cap === true}
      title={!filled && $debugMode ? `${stat.label} fill unavailable: current and a positive total are required` : undefined}>
      <VitalBar kind={stat.kind} label={stat.label} {text} fill={filled ? line!.current! / line!.total! * 100 : 0}
        tone={statTone(line?.at_cap === true, line?.boost ?? null)} />
    </div>
  {/each}
  <dl class="stat-grid">
    {#each stats as stat (stat.id)}
      {@const line = panel?.lines.find(l => l.stat_id === stat.id)}
      <div class="stat" data-stat-id={stat.id} data-tone={statTone(line?.at_cap === true, line?.boost ?? null)} class:at-cap={line?.at_cap === true}>
        <dt>{stat.label}</dt>
        <dd>{vitalText(line?.total ?? null, null, line?.at_cap ? 0 : potsLeft(stat.id, line?.base ?? null, line?.cap ?? null), line?.boost ?? null)}</dd>
      </div>
    {/each}
  </dl>
  {#if !panel}<DebugNote text="Stats unavailable · not observed" />
  {:else}<DebugNote text={`Observed boost${panel.breakdown_provisional ? ' · provisional split' : ''}`} />{/if}
  {#if $debugMode && conditions.length}
    <ul aria-label="Conditions">{#each conditions as condition}<li>{condition.name}</li>{/each}</ul>
  {/if}
</section>

<style>
  .stat-panel { min-width: 0; display: grid; gap: 4px; }
  .stat-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; margin: 0;
    container-type: inline-size; container-name: rail-stats; }
  .stat { min-width: 0; text-align: center; padding: 3px 2px 5px; border: 1px solid var(--border); border-radius: 5px; background: #00000045; }
  dt { font-size: 9.5px; font-weight: 700; line-height: 1.3; letter-spacing: .1em; color: var(--text-muted); }
  dd { margin: 0; white-space: nowrap; font: 700 var(--rail-stat-size, 14px)/calc(1em + 4px) var(--font-ui); /* whole-pixel line box tall enough for the glyphs at every stepped size */ font-variant-numeric: tabular-nums; letter-spacing: -.03em; color: #fff; }
  [data-tone="max"] dd { color: #ffe04a; }
  [data-tone="boosted"] dd { color: #5cff5c; }
  .stale { opacity: .7; }
  ul { margin: 0; padding: 0; list-style: none; color: var(--text-muted); font-size: 11px; overflow-wrap: anywhere; }
  @media (max-width: 900px) {
    .stat-panel .stat { padding-inline: 1px; }
  }
  /* Classic scrollbars reduce the card width independently of the viewport.
     Size the complete value together, allowing for cell padding and glyph rounding;
     min() preserves the hero's smaller narrow-rail size until space requires less. */
  @container rail-stats (max-width: 196px) {
    dd { font-size: min(var(--rail-stat-size, 14px), 13px); }
  }
  @container rail-stats (max-width: 184px) {
    dd { font-size: min(var(--rail-stat-size, 14px), 12px); }
  }
  @container rail-stats (max-width: 172px) {
    dd { font-size: min(var(--rail-stat-size, 14px), 11px); }
  }
  /* No step below 11px: with thin rail scrollbars a scrolled 900px rail (155px grid) still fits 11px. */
</style>

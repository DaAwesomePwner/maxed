<script lang="ts">
  import { settingsHref } from '../navigation/routes';
  import type { DungeonPops } from '../bindings/DungeonPops';
  import type { TimeRange } from '../bindings/TimeRange';
  import { presetFor } from '../runs/runs-list';
  import { rangeLabel } from '../loot/loot-format';
  import './glyphs.css';
  let { rows, muted, filtered = new Set<string>(), selected, range, onSelect }: {
    rows: DungeonPops[]; muted: Set<string>; filtered?: Set<string>; selected: string | null; range: TimeRange | null; onSelect: (dungeon: string) => void;
  } = $props();
  const labels = { all: 'All time', '30d': '30 days', '7d': '7 days', today: 'Today' };
  let preset = $derived(presetFor(range, new Date()));
  let label = $derived(preset ? labels[preset] : range ? rangeLabel(range) : 'All time');
  let filteredCount = $derived(rows.filter(row => filtered.has(row.dungeon)).length);
  let max = $derived(Math.max(1, ...rows.map(row => row.pops)));
</script>
<section class="card" aria-label="Dungeons">
  <header><h2>Dungeons</h2><span>{label}</span></header>
  <div class="ranking">
    {#each rows as row (row.dungeon)}
      <button class="ghost rank" aria-pressed={selected === row.dungeon} onclick={() => onSelect(row.dungeon)} title={row.dungeon}>
        <i class="g portal" aria-hidden="true"></i><span class:muted={muted.has(row.dungeon) && !filtered.has(row.dungeon)}>{row.dungeon}</span>
        <span class="track"><i style:width={`${row.pops / max * 100}%`}></i></span><b>{row.pops.toLocaleString('en-US')}</b>
        {#if filtered.has(row.dungeon)}<small class="filtered">Filtered by rules{muted.has(row.dungeon) ? ' · Muted' : ''}</small>{/if}
      </button>
    {:else}<p>No dungeons in this range.</p>{/each}
  </div>
  {#if filteredCount}<p>{filteredCount} shown dungeon{filteredCount === 1 ? '' : 's'} filtered by rules</p>{/if}
  <footer><span>Struck through: muted for alerts. Marked “Filtered by rules”: excluded by notification rules.</span><a href={settingsHref('notifications')}>Alert settings ›</a></footer>
</section>
<style>
  .card { min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); font-size: var(--text-caption); line-height: var(--leading-caption); }
  header { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-2); margin-bottom: var(--space-2); } h2 { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  header span, p { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); } p { margin: var(--space-2) 0 0; }
  .ranking { max-height: 480px; overflow-y: auto; min-width: 0; }
  .rank { display: grid; grid-template-columns: 16px minmax(0, 1fr) 72px 28px; gap: var(--space-2); align-items: center; min-height: var(--row-compact); width: 100%; padding: 0 var(--space-2); text-align: left; color: var(--text-primary); border: 0; border-radius: var(--radius-tag); background: transparent; cursor: pointer; font: inherit;
    transition: background-color var(--motion-fast) var(--motion-ease); }
  .rank span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .rank b { text-align: right; font-weight: var(--weight-normal); font-variant-numeric: tabular-nums; }
  .filtered { grid-column: 2 / -1; color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta); padding-bottom: var(--space-1); }
  .track { height: 6px; background: var(--bar-track); border-radius: 2px; overflow: hidden; } .track i { display: block; height: 100%; background: var(--bar-party); }
  .rank:hover:enabled { background: var(--surface-hover); }
  .rank[aria-pressed="true"], .rank[aria-pressed="true"]:hover:enabled { background: var(--surface-selected); box-shadow: var(--selected-edge); } .rank[aria-pressed="true"] .track i { background: var(--chart-party); }
  .rank:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; } .muted { text-decoration: line-through; color: var(--text-muted); }
  footer { display: flex; flex-wrap: wrap; gap: var(--space-2); justify-content: space-between; border-top: 1px solid var(--hairline); margin-top: var(--space-2); padding-top: var(--space-2); color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  footer a { color: var(--text-secondary); text-decoration: none; } footer a:hover, footer a:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
</style>

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
  {#if filteredCount}<p>{filteredCount} shown dungeon{filteredCount === 1 ? '' : 's'} filtered by rules.</p>{/if}
  <footer><span>Struck through: muted for alerts. Marked “Filtered by rules”: excluded by notification rules.</span><a href={settingsHref('notifications')}>Alert settings ›</a></footer>
</section>
<style>
  .card { padding: 12px; min-width: 0; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); font-size: 12px; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 8px; } h2 { margin: 0; font-size: 13px; } header span, p { color: var(--text-muted); font-size: 11px; }
  .ranking { max-height: 480px; overflow-y: auto; min-width: 0; }
  .rank { display: grid; grid-template-columns: 20px minmax(0, 1fr) 70px 26px; gap: 8px; align-items: center; min-height: 26px; width: 100%; padding: 0 4px; text-align: left; color: var(--text-primary); border: 0; border-radius: 5px; background: transparent; cursor: pointer; font: inherit; }
  .rank span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .rank b { text-align: right; font-weight: 500; font-variant-numeric: tabular-nums; }
  .filtered { grid-column: 2 / -1; color: var(--violet-text); font-size: 10px; padding-bottom: 4px; }
  .track { height: 6px; background: #00000040; border-radius: 2px; } .track i { display: block; height: 100%; background: var(--violet); border-radius: 2px; }
  .rank[aria-pressed="true"] { background: #f2c14e14; box-shadow: inset 2px 0 var(--gold); } .rank[aria-pressed="true"] .track i { background: var(--gold); } .rank:hover { background: #ffffff08; } .rank:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; } .muted { text-decoration: line-through; color: var(--text-muted); }
  footer { display: flex; flex-wrap: wrap; gap: 6px; justify-content: space-between; border-top: 1px solid var(--border); margin-top: 8px; padding-top: 8px; color: var(--text-muted); font-size: 10px; } a { color: var(--violet-text); text-decoration: none; }
</style>

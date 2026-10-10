<script lang="ts">
  import type { LootStats } from '../bindings/LootStats';
  import type { HomeVisuals } from '../home/visuals';
  import { routeHref } from '../navigation/routes';
  import { bagLabel, STATS_CAPTION } from './presentation';
  import { shortWhen, whiteRate } from './loot-format';
  import { bagTotal, rankShade, sortDungeons, stackSegments, type StatsSort } from './stats-screen';
  import { bagTone } from '../styles/rarity';
  import { spriteUrl } from '../bridge/visuals';
  import SpriteImage from '../components/SpriteImage.svelte';
  let { stats, visuals, ranged, sort, descending, onSort, portals }: {
    stats: LootStats; visuals: HomeVisuals; ranged: boolean;
    sort: StatsSort; descending: boolean; onSort: (key: StatsSort) => void; portals: Map<string, number>;
  } = $props();
  const columns: Array<{ key: StatsSort; label: string }> = [
    { key: 'dungeon', label: 'Dungeon' }, { key: 'runs_ended', label: 'Runs' }, { key: 'clears', label: 'Clears' },
    { key: 'bags', label: 'Bags' }, { key: 'whites', label: 'Whites' }, { key: 'rate', label: 'White rate' },
    { key: 'runs_since_last_white', label: 'Since last white' }, { key: 'best', label: 'Best bag' },
  ];
  const visual = (type: number | null | undefined) => visuals.objects.find(row => row.object_type === type) ?? null;
  const tone = (type: number | null | undefined) => bagTone(visual(type)?.bag ?? null);
  let ranks = $derived([...new Set([
    ...stats.totals, ...stats.dungeons.flatMap(row => row.bags), ...stats.sources.flatMap(row => row.bags),
  ].filter(tier => tier.count > 0).map(tier => tier.bag_rank).filter((rank): rank is number => rank !== null && rank !== 255))]);
  const barColor = (rank: number | null, type: number | null | undefined) => {
    const bag = tone(type);
    return bag === 'unknown' ? rankShade(rank, ranks) : `var(--bag-${bag})`;
  };
  const label = (rank: number | null, type: number | null | undefined) => bagLabel(rank, visual(type));
  let dungeons = $derived(sortDungeons(stats.dungeons, sort, descending));
  let longest = $derived(Math.max(1, ...stats.dungeons.map(row => row.runs_since_last_white ?? 0)));
  let tiers = $derived([...stats.totals].sort((a, b) => (b.bag_rank ?? -1) - (a.bag_rank ?? -1)));
  let maxTier = $derived(Math.max(1, ...tiers.map(tier => tier.count)));
  let sources = $derived([...stats.sources].sort((a, b) => bagTotal(b.bags) - bagTotal(a.bags) || a.source_enemy - b.source_enemy).slice(0, 12));
  const now = new Date();
</script>

{#snippet icon(type: number | null | undefined)}
  {@const sprite = visual(type)?.sprite ?? null}
  <SpriteImage {sprite} sheetUrl={sprite ? spriteUrl(visuals.client_fingerprint, sprite) : null} alt="" fit={20} />
{/snippet}
{#snippet bestBag(rank: number, type: number | null | undefined, at: string)}
  <span class="cell" data-bag-tone={tone(type)} class:white={rank === 255} title={label(rank, type)}>{@render icon(type)}<span>{shortWhen(at, now)}</span></span>
{/snippet}
<div class="stats-grid">
  <section class="card dungeon-card">
    <h3>By dungeon</h3>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (The region supports keyboard scrolling.) -->
    <div class="table-scroll" role="region" aria-label="Dungeon statistics" tabindex="0">
      <table aria-label="By dungeon" class="dungeons">
        <colgroup><col class="name-col" /><col /><col /><col class="bags-col" /><col /><col /><col class="dry-col" /><col class="best-col" /></colgroup>
        <thead><tr>{#each columns as column}
          <th scope="col" aria-sort={sort === column.key ? descending ? 'descending' : 'ascending' : 'none'}>
            <button class="ctl ghost" type="button" onclick={() => onSort(column.key)}>{column.label}{column.key === 'runs_since_last_white' && ranged ? ' (all time)' : ''}{#if sort === column.key}<span aria-hidden="true">{descending ? ' ↓' : ' ↑'}</span>{/if}</button>
          </th>
        {/each}</tr></thead>
        <tbody>{#each dungeons as row (row.dungeon)}
          <tr data-row-id={row.dungeon} onclick={(event) => { if (!(event.target as Element).closest('a, button')) (event.currentTarget.querySelector('.dungeon-link') as HTMLAnchorElement)?.click(); }}>
            <th scope="row"><a class="cell dungeon-link" title={row.dungeon} href={routeHref({ kind: 'loot', view: 'feed', seed: { place: row.dungeon } })}>{@render icon(portals.get(row.dungeon))}<span class="ellipsis">{row.dungeon}</span></a></th>
            <td>{row.runs_ended}</td><td>{row.clears}</td>
            <td><span class="bag-cell"><span class="stack" aria-hidden="true">{#each stackSegments(row.bags) as segment}<i style:width={`${segment.percent}%`} style:background={barColor(segment.rank, segment.bagType)}></i>{/each}</span>{bagTotal(row.bags)}</span></td>
            <td>{row.whites}</td><td>{whiteRate(row.whites, row.runs_ended)}</td>
            <td>{#if row.runs_since_last_white === null}<span class="muted">No whites yet</span>{:else}<span class="dry" class:amber={row.runs_since_last_white >= 20}><i><b style:width={`${row.runs_since_last_white / longest * 100}%`}></b></i><span>{row.runs_since_last_white} {row.runs_since_last_white === 1 ? 'run' : 'runs'}</span></span>{/if}</td>
            <td>{#if row.best}{#if row.best.run_id}<a aria-label={`${label(row.best.bag_rank, row.best.bag_type)} · ${shortWhen(row.best.at, now)}`} href={routeHref({ kind: 'run', id: row.best.run_id })}>{@render bestBag(row.best.bag_rank, row.best.bag_type, row.best.at)}</a>{:else}{@render bestBag(row.best.bag_rank, row.best.bag_type, row.best.at)}{/if}{:else}—{/if}</td>
          </tr>
        {/each}</tbody>
      </table>
    </div>
    <footer>{STATS_CAPTION}</footer>
  </section>
  <aside>
    <section class="card">
      <header><h3>Bags by tier</h3><span>{bagTotal(stats.totals)}</span></header>
      <table aria-label="Bags by tier" class="tiers"><tbody>
        {#each tiers as tier}<tr><th scope="row"><span class="cell" data-bag-tone={tone(tier.bag_type)} class:white={tier.bag_rank === 255}>{@render icon(tier.bag_type)}<span>{label(tier.bag_rank, tier.bag_type)}</span></span></th><td><span class="tier-bar"><i style:width={`${tier.count / maxTier * 100}%`} style:background={barColor(tier.bag_rank, tier.bag_type)}></i></span></td><td>{tier.count}</td></tr>{/each}
        {#if !tiers.some(tier => tier.bag_rank === null)}<tr><th scope="row">Unknown tier</th><td></td><td>0</td></tr>{/if}
      </tbody></table>
    </section>
    <section class="card source-card">
      <header><h3>By source</h3><span>top {sources.length} of {stats.sources.length}</span></header>
      <table aria-label="By source"><thead><tr><th>Source</th><th>Bags</th><th>Whites</th><th>Rate</th></tr></thead><tbody>
        {#each sources as row}{@const name = visual(row.source_enemy)?.name ?? `#${row.source_enemy}`}<tr><th scope="row"><span class="cell" title={`${name} · ${row.dungeon ?? 'Outside a run'}`}>{@render icon(row.source_enemy)}<span class="ellipsis">{name}</span></span></th><td>{bagTotal(row.bags)}</td><td>{row.whites}</td><td><span title={row.runs_ended === null ? 'Outside a run: counts only' : undefined}>{whiteRate(row.whites, row.runs_ended)}</span></td></tr>{/each}
      </tbody></table>
    </section>
  </aside>
</div>
<style>
  .stats-grid { display: grid; grid-template-columns: minmax(0, 1fr) 330px; gap: 10px; min-width: 0; font-size: 12px; }
  .card { min-width: 0; border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px; background: var(--surface-panel); }
  .dungeon-card { display: flex; flex-direction: column; height: min(740px, calc(100vh - 220px)); min-height: 300px; padding: 10px 6px 6px; box-sizing: border-box; }
  h3 { margin: 0; color: var(--text-muted); font-size: 11px; text-transform: uppercase; letter-spacing: .07em; font-weight: 600; }
  .dungeon-card > h3 { margin: 0 6px 8px; }
  header { display: flex; justify-content: space-between; gap: 8px; margin-bottom: 8px; color: var(--text-muted); }
  aside { display: grid; align-content: start; gap: 10px; min-width: 0; padding: 0; border: 0; background: transparent; }
  .table-scroll { overflow: auto; min-height: 0; flex: 1; }
  table { width: 100%; border-collapse: collapse; table-layout: fixed; font-variant-numeric: tabular-nums; }
  .dungeons { min-width: 770px; }
  .name-col { width: 23%; } .bags-col { width: 18%; } .dry-col { width: 18%; } .best-col { width: 12%; }
  th, td { padding: 3px 4px; height: 30px; box-sizing: border-box; border-bottom: 1px solid var(--border); text-align: right; font-weight: normal; }
  th:first-child { text-align: left; } thead th { color: var(--text-secondary); font-size: 11px; background: var(--surface-panel); position: sticky; top: 0; z-index: 1; }
  thead button.ctl { padding: 0; font-size: 11px; height: auto; min-height: 26px; white-space: normal; text-align: inherit; }
  .cell { display: flex; align-items: center; gap: 6px; min-width: 0; } .ellipsis { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  a { color: inherit; text-decoration: none; } a:hover { color: var(--violet-text); }
  tbody tr[data-row-id] { cursor: pointer; } tbody tr[data-row-id]:hover { background: var(--surface-raised); }
  tbody tr[data-row-id]:focus-within { background: #f2c14e14; box-shadow: inset 2px 0 var(--gold); }
  .bag-cell { display: flex; align-items: center; justify-content: end; gap: 8px; } .stack { display: inline-flex; width: 96px; flex-shrink: 1; height: 6px; overflow: hidden; border-radius: 2px; background: var(--surface-raised); } .stack i { height: 100%; }
  .dry { display: flex; align-items: center; gap: 6px; white-space: nowrap; color: var(--text-secondary); } .dry > i { flex: 1; height: 4px; background: var(--surface-raised); } .dry b { display: block; height: 100%; background: var(--text-muted); } .dry.amber { color: var(--estimated); } .dry.amber b { background: var(--estimated); }
  .white { color: var(--text-primary); filter: drop-shadow(0 0 3px #ffffff30); } .muted { color: var(--text-muted); }
  footer { padding: 10px 6px 0; font-size: 11px; color: var(--text-muted); }
  .tiers th { width: 43%; } .tiers td:last-child { width: 40px; } .tiers th, .tiers td { border: 0; }
  .tier-bar { display: block; height: 8px; background: var(--surface-raised); border-radius: 2px; overflow: hidden; } .tier-bar i { display: block; height: 100%; }
  .source-card th:first-child { width: 52%; }
  :is(a, button, [tabindex]):focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  @media (max-width: 1200px) { .stats-grid { grid-template-columns: minmax(0, 1fr); } aside { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); } }
  @media (max-width: 650px) { aside { grid-template-columns: minmax(0, 1fr); } }
</style>

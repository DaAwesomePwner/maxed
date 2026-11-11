<script lang="ts">
  import { itemHover } from '../components/item-hover';
  import type { CollectionCell } from '../bindings/CollectionCell';
  import type { ChaseItem } from '../bindings/ChaseItem';
  import type { HomeVisuals } from '../home/visuals';
  import { spriteUrl } from '../bridge/visuals';
  import { shortWhen, tierClass } from './loot-format';
  import { dungeonList, sortCells, type CollectionSort } from './collection-screen';
  import SpriteImage from '../components/SpriteImage.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  let { dungeon, dungeons, cells, visuals, ready, onChange, chase, sort, onSort, portals, visited = [], visitsReady = false }: {
    dungeon: string | null; dungeons: string[]; visited?: string[]; visitsReady?: boolean; cells: CollectionCell[]; visuals: HomeVisuals;
    ready: boolean; onChange: (dungeon: string | null) => void; chase: ChaseItem[];
    sort: CollectionSort; onSort: (sort: CollectionSort) => void; portals: Map<string, number>;
  } = $props();
  const nameOf = (type: number) => visuals.objects.find(object => object.object_type === type)?.name ?? `#${type}`;
  let choices = $derived(dungeonList(dungeons, dungeon));
  let unvisited = $derived(choices.filter(name => !visited.includes(name)));
  let sorted = $derived(sortCells(cells, sort, nameOf));
</script>
{#snippet choice(name: string)}
  {@const visual = visuals.objects.find(object => object.object_type === portals.get(name))}
  <button type="button" class="ctl ghost" aria-pressed={dungeon === name} title={name} onclick={() => onChange(name)}>
    <SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={24} alt="" /><span>{name}</span>
  </button>
{/snippet}
<div class="card-head"><h3>UT and ST items seen by dungeon</h3><span class="note">Only items Maxed has seen drop</span>
  <label>Sort <select class="ctl" value={sort} onchange={event => onSort(event.currentTarget.value as CollectionSort)}>
    <option value="seen">Most seen</option><option value="recent">Recent</option><option value="name">Name</option>
  </select></label>
</div>
<div class="body">
  <nav aria-label="Collection dungeon">
    {#each visited as name}{@render choice(name)}{/each}
    {#if unvisited.length}<details open={dungeon !== null && unvisited.includes(dungeon)}><summary title={visitsReady ? 'No observed bags or ended runs recorded by Maxed.' : undefined}>{visitsReady ? 'Not visited' : 'All dungeons'} ({unvisited.length})</summary>
      {#each unvisited as name}{@render choice(name)}{/each}
    </details>{/if}
  </nav>
  <div class="items">
    {#if ready}
      {#if dungeon === null}<EmptyState heading="Choose a dungeon" description="" />
      {:else if !cells.length}<EmptyState heading={`No UT or ST items seen from ${dungeon} yet`} description="" />
      {:else}
        <ul aria-label="Observed items">
          {#each sorted as cell (cell.item_type)}
            {@const visual = visuals.objects.find(object => object.object_type === cell.item_type)}
            <li><a use:itemHover={{ type: cell.item_type, name: nameOf(cell.item_type), tier: cell.tier }} href={`#/loot/item/${cell.item_type}`} data-row-id={`collection-${cell.item_type}`}>
              <span class="tile {tierClass(cell.tier)}"><SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={24} alt="" /></span>
              <span class="name {tierClass(cell.tier)}">{nameOf(cell.item_type)}</span>
              {#if chase.some(item => item.item_type === cell.item_type)}<span class="star" aria-label="Chased item" title="Chased item">★</span>{/if}
              <span class="count">×{cell.count}</span><span class="last">{shortWhen(cell.last_at, new Date())}</span>
            </a></li>
          {/each}
        </ul>
      {/if}
    {/if}
  </div>
</div>
<style>
  .card-head { display: flex; align-items: center; gap: var(--space-3); margin-bottom: var(--space-3); min-width: 0; }
  h3 { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); color: var(--text-primary); }
  .note, label { color: var(--text-secondary); font-size: var(--text-meta); }
  label { margin-left: auto; display: flex; align-items: center; gap: var(--space-2); flex: none; }
  .body { display: grid; grid-template-columns: 232px minmax(0, 1fr); gap: var(--space-4); min-width: 0; }
  nav { max-height: 432px; overflow: auto; border-right: 1px solid var(--hairline); padding-right: var(--space-3); }
  summary { padding-block: var(--space-2); font-size: var(--text-caption); color: var(--text-secondary); cursor: pointer; }
  nav button { display: flex; width: 100%; height: var(--control-dense); gap: var(--space-2); justify-content: start; }
  nav button span, .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  nav button[aria-pressed="true"], nav button[aria-pressed="true"]:hover:not(:disabled) { background: var(--surface-selected); color: var(--text-primary); box-shadow: var(--selected-edge); }
  .items { min-width: 0; max-height: 432px; overflow: auto; }
  ul { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: var(--space-4); list-style: none; margin: 0; padding: 0; }
  li { min-width: 0; }
  a { box-sizing: border-box; height: var(--row-default); display: flex; align-items: center; gap: var(--space-2); padding: 0 var(--space-1); border-top: 1px solid var(--hairline); color: var(--text-primary); text-decoration: none; font-size: var(--text-caption);
    transition: background-color var(--motion-fast) var(--motion-ease); }
  a:hover { background: var(--surface-hover); }
  a:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .tile { display: inline-flex; flex: none; border: 1px solid currentColor; border-radius: var(--radius-sprite); }
  .name { flex: 1; font-weight: var(--weight-medium); }
  .star { color: var(--gold); } .ut { color: var(--tier-ut); } .st { color: var(--tier-st); }
  .count, .last { flex: none; font-variant-numeric: tabular-nums; }
  .last { color: var(--text-secondary); font-size: var(--text-meta); min-width: 48px; text-align: right; }
  @media (max-width: 1000px) { .body { grid-template-columns: 172px minmax(0, 1fr); gap: var(--space-3); } .note { display: none; } }
</style>

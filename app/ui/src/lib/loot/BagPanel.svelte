<script lang="ts">
  import { debugMode } from '../debug/debug';
  import { reasonLabel } from '../home/evidence';
  import type { LootDropRow } from '../bindings/LootDropRow';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { HomeVisuals } from '../home/visuals';
  import type { RunStory } from '../bindings/RunStory';
  import ItemTile from '../components/ItemTile.svelte';
  import SpriteImage from '../components/SpriteImage.svelte';
  import BagList from '../home/v3/BagList.svelte';
  import { routeHref } from '../navigation/routes';
  import { bagLabel, sourceLabel, placeLabel, enchantLabel } from './presentation';
  import { tierClass, clockWhen } from './loot-format';
  import { bagTone } from '../styles/rarity';
  let { row, visuals, resolveSpriteUrl, story = null, now }: {
    row: LootDropRow; visuals: HomeVisuals; resolveSpriteUrl: (s: SpriteRef) => string | null; story?: RunStory | null; now: Date;
  } = $props();
  const visual = (type: number | null) => visuals.objects.find(v => v.object_type === type) ?? null;
  let bag = $derived(visual(row.drop.bag_type));
  let source = $derived(sourceLabel(row.drop, visual(row.drop.source_enemy)?.name ?? null));
</script>
<aside aria-label="Selected bag">
  <section class="card">
    <h3>Bag</h3>
    <div class="bag" data-bag-tone={bagTone(bag?.bag ?? null)} class:white={row.drop.bag_rank === 255}>
      <SpriteImage sprite={bag?.sprite ?? null} sheetUrl={bag?.sprite ? resolveSpriteUrl(bag.sprite) : null} fit={40} alt="" />
      <div><strong>{bagLabel(row.drop.bag_rank, bag)}</strong><p title={`${source.text} · ${placeLabel(row.dungeon)}`}>{source.text} · {placeLabel(row.dungeon)}</p><small>{clockWhen(row.drop.at, now)}</small></div>
    </div>
    <ul aria-label="Bag items">
      {#each [...row.drop.contents].sort((a, b) => a.slot - b.slot) as item (`${item.slot}:${item.item_type}`)}
        {@const object = visual(item.item_type)}
        <li class:taken={item.taken_observed_at !== null} title={item.taken_observed_at !== null ? 'Removal observed; who took the item is unknown.' : undefined}>
          <a href={routeHref({ kind: 'item', type: item.item_type })} class={tierClass(object?.item_tier)}>
            <span class="tile"><ItemTile sprite={object?.sprite ?? null} {resolveSpriteUrl} fit={24} alt="" enchant={item.enchant} /></span>
            <span class="name" title={object?.name ?? `#${item.item_type}`}>{object?.name ?? `#${item.item_type}`}<small>{object?.item_tier ?? '—'} · {enchantLabel(item.enchant)}{item.taken_observed_at !== null ? ' · Taken' : ''}</small></span>
          </a>
        </li>
      {/each}
    </ul>
    {#if !row.drop.contents_known}<p>{row.drop.contents.length ? 'partial' : 'contents not seen'}</p>{:else if !row.drop.contents.length}<p>Empty bag</p>{/if}
    <dl><dt>Source</dt><dd title={source.text}>{source.text}{#if source.estimated} <small class="tag" title={$debugMode ? reasonLabel(source.reason) : undefined}>est.</small>{/if}</dd>
      {#if row.drop.run_id}<dt>Run</dt><dd><a href={routeHref({ kind: 'run', id: row.drop.run_id })}>Run ›</a></dd>{/if}
    </dl>
  </section>
  {#if row.drop.run_id && story?.run.id === row.drop.run_id}
    <section class="card other"><h3>Other bags this run</h3><div class="other-list"><BagList drops={story.drops.filter(drop => drop.id !== row.drop.id)} {visuals} {resolveSpriteUrl} scroll selectedDropId={row.drop.id} clock={at => clockWhen(at, now)} /></div></section>
  {/if}
</aside>
<style>
  aside { padding: 0; margin: 0; border: 0; background: none; display: grid; align-content: start; gap: 10px; min-width: 0; }
  .card { min-width: 0; border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px; background: var(--surface-panel); }
  h3 { margin: 0 0 10px; font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-muted); }
  .bag { padding: 8px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-raised); display: flex; gap: 10px; align-items: center; } .bag > div { min-width: 0; }
  .white { color: var(--bag-white); filter: drop-shadow(0 0 3px #ffffff30); }
  p { margin: 4px 0; color: var(--text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  ul { list-style: none; margin: 12px 0; padding: 0; max-height: 280px; overflow-y: auto; }
  li { padding: 5px 0; border-top: 1px solid var(--border); }
  li a { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } small { display: block; color: var(--text-muted); font-size: 11px; }
  .tile { border: 1px solid currentColor; border-radius: 4px; display: inline-flex; }
  .taken { opacity: .45; } .ut { color: var(--violet-text); } .st { color: var(--bag-orange); }
  a { color: inherit; text-decoration: none; } a:focus-visible { outline: 2px solid var(--focus-ring); }
  dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 6px; margin: 10px 0 0; } dt { color: var(--text-muted); } dd { text-align: right; margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  dd small { display: inline; } .tag { margin-left: 4px; border: 1px solid var(--border); border-radius: 3px; padding: 0 3px; } .other-list { height: 220px; overflow: hidden; }
  .other-list :global(.row) { grid-template-columns: 24px minmax(0, 1fr) 100px; }
</style>

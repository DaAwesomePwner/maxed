<script lang="ts">
  import type { ItemHistory } from '../bindings/ItemHistory';
  import type { HomeVisuals } from '../home/visuals';
  import { placeLabel } from './presentation';
  import { shortWhen } from './loot-format';
  import { shareRows } from './item-screen';
  import SpriteImage from '../components/SpriteImage.svelte';
  import { spriteUrl } from '../bridge/visuals';
  import EnchantLabel from './EnchantLabel.svelte';
  let { item, visuals, now }: { item: ItemHistory; visuals: HomeVisuals; now: Date } = $props();
</script>
{#snippet share(count: number, fraction: number)}
  <span class="share"><span class="bar" aria-hidden="true"><i style:width={`${fraction * 100}%`}></i></span>{count}</span>
{/snippet}
<aside>
  <section class="card"><h3>Where</h3><div class="scroll"><table aria-label="Where">
    <colgroup><col /><col style="width: 76px" /><col style="width: 66px" /></colgroup>
    <thead><tr><th>Place</th><th>Seen</th><th>Last</th></tr></thead>
    <tbody>{#each shareRows(item.by_dungeon) as place}<tr>
      <th title={placeLabel(place.dungeon)}>{placeLabel(place.dungeon)}</th><td>{@render share(place.count, place.share)}</td><td title={shortWhen(place.last_at, now)}>{shortWhen(place.last_at, now)}</td>
    </tr>{/each}</tbody>
  </table></div></section>
  <section class="card"><h3>From</h3><div class="scroll"><table aria-label="From"><tbody>
    {#each shareRows(item.by_source) as source}
      {@const name = source.source_enemy === null ? 'Unknown source' : visuals.objects.find(object => object.object_type === source.source_enemy)?.name ?? `#${source.source_enemy}`}
      {@const visual = visuals.objects.find(object => object.object_type === source.source_enemy)}
      <tr><th><div class="source"><SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={20} alt="" /><span title={name}>{name}</span>{#if source.estimated}<small class="tag">estimated</small>{/if}</div></th><td>{@render share(source.count, source.share)}</td></tr>
    {/each}
  </tbody></table></div></section>
  <section class="card"><h3>Enchants</h3><div class="scroll"><table aria-label="Enchants"><tbody>
    {#each shareRows(item.enchant_variants) as variant}<tr><th>
      <EnchantLabel enchant={{ ...variant, slots: null }} />{#if variant.state === 'recorded' && variant.ids.length}<span class="ids" title={variant.ids.map(id => `#${id}`).join(', ')}> ({variant.ids.map(id => `#${id}`).join(', ')})</span>{/if}
    </th><td>{@render share(variant.count, variant.share)}</td></tr>{/each}
  </tbody></table></div></section>
</aside>
<style>
  aside { padding: 0; margin: 0; border: 0; background: none; display: grid; align-content: start; gap: 10px; min-width: 0; }
  .card { padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); min-width: 0; }
  h3 { margin: 0 0 8px; font-size: 11px; font-weight: 500; text-transform: uppercase; letter-spacing: .07em; color: var(--text-secondary); }
  .scroll { max-height: 270px; overflow: auto; }
  table { table-layout: fixed; width: 100%; border-collapse: collapse; font-size: 12px; font-variant-numeric: tabular-nums; }
  th, td { height: 30px; border-bottom: 1px solid var(--border); text-align: left; font-weight: 400; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  thead th { height: 24px; font-size: 11px; color: var(--text-muted); }
  thead th:not(:first-child) { text-align: right; }
  td { width: 76px; text-align: right; } td:last-child { width: 66px; }
  .source { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .source span { overflow: hidden; text-overflow: ellipsis; }
  .tag { flex: none; border: 1px solid var(--border); border-radius: 4px; padding: 0 3px; color: var(--warning); font-size: 10px; }
  .share { display: inline-flex; align-items: center; gap: 6px; }
  .bar { width: 40px; height: 3px; background: var(--surface-raised); } .bar i { display: block; height: 100%; background: var(--violet); }
  .ids { color: var(--text-muted); font-size: 11px; }
</style>

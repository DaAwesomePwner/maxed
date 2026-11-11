<script lang="ts">
  import { itemHover } from '../components/item-hover';
  import type { ItemHistory } from '../bindings/ItemHistory';
  import type { HomeVisuals } from '../home/visuals';
  import { placeLabel } from './presentation';
  import { shortWhen } from './loot-format';
  import { shareRows } from './item-screen';
  import SpriteImage from '../components/SpriteImage.svelte';
  import { spriteUrl } from '../bridge/visuals';
  import Badge from '../components/Badge.svelte';
  import EnchantLabel from './EnchantLabel.svelte';
  let { item, visuals, now }: { item: ItemHistory; visuals: HomeVisuals; now: Date } = $props();
</script>
{#snippet share(count: number, fraction: number)}
  <span class="share"><span class="bar" aria-hidden="true"><i style:width={`${fraction * 100}%`}></i></span>{count}</span>
{/snippet}
<aside>
  <section class="card"><h3>Where</h3><div class="scroll"><table aria-label="Where">
    <colgroup><col /><col style="width: 76px" /><col style="width: 64px" /></colgroup>
    <thead><tr><th>Place</th><th>Seen</th><th>Last</th></tr></thead>
    <tbody>{#each shareRows(item.by_dungeon) as place}<tr>
      <th title={placeLabel(place.dungeon)}>{placeLabel(place.dungeon)}</th><td>{@render share(place.count, place.share)}</td><td title={shortWhen(place.last_at, now)}>{shortWhen(place.last_at, now)}</td>
    </tr>{/each}</tbody>
  </table></div></section>
  <section class="card"><h3>From</h3><div class="scroll"><table aria-label="From"><tbody>
    {#each shareRows(item.by_source) as source}
      {@const name = source.source_enemy === null ? 'Unknown source' : visuals.objects.find(object => object.object_type === source.source_enemy)?.name ?? `#${source.source_enemy}`}
      {@const visual = visuals.objects.find(object => object.object_type === source.source_enemy)}
      <tr><th><div class="source"><SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={20} alt="" /><span title={name}>{name}</span>{#if source.estimated}<Badge label="estimated" tone="warning" />{/if}</div></th><td>{@render share(source.count, source.share)}</td></tr>
    {/each}
  </tbody></table></div></section>
  <section class="card"><h3>Enchants</h3><div class="scroll"><table aria-label="Enchants"><tbody>
    {#each shareRows(item.enchant_variants) as variant}<tr><th use:itemHover={{ type: item.item_type, enchant: { state: variant.state, slots: null, applied: variant.applied, ids: variant.ids } }}>
      <EnchantLabel enchant={{ ...variant, slots: null }} />{#if variant.state === 'recorded' && variant.ids.length}<span class="ids"> ({variant.ids.map(id => `#${id}`).join(', ')})</span>{/if}
    </th><td>{@render share(variant.count, variant.share)}</td></tr>{/each}
  </tbody></table></div></section>
</aside>
<style>
  aside { padding: 0; margin: 0; border: 0; background: none; display: grid; align-content: start; gap: var(--space-3); min-width: 0; }
  .card { min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); font-size: var(--text-caption); line-height: var(--leading-caption); }
  h3 { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); color: var(--text-primary); margin-bottom: var(--space-2); }
  .scroll { max-height: 272px; overflow: auto; }
  table { table-layout: fixed; width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { height: var(--row-compact); border-bottom: 1px solid var(--hairline); text-align: left; font-weight: var(--weight-normal); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  thead th { height: 24px; font-size: var(--text-meta); font-weight: var(--weight-medium); color: var(--text-muted); border-bottom-color: var(--hairline-strong); }
  thead th:not(:first-child) { text-align: right; }
  td { width: 76px; text-align: right; } td:last-child { width: 64px; }
  .source { display: flex; align-items: center; gap: var(--space-1); min-width: 0; }
  .source span { overflow: hidden; text-overflow: ellipsis; }
  .share { display: inline-flex; align-items: center; gap: var(--space-2); }
  .bar { width: 40px; height: 4px; background: var(--bar-track); } .bar i { display: block; height: 100%; background: var(--chart-party); }
  .ids { color: var(--text-muted); font-size: var(--text-meta); }
</style>

<script lang="ts">
  import type { EquipmentView } from '../bindings/EquipmentView';
  import type { GearChange } from '../bindings/GearChange';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { HomeVisuals } from '../home/visuals';
  import ItemTile from '../components/ItemTile.svelte';
  import { itemHover } from '../components/item-hover';
  import { objectVisual } from '../bridge/visual-index';
  import { enchantCount, pipLabel } from '../styles/enchant';
  import { formatClock } from '../styles/format';
  import { timestampMs } from './player-view';
  import './players.css';
  let { equipment, observedAt, swaps, baseline = null, visuals, resolveSpriteUrl, missingText = 'Not recorded for this run' }: {
    missingText?: string; equipment: EquipmentView[] | null; observedAt: number | null; swaps: GearChange[]; visuals: HomeVisuals;
    baseline?: { at: number; equipment: EquipmentView[] } | null;
    resolveSpriteUrl: (sprite: SpriteRef) => string | null;
  } = $props();
  const slots = ['Weapon', 'Ability', 'Armor', 'Ring'];
  const nameFor = (item: EquipmentView | undefined) => item?.item_type == null ? 'Not seen' : objectVisual(visuals.objects, item.item_type)?.name ?? `Item ${item.item_type}`;
  let ordered = $derived([...swaps].sort((a, b) => (timestampMs(a.at) ?? 0) - (timestampMs(b.at) ?? 0)));
  let changes = $derived(ordered.map((row, i) => {
    if (i === 0 && baseline === null) return { at: row.at, label: `Started with ${row.equipment.length ? row.equipment.filter(item => item.slot >= 0 && item.slot < 4).map(nameFor).join(', ') : 'Not seen'}` };
    const previous = i === 0 ? baseline! : ordered[i - 1];
    const changed = slots.flatMap((slot, index) => {
      const before = previous.equipment.find(item => item.slot === index), after = row.equipment.find(item => item.slot === index);
      return JSON.stringify(before ?? null) === JSON.stringify(after ?? null) ? [] : [`${slot} → ${nameFor(after)}`];
    });
    return { at: row.at, label: changed.join(' · ') };
  }).filter(row => row.label));
</script>
<section class="player-card" aria-label="Gear">
  <h3>Gear {#if baseline !== null || observedAt !== null}<span class="player-card-sub">{baseline !== null ? 'at fight start' : 'latest seen'} {formatClock(String(Math.round((baseline?.at ?? observedAt!) * 1000))) ?? '—'}</span>{/if}</h3>
  {#if equipment === null}<p class="player-muted">{missingText}</p>
  {:else}<ul class="player-gear" aria-label="Equipment">
    {#each slots as slot, index}
      {@const item = equipment.find(row => row.slot === index)}
      {@const visual = objectVisual(visuals.objects, item?.item_type)}
      {@const name = nameFor(item)}
      {@const count = enchantCount(item?.enchant)}
      {#snippet gearIdentity()}
        <ItemTile sprite={visual?.sprite ?? null} {resolveSpriteUrl} alt={name} fit={32} enchant={item?.enchant ?? null} />
        <div><b>{name}</b><span>{slot} · {visual?.item_tier ?? '—'}{#if count !== null && count > 0} · <span title={pipLabel(count)}>{count} {count === 1 ? 'enchant' : 'enchants'}</span>{/if}</span></div>
      {/snippet}
      {#if item?.item_type != null}
        <li data-slot={index} use:itemHover={{ type: item.item_type, enchant: item.enchant, name, tier: visual?.item_tier }}>{@render gearIdentity()}</li>
      {:else}<li data-slot={index}>{@render gearIdentity()}</li>{/if}
    {/each}
  </ul>{/if}
  {#if changes.length}<ul class="player-swaps" aria-label="Gear swaps">{#each changes as change}<li><time>{formatClock(change.at) ?? '—'}</time><span>{change.label}</span></li>{/each}</ul>{/if}
</section>

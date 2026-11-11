<script lang="ts">
  import { itemHover } from '../components/item-hover';
  import DebugNote from '../debug/DebugNote.svelte';
  import SpriteImage from '../components/SpriteImage.svelte';
  import ItemTile from '../components/ItemTile.svelte';
  import VitalBar from '../components/VitalBar.svelte';
  import StatPanel from './StatPanel.svelte';
  import { characterMaxed } from './vitals';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { RailView } from './rail';
  let { view, sheetUrl = null, resolveSpriteUrl }: {
    view: RailView; sheetUrl?: string | null; resolveSpriteUrl?: (sprite: SpriteRef) => string | null;
  } = $props();
  const known = (n: number | null | undefined): n is number => typeof n === 'number' && Number.isFinite(n) && n >= 0;
  let maxed = $derived(characterMaxed(view.character));
  let fame = $derived(view.character?.fame);
  let goal = $derived(view.character?.stats.find(s => s.stat_id === 58)?.value);
  let hasGoal = $derived(known(goal) && goal > 0);
  let fameText = $derived(!known(fame) ? '—' : hasGoal ? `${fame}/${goal}` : String(fame));
  let fameFill = $derived(!known(fame) ? 0 : hasGoal ? fame / goal! * 100 : 100);
</script>

<section class="compact-hero" aria-label="Character summary">
  <div class="hero-body" class:stale={view.stale}>
    <div class="hero-top">
      <div class="avatar"><SpriteImage sprite={view.sprite} {sheetUrl} fit={60} alt={view.className} /></div>
      <div class="identity">
        <h2 class="cname" title={view.name}>{view.name}</h2>
        <p class="cclass" title={view.character ? `${view.className} · Lv ${view.character.level ?? '—'}` : undefined}><span>{view.character ? view.className : '—'}</span> · Lv {#if view.character?.level == null}{@render unavailable()}{:else}{view.character.level}{/if}</p>
        <p class="maxed">{#if maxed === null}{@render unavailable()}{:else}{maxed}/8{/if} <span>maxed</span></p>
      </div>
    </div>
    {#if view.character}
      {#if view.stale}<DebugNote text="Last seen" />{/if}
      <ul class="equipment" aria-label="Equipment">
        {#each view.equipment as item (item.slot)}
          {@const name = item.name ?? item.label}
          {#if item.itemType !== null}
            <li data-slot={item.slot} data-item-type={item.itemType} use:itemHover={{ type: item.itemType, enchant: item.enchant, name: item.name ?? undefined }}>
              <ItemTile sprite={item.sprite} {resolveSpriteUrl} fit={32} alt={name} enchant={item.enchant} />
            </li>
          {:else}
            <li data-slot={item.slot} title={name}><ItemTile sprite={item.sprite} {resolveSpriteUrl} fit={32} alt={name} enchant={item.enchant} /></li>
          {/if}
        {/each}
      </ul>
      <VitalBar kind="fame" label="Fame" text={fameText} fill={fameFill} tone="max" />
      <StatPanel panel={view.character.stat_panel} stale={view.stale} />
      {#if view.provisional}<DebugNote text="Unverified identity" />{/if}
      {#if view.died}<p class="death">Death</p>{/if}
    {/if}
  </div>
</section>
{#snippet unavailable()}<span aria-hidden="true">—</span><span class="sr-only">Unavailable</span>{/snippet}

<style>
  .compact-hero { flex: 0 0 auto; min-width: 0; border: 1px solid var(--hairline); border-radius: var(--radius-card); padding: var(--space-3); background: var(--surface-panel); --rail-vital-size: 12px; --rail-label-size: 12px; --rail-stat-size: 14px; }
  .hero-body { display: grid; gap: var(--space-2); min-width: 0; }
  .hero-top { display: grid; grid-template-columns: 60px minmax(0, 1fr); gap: var(--space-2); align-items: center; }
  .avatar { width: 60px; height: 60px; display: grid; place-items: center; background: var(--surface-inset); border-radius: var(--radius-control); }
  .identity { min-width: 0; }
  .cname { overflow: hidden; text-overflow: ellipsis; margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-semibold); white-space: nowrap; }
  p { margin: 0; }
  .cclass { display: flex; min-width: 0; overflow: hidden; color: var(--text-secondary); font-size: var(--text-meta); line-height: var(--leading-meta); white-space: pre; }
  .cclass > span:first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .maxed { color: var(--gold); font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .maxed span { font-weight: var(--weight-normal); font-size: var(--text-meta); color: var(--text-secondary); }
  .equipment { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--space-1); margin: 0; padding: 0; list-style: none; }
  .equipment li { min-width: 0; height: var(--row-min); display: grid; place-items: center; background: var(--surface-inset); border: 1px solid var(--hairline); border-radius: var(--radius-sprite); }
  .stale { opacity: .7; }
  .death { color: var(--danger); font-size: var(--text-meta); }
  @media (max-width: 900px) {
    .compact-hero { padding: var(--space-1); --rail-vital-size: 12px; --rail-label-size: 12px; --rail-stat-size: 12px; }
    .hero-top { gap: var(--space-1); }
    .cname { font-size: var(--text-meta); line-height: var(--leading-meta); }
  }
</style>

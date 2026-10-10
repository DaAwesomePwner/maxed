<script lang="ts">
  import { untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { DropFilter } from '../bindings/DropFilter';
  import ItemSearch from './ItemSearch.svelte';
  import type { HomeVisuals } from '../home/visuals';
  import { bagLabel } from './presentation';
  let { client, filter, visuals, enabled = true, onApply }: {
    client?: Client; filter: DropFilter; visuals: HomeVisuals; enabled?: boolean;
    onApply: (filter: DropFilter) => void;
  } = $props();
  let rank = $derived(filter.min_rank === null ? '' : String(filter.min_rank));
  let place = $derived(filter.place?.kind === 'outside' ? 'outside' : filter.place?.kind === 'dungeon' ? `dungeon:${filter.place.name}` : '');
  let itemType = $derived(filter.item_type);
  let chosenName = $state<string | null>(null);
  let dungeons = $state<string[]>([]), catalogError = $state('');
  let catalogRevision = $state(0);
  let knownRanks = $state.raw<ReadonlyMap<number, string>>(new Map());
  let rankFingerprint = '';
  let chosen = $derived(chosenName ?? visuals.objects.find(row => row.object_type === itemType)?.name ?? `#${itemType}`);
  $effect(() => {
    if (visuals.phase !== 'ready' || !visuals.client_fingerprint) return;
    // Keep discovered catalog tiers across paging and empty filtered results.
    // The u8 rank bounds this map to at most 256 entries.
    const known = new Map(visuals.client_fingerprint === rankFingerprint ? untrack(() => knownRanks) : []);
    rankFingerprint = visuals.client_fingerprint;
    for (const visual of visuals.objects) if (visual.bag?.rank != null) known.set(visual.bag.rank, bagLabel(visual.bag.rank, visual));
    knownRanks = known;
  });
  let ranks = $derived.by(() => {
    const known = new Map(knownRanks);
    if (!known.has(255)) known.set(255, bagLabel(255, null));
    // Keep the restored selection usable even when this page has no bag visuals.
    if (rank && !known.has(Number(rank))) known.set(Number(rank), bagLabel(Number(rank), null));
    return [...known].sort(([a], [b]) => a - b);
  });
  let places = $derived([...new Set([...dungeons, ...(place.startsWith('dungeon:') ? [place.slice(8)] : [])])]);
  $effect(() => {
    const owner = client; catalogRevision;
    if (!owner || !enabled) return;
    let current = true;
    catalogError = '';
    void owner.listCatalogDungeons().then(value => { if (current) dungeons = value; }, () => {
      if (current) catalogError = 'Dungeon choices unavailable.';
    });
    return () => { current = false; };
  });
  function change(patch: Partial<DropFilter>) { onApply({ ...filter, ...patch, cursor: null }); }
</script>
<div class="toolbar" aria-label="Loot filters">
  {#if itemType !== null}<button class="ctl" type="button" aria-label="Clear item" title={chosen} onclick={() => { chosenName = null; change({ item_type: null }); }}>{chosen} ×</button>
  {:else if client}<ItemSearch {client} {enabled} placeholder="Find an item" onPick={item => { chosenName = item.name; change({ item_type: item.object_type }); }} />{/if}
  <select class="ctl" aria-label="Minimum bag tier" value={rank} onchange={event => change({ min_rank: event.currentTarget.value === '' ? null : Number(event.currentTarget.value) })}><option value="">Any bag</option>{#each ranks as [value, label]}<option value={String(value)}>{label}</option>{/each}</select>
  <select class="ctl" aria-label="Place" value={place} onchange={event => { const value = event.currentTarget.value; change({ place: value === '' ? null : value === 'outside' ? { kind: 'outside' } : { kind: 'dungeon', name: value.slice(8) } }); }}><option value="">Any place</option><option value="outside">Outside a run</option>{#each places as name}<option value={`dungeon:${name}`}>{name}</option>{/each}</select>
  <button class="ctl" type="button" aria-pressed={filter.enchanted_only} onclick={() => change({ enchanted_only: !filter.enchanted_only })}>Enchanted only</button>
  {#if catalogError}<span>{catalogError} <button class="ctl" type="button" onclick={() => catalogRevision++}>Retry dungeon choices</button></span>{/if}
</div>
<style>
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; min-width: 0; }
  select, button { max-width: 230px; overflow: hidden; text-overflow: ellipsis; }
  [aria-pressed="true"] { border-color: var(--gold); color: var(--gold); }
</style>

<script lang="ts">
  import { closeItemHover, itemHover, openItemHover } from '../components/item-hover';
  import { onDestroy, type Snippet } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { ItemMatch } from '../bindings/ItemMatch';
  import ItemTile from '../components/ItemTile.svelte';
  import { createHomeVisuals, emptyHomeVisuals } from '../home/visuals';
  import { spriteUrl } from '../bridge/visuals';
  import { tierClass } from './loot-format';
  let { client, enabled = true, placeholder, onPick, resultExtra }: {
    client: Client; enabled?: boolean; placeholder: string; onPick?: (item: ItemMatch) => void; resultExtra?: Snippet<[ItemMatch]>;
  } = $props();
  const id = $props.id();
  let query = $state(''), searchMessage = $state('');
  let results = $state<ItemMatch[]>([]), active = $state(-1), searching = $state(false);
  let searchRevision = 0;
  let searchInput: HTMLInputElement;
  let listbox = $state<HTMLUListElement>();
  function closeHover() { if (listbox) closeItemHover(listbox); }
  onDestroy(closeHover);
  let visuals = $state.raw(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  $effect(() => {
    if (!enabled) return;
    const instance = createHomeVisuals(client, next => { visuals = next; });
    visualController = instance;
    return () => { instance.dispose(); visualController = undefined; };
  });
  $effect(() => { visualController?.update(results.map(item => item.object_type)); });
  $effect(() => {
    client; enabled;
    return () => { searchRevision++; };
  });
  function clearResults() { closeHover(); clearTimeout(timer); searchRevision++; results = []; active = -1; searching = false; searchMessage = ''; }
  let timer: ReturnType<typeof setTimeout> | undefined;
  function input() { clearTimeout(timer); clearResults(); timer = setTimeout(() => void search(), 250); }
  $effect(() => () => clearTimeout(timer));
  async function search() {
    clearTimeout(timer);
    clearResults();
    const text = query.trim();
    if (!text || [...text].length > 64) { searchMessage = 'Enter 1–64 characters to search'; return; }
    if (!client) { searchMessage = 'Item search unavailable'; return; }
    if (!enabled) { searchMessage = 'Item search unavailable. Press Enter to retry.'; return; }
    const request = searchRevision;
    searching = true;
    try {
      const matches = await client.searchItems(text, 10);
      if (request !== searchRevision) return;
      results = matches; searchMessage = matches.length ? '' : 'No matching items';
    } catch {
      if (request === searchRevision) searchMessage = 'Item search unavailable. Press Enter to retry.';
    } finally { if (request === searchRevision) searching = false; }
  }
  function choose(item: ItemMatch) {
    if (!onPick) return;
    query = ''; clearResults(); onPick(item); searchInput?.focus();
  }
  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') { query = ''; clearResults(); return; }
    if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && results.length) {
      event.preventDefault();
      active = event.key === 'ArrowDown' ? (active + 1) % results.length : (active < 0 ? results.length - 1 : (active + results.length - 1) % results.length);
      const anchor = listbox?.children[active], item = results[active];
      if (anchor instanceof HTMLElement) openItemHover(anchor, { type: item.object_type, name: item.name, tier: item.tier });
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (active >= 0 && results[active]) choose(results[active]); else void search();
    }
  }
</script>
<div class="search">
  <input id={`${id}-search`} bind:this={searchInput} bind:value={query} oninput={input} onkeydown={keydown} onblur={closeHover}
    role="combobox" aria-autocomplete="list" aria-expanded={results.length > 0} aria-controls={results.length ? `${id}-results` : undefined}
    aria-activedescendant={active >= 0 ? `${id}-option-${active}` : undefined} {placeholder} aria-label={placeholder} class="ctl" autocomplete="off" />
  {#if results.length}
    <ul id={`${id}-results`} role="listbox" aria-label="Item results" bind:this={listbox}>
      {#each results as item, index (item.object_type)}
        <li id={`${id}-option-${index}`} role="option" aria-selected={active === index} tabindex="-1"
          onclick={event => { if (!(event.target instanceof Element && event.target.closest('button, select, input, a'))) choose(item); }} onkeydown={event => { if (event.target !== event.currentTarget) return; if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); choose(item); } }}>
          <span class="tile {tierClass(item.tier)}" use:itemHover={{ type: item.object_type, name: item.name, tier: item.tier }}><ItemTile sprite={visuals.objects.find(v => v.object_type === item.object_type)?.sprite ?? null} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} fit={24} alt="" /></span>
          <span class={tierClass(item.tier)} use:itemHover={{ type: item.object_type, name: item.name, tier: item.tier }}>{item.name}</span>{#if item.tier}<small class={tierClass(item.tier)}>{item.tier}</small>{/if}
          {#if resultExtra}{@render resultExtra(item)}{/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if searching || searchMessage}<p role="status">{searching ? 'Searching items' : searchMessage}</p>{/if}
</div>
<style>
  .search { position: relative; width: 232px; max-width: 100%; min-width: 0; }
  input { width: 100%; box-sizing: border-box; }
  ul { position: absolute; top: calc(var(--control-dense) + var(--space-1)); left: 0; width: min(400px, 75vw); z-index: 5; margin: 0; padding: var(--space-1); list-style: none; border: 1px solid var(--hairline-strong); border-radius: var(--radius-control); background: var(--surface-raised); box-shadow: var(--elevation-popover); font-size: var(--text-caption); }
  li { display: flex; gap: var(--space-2); align-items: center; padding: var(--space-1); border-radius: var(--radius-tag); cursor: pointer; }
  li > span:not(.tile) { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  li:hover, [aria-selected="true"] { background: var(--surface-hover); }
  small { font-size: var(--text-meta); }
  .tile { flex: none; display: inline-flex; border: 1px solid currentColor; border-radius: var(--radius-sprite); }
  .ut { color: var(--tier-ut); } .st { color: var(--tier-st); }
  p { margin: var(--space-1) 0; font-size: var(--text-meta); color: var(--text-secondary); }
</style>

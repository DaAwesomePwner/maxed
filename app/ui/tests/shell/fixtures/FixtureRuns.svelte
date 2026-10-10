<script lang="ts">
  import { untrack } from 'svelte';
  import type { Router, NavigationState } from '../../../src/lib/navigation/history';
  import type { RunView } from '../../../src/lib/bindings/RunView';
  import type { RunSort } from '../../../src/lib/bindings/RunSort';
  import { routeHref } from '../../../src/lib/navigation/routes';
  import ListRow from '../../../src/lib/components/ListRow.svelte';
  let { navigation, router, runs }: { navigation: NavigationState; router: Router; runs: RunView[] } = $props();
  let dungeon = $state(untrack(() => navigation.entry.filters.dungeon ?? ''));
  let sort = $state<RunSort>(untrack(() => navigation.entry.filters.sort));
  let shown = $derived(runs.filter((run) => !dungeon || run.dungeon.toLowerCase().includes(dungeon.toLowerCase())));
  let ordered = $derived(sort === 'oldest' ? [...shown].reverse() : shown);
  function saveFilters() { router.save({ filters: { ...navigation.entry.filters, dungeon: dungeon || null, sort } }); }
</script>

<p>Synthetic navigation fixture. These rows exercise history restoration; they are not saved gameplay.</p>
<div class="filters">
  <label>Dungeon <input value={dungeon} oninput={(event) => { dungeon = event.currentTarget.value; saveFilters(); }} /></label>
  <label>Order <select value={sort} onchange={(event) => { sort = event.currentTarget.value as RunSort; saveFilters(); }}><option value="newest">Newest</option><option value="oldest">Oldest</option></select></label>
</div>
<div aria-label="Synthetic runs">
  {#each ordered as run (run.id)}
    <div data-row-id={run.id}>
      <ListRow label={run.dungeon} href={routeHref({ kind: 'run', id: run.id })}><span>{run.dungeon}</span><span>{run.outcome ?? 'Outcome unavailable'}</span></ListRow>
    </div>
  {/each}
  {#if !ordered.length}<p>No synthetic runs match this filter.</p>{/if}
</div>

<style>
  .filters { display: flex; flex-wrap: wrap; gap: var(--space-4); margin: var(--space-6) 0; }
  label { display: grid; gap: var(--space-2); }
  input, select { min-height: var(--target-min); max-width: 100%; color: var(--text-primary); background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-control); padding: var(--space-2); font: inherit; }
</style>

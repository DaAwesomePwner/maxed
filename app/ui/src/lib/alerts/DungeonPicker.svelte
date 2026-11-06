<script lang="ts">
  import { untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { AlertPreferences } from '../bindings/AlertPreferences';
  import { createDungeonPortalSprite, dungeonPortalSpriteFromPage, type DungeonPortalSprite } from '../bridge/dungeon-portal';
  import type { DungeonDef } from '../bindings/DungeonDef';
  import { keyPopFilterWithinLimit, keyPopFilterLimitMessage } from './preferences';
  import DungeonPortal from './DungeonPortal.svelte';
  let { client, preferences, disabled, onEdit }: {
    client?: Client; preferences: AlertPreferences; disabled: boolean;
    onEdit: (patch: Partial<AlertPreferences>) => void;
  } = $props();
  type Mode = 'all' | 'except' | 'selected';
  const modeOf = (p: AlertPreferences): Mode => p.key_pop_dungeons !== null ? 'selected'
    : p.key_pop_excluded_dungeons.length ? 'except' : 'all';
  let mode = $state<Mode>(untrack(() => modeOf(preferences)));
  let lastOnly: string[] = [], lastExcept = untrack(() => [...preferences.key_pop_excluded_dungeons]);
  let expected: Partial<AlertPreferences> | null = null;
  $effect(() => {
    const p = preferences;
    // Keep an explicitly chosen empty All except editor open; external saves/revert still win.
    if (p.key_pop_dungeons !== expected?.key_pop_dungeons || p.key_pop_excluded_dungeons !== expected?.key_pop_excluded_dungeons) {
      mode = modeOf(p);
    }
  });
  let query = $state(''), message = $state(''), limitMessage = $state('');
  let catalog = $state<DungeonDef[]>([]);
  let catalogStatus = $state<'loading' | 'ready' | 'failed'>('loading');
  let portals = $state.raw(new Map<string, DungeonPortalSprite | null>());
  let fallbackPortals = $state.raw(new Map<string, DungeonPortalSprite | null>());
  $effect(() => {
    const owner = client;
    if (!owner) return;
    let disposed = false, revision = 0;
    async function load(restarts = 0): Promise<void> {
      const request = ++revision;
      catalogStatus = 'loading'; portals = new Map();
      try {
        const rows = await owner!.listCatalogDungeonOptions();
        if (disposed || request !== revision) return;
        catalog = rows; catalogStatus = 'ready'; message = '';
        const types = [...new Set(rows.map(row => row.portal_type)
          .filter((type): type is number => type != null && Number.isInteger(type) && type >= 0 && type <= 65535))];
        if (types.length) {
          try {
            const byType = new Map<number, DungeonPortalSprite | null>();
            const names = new Map(rows.map(row => [row.portal_type, row.name]));
            let fingerprint: string | undefined;
            // The visuals API accepts at most 128 types. Publish only the complete current load.
            // Each cache lookup can make a fingerprint probe plus a data fetch (at most
            // two native calls per chunk). With the final probe below, a stable cold
            // 166-type load sends sizes [128, 0, 38, 0], plus two catalog reads total.
            for (let offset = 0; offset < types.length; offset += 128) {
              const chunk = types.slice(offset, offset + 128);
              const page = await owner!.getObjectVisuals(chunk);
              if (disposed || request !== revision) return;
              if (fingerprint !== undefined && page.client_fingerprint !== fingerprint) {
                // Native extraction can replace the catalog between chunks without a UI event.
                // Discard this map and retry at most twice; continual changes leave names only.
                if (restarts < 2) await load(restarts + 1);
                return;
              }
              fingerprint = page.client_fingerprint;
              for (const type of chunk) byType.set(type, dungeonPortalSpriteFromPage(page, type, names.get(type)!));
            }
            // One additional catalog read binds these sprites to the names/types we loaded.
            // Extraction may have committed after the initial list but before the first page.
            const confirmed = await owner!.listCatalogDungeonOptions();
            if (disposed || request !== revision) return;
            const catalogKey = (definitions: DungeonDef[]) => JSON.stringify(definitions.map(row => [row.name, row.aliases, row.portal_type]));
            if (catalogKey(confirmed) !== catalogKey(rows)) {
              if (restarts < 2) await load(restarts + 1);
              else catalog = confirmed;
              return;
            }
            // Identical dungeon rows can belong to a new sprite generation.
            const current = await owner!.getObjectVisuals([]);
            if (disposed || request !== revision) return;
            if (current.client_fingerprint !== fingerprint) {
              if (restarts < 2) await load(restarts + 1);
              return;
            }
            portals = new Map(rows.map(row => [row.name, row.portal_type == null ? null : byType.get(row.portal_type) ?? null]));
          } catch { /* The catalog remains searchable when artwork is unavailable. */ }
        }
      } catch {
        if (!disposed && request === revision) { catalog = []; catalogStatus = 'failed'; message = 'Dungeon catalog unavailable. Check assets in Settings.'; }
      }
    }
    void load();
    const release = owner.onVisualsChanged(() => { void load(); });
    return () => { disposed = true; release(); };
  });
  let selected = $derived([...new Set(mode === 'selected' ? preferences.key_pop_dungeons ?? [] : preferences.key_pop_excluded_dungeons)]);
  $effect(() => {
    const owner = client;
    const missing = catalogStatus === 'ready' ? selected.filter(name => !catalog.some(row => row.name === name)) : [];
    fallbackPortals = new Map();
    if (!owner) return;
    const lookups = missing.map(name => {
      const lookup = createDungeonPortalSprite(owner, value => { untrack(() => { fallbackPortals = new Map(fallbackPortals).set(name, value); }); });
      lookup.update(name);
      return lookup;
    });
    return () => { for (const lookup of lookups) lookup.dispose(); };
  });
  let visible = $derived.by(() => {
    const names = [...new Set([...catalog.map(row => row.name), ...selected])].sort();
    const term = query.trim().toLocaleLowerCase();
    return names.filter(name => [name, ...(catalog.find(row => row.name === name)?.aliases ?? [])]
      .some(value => value.toLocaleLowerCase().includes(term)));
  });
  function edit(patch: Partial<AlertPreferences>): boolean {
    const next = { ...preferences, ...patch };
    if (!keyPopFilterWithinLimit(next)) { limitMessage = keyPopFilterLimitMessage; return false; }
    limitMessage = ''; expected = next;
    onEdit(patch);
    return true;
  }
  function switchMode(next: Mode) {
    if (mode === 'selected') lastOnly = [...selected];
    if (mode === 'except') lastExcept = [...selected];
    if (edit({ key_pop_dungeons: next === 'selected' ? lastOnly : null,
      key_pop_excluded_dungeons: next === 'all' ? [] : next === 'except' ? lastExcept : preferences.key_pop_excluded_dungeons })) mode = next;
  }
  function select(names: string[]): boolean {
    return edit(mode === 'selected' ? { key_pop_dungeons: names } : { key_pop_excluded_dungeons: names });
  }
</script>
<label for="key-pop-dungeons">Key Popped dungeons</label>
<select id="key-pop-dungeons" value={mode} {disabled} onchange={event => { switchMode(event.currentTarget.value as Mode); event.currentTarget.value = mode; }}>
  <option value="all">All dungeons</option><option value="except">All except…</option><option value="selected">Only…</option>
</select>
{#if limitMessage}<p role="alert">{limitMessage}</p>{/if}
{#if mode !== 'all'}
  <fieldset {disabled}>
    <legend>{mode === 'except' ? 'Muted dungeons' : 'Chosen dungeons'}</legend>
    {#if mode === 'selected' && selected.length === 0}<p class="note">Choose dungeons to receive key-pop alerts. None selected means no alerts.</p>{/if}
    <div class="chips" aria-label="Selected dungeons">
      {#each selected as dungeon (dungeon)}<button type="button" aria-label={`Remove ${dungeon}`} onclick={() => select(selected.filter(name => name !== dungeon))}><DungeonPortal portal={portals.get(dungeon) ?? fallbackPortals.get(dungeon) ?? null} />{dungeon} ×</button>{/each}
    </div>
    <label for="dungeon-search">Search dungeons</label>
    <input id="dungeon-search" type="search" bind:value={query} />
    <div class="actions"><button type="button" onclick={() => select([...new Set([...selected, ...visible])])}>Select all</button><button type="button" onclick={() => select(selected.filter(name => !visible.includes(name)))}>Clear</button></div>
    <div class="results">
      {#each visible as dungeon (dungeon)}
        <label class="choice"><input type="checkbox" checked={selected.includes(dungeon)} onchange={event => { if (!select(event.currentTarget.checked ? [...selected, dungeon] : selected.filter(name => name !== dungeon))) event.currentTarget.checked = selected.includes(dungeon); }} /><DungeonPortal portal={portals.get(dungeon) ?? fallbackPortals.get(dungeon) ?? null} />{dungeon}</label>
      {/each}
      {#if !visible.length}<p>No matching dungeons.</p>{/if}
    </div>
    {#if message}<p>{message}</p>{/if}
  </fieldset>
{/if}
<style>
  fieldset { min-width: 0; padding: 12px; border-radius: 6px; border: 1px solid var(--border); margin: var(--space-2) 0; }
  .results { max-height: 14rem; overflow: auto; margin-top: var(--space-2); }
  .choice, .chips button { display: flex; align-items: center; gap: var(--space-1); min-height: var(--target-min); }
  .chips, .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); margin: var(--space-2) 0; }
  .note { color: var(--text-secondary); font-size: 12px; }
  button, select, input[type="search"] { min-height: var(--target-min); max-width: 100%; box-sizing: border-box; color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-control); font: inherit; padding: var(--space-1) var(--space-2); }
  input[type="search"] { display: block; width: 100%; }
  input { accent-color: var(--violet); }
</style>

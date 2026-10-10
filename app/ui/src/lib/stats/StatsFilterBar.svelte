<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { StatsCharacter } from '../bindings/StatsCharacter';
  import type { StatsRange } from '../navigation/state';
  let { client, range, characterId, characters, showRange = true, onChange }: {
    client?: Client; range: StatsRange; characterId: string | null; characters: StatsCharacter[]; showRange?: boolean;
    onChange: (patch: { range?: StatsRange; characterId?: string | null }) => void;
  } = $props();
  let classes = $state(new Map<number, string>());
  $effect(() => {
    const owner = client;
    const ids = [...new Set(characters.map(c => c.class_id).filter((id): id is number => id !== null))];
    if (!owner || !ids.length) return;
    let current = true;
    owner.getObjectVisuals(ids).then(page => {
      if (current) classes = new Map(page.objects.flatMap(o => o.name ? [[o.object_type, o.name] as [number, string]] : []));
    }).catch(() => { /* Class names are optional; options still show character names. */ });
    return () => { current = false; };
  });
  const label = (c: StatsCharacter) => `${c.name ?? 'Unnamed character'} · ${(c.class_id !== null && classes.get(c.class_id)) || 'Unknown class'}`;
</script>

<div class="filters" role="group" aria-label="Stats filters">
  {#if showRange}
    <label>Range <select value={range} onchange={e => onChange({ range: e.currentTarget.value as StatsRange })}>
      <option value="all">All time</option><option value="30d">Last 30 days</option><option value="7d">Last 7 days</option>
    </select></label>
  {/if}
  <label>Character <select value={characterId ?? ''} onchange={e => onChange({ characterId: e.currentTarget.value || null })}>
    <option value="">All characters</option>
    {#if characterId && !characters.some(c => c.id === characterId)}<option value={characterId}>Selected character</option>{/if}
    {#each characters as character (character.id)}<option value={character.id}>{label(character)}</option>{/each}
  </select></label>
</div>

<style>
  .filters { display: flex; flex-wrap: wrap; gap: var(--space-3); }
  label { display: grid; gap: var(--space-1); }
  select { font: inherit; color: var(--text-primary); min-height: var(--target-min); border-radius: var(--radius-control); background: var(--surface-panel); border: 1px solid var(--border-strong); padding: var(--space-2) var(--space-3); }
</style>

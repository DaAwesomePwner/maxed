<script lang="ts">
  import { dungeonCatalog, dungeonMetadata } from './dungeon-catalog';
  import { keyPopAllows, filteredByRules, includesKeyPopDungeon } from './preferences';
  import { getContext } from 'svelte';
  import { DUNGEON_MUTE, type DungeonMute } from './dungeon-mute';
  let { dungeon }: { dungeon: string } = $props();
  const mute = getContext<DungeonMute | undefined>(DUNGEON_MUTE);
  const feedback = mute?.state;
  const status = mute?.status;
  const preferences = mute?.preferences;
</script>
{#if mute && feedback && status && preferences}
  {#if $preferences && filteredByRules($preferences, dungeon, dungeonMetadata(dungeon, $dungeonCatalog))}<span>Filtered by rules</span>{:else if $preferences && !includesKeyPopDungeon($preferences, dungeon)}<span>Muted</span>{/if}
  <button type="button" disabled={$feedback?.busy || $status !== 'ready' || !mute.available()
    || (!!$preferences && !keyPopAllows($preferences, dungeon, dungeonMetadata(dungeon, $dungeonCatalog)))} onclick={() => { void mute.mute(dungeon); }}>Mute {dungeon}</button>
{/if}
<style>
  button { min-height: var(--target-min); border: 1px solid var(--border-strong); border-radius: var(--radius-control); padding: var(--space-2); color: var(--text-primary); background: var(--surface-panel); font: inherit; cursor: pointer; }
  button:disabled { color: var(--text-disabled); cursor: default; }
</style>

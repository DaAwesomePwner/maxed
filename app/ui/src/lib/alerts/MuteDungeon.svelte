<script lang="ts">
  import { getContext } from 'svelte';
  import { DUNGEON_MUTE, type DungeonMute } from './dungeon-mute';
  let { dungeon }: { dungeon: string } = $props();
  const mute = getContext<DungeonMute | undefined>(DUNGEON_MUTE);
  const feedback = mute?.state;
  const status = mute?.status;
  const preferences = mute?.preferences;
</script>
{#if mute && feedback && status && preferences}
  <button type="button" disabled={$feedback?.busy || $status !== 'ready' || !mute.available()
    || ($preferences?.key_pop_dungeons != null ? !$preferences.key_pop_dungeons.includes(dungeon) : $preferences?.key_pop_excluded_dungeons.includes(dungeon))} onclick={() => { void mute.mute(dungeon); }}>Mute {dungeon}</button>
{/if}
<style>
  button { min-height: var(--target-min); border: 1px solid var(--border-strong); border-radius: var(--radius-control); padding: var(--space-2); color: var(--text-primary); background: var(--surface-panel); font: inherit; cursor: pointer; }
  button:disabled { color: var(--text-disabled); cursor: default; }
</style>

<script lang="ts">
  import type { DungeonMute } from './dungeon-mute';
  let { mute }: { mute: DungeonMute } = $props();
  let feedback = $derived(mute.state);
</script>
{#if $feedback.message}
  <div class="feedback" role="status">
    <span>{$feedback.message}</span>
    {#if $feedback.undo}<button type="button" disabled={$feedback.busy} onclick={() => { void mute.undo(); }}>Undo</button>{/if}
    {#if $feedback.failed}<button type="button" disabled={$feedback.busy} onclick={() => { void mute.retry(); }}>Retry mute</button>{/if}
  </div>
{/if}
<style>
  .feedback { position: absolute; bottom: var(--space-3); right: var(--space-3); z-index: 21; max-width: calc(100% - 2 * var(--space-3)); display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); padding: var(--space-2); background: var(--surface-panel); border: 1px solid var(--border-strong); border-radius: var(--radius-control); }
  button { min-height: var(--target-min); background: var(--surface-raised); color: var(--text-primary); border: 1px solid var(--border-strong); border-radius: var(--radius-control); font: inherit; cursor: pointer; }
</style>

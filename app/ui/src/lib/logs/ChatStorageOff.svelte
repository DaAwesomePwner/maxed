<script lang="ts">
  let { channel, onEnable }: { channel: string; onEnable: () => void } = $props();
</script>
<div class="empty"><strong>{channel[0].toUpperCase() + channel.slice(1)} chat isn't being saved</strong><p>Maxed only keeps channels you turn on.</p><button class="ctl" onclick={onEnable}>Turn on {channel} chat</button></div>
<style>
  .empty { padding: var(--space-5) var(--space-4); margin: var(--space-2); border: 1px dashed var(--hairline-strong); border-radius: var(--radius-control); font-size: var(--text-caption); }
  strong { font-weight: var(--weight-medium); }
  p { margin: var(--space-1) 0 var(--space-3); color: var(--text-secondary); }
</style>

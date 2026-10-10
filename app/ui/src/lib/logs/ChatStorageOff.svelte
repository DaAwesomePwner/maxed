<script lang="ts">
  let { channel, onEnable }: { channel: string; onEnable: () => void } = $props();
</script>
<div class="empty"><strong>{channel[0].toUpperCase() + channel.slice(1)} chat isn't being saved</strong><p>Maxed only keeps channels you turn on.</p><button class="ctl" onclick={onEnable}>Turn on {channel} chat</button></div>
<style>
  .empty { padding: 20px 14px; margin: 6px; border: 1px dashed var(--border); border-radius: 6px; font-size: 12px; }
  p { margin: 6px 0 12px; color: var(--text-secondary); }
</style>

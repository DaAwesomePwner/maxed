<script lang="ts">
  import '../styles/controls.css';
  let { count, noun, showCount = true, onClick }: { count: number; noun: string; showCount?: boolean; onClick: () => void } = $props();
</script>
{#if count > 0}<button class="ctl newpill" onclick={onClick}>↑ {showCount ? `${count > 99 ? '99+' : count} new` : 'New'} {showCount && count === 1 ? noun.replace(/s$/, '') : noun}</button>{/if}
<style>
  .newpill { position: absolute; left: 50%; top: var(--space-8); transform: translateX(-50%); z-index: 2; border-color: var(--hairline-strong); background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--elevation-popover); }
</style>

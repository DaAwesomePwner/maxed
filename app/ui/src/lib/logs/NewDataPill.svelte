<script lang="ts">
  import '../styles/controls.css';
  let { count, noun, showCount = true, onClick }: { count: number; noun: string; showCount?: boolean; onClick: () => void } = $props();
</script>
{#if count > 0}<button class="ctl newpill" onclick={onClick}>↑ {showCount ? `${count > 99 ? '99+' : count} new` : 'New'} {showCount && count === 1 ? noun.replace(/s$/, '') : noun}</button>{/if}
<style>
  .newpill { position: absolute; left: 50%; top: 34px; transform: translateX(-50%); z-index: 2; padding: 0 12px; border-radius: 99px; background: var(--violet); color: #fff; font-size: 11.5px; font-weight: 600; box-shadow: 0 6px 18px #00000080; }
</style>

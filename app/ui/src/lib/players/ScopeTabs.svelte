<script lang="ts">
  import './players.css';
  let { tabs }: { tabs: { label: string; sub: string; href: string | null; current: boolean }[] } = $props();
</script>
<nav class="player-scope" aria-label="Player scope">
  {#each tabs as tab}
    {#if tab.href}<a href={tab.href} aria-current={tab.current ? 'page' : undefined}>{tab.label} <small>{tab.sub}</small></a>
    {:else}<button type="button" disabled>{tab.label} <small>{tab.sub}</small></button>{/if}
  {/each}
</nav>

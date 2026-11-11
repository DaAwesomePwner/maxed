<script lang="ts">
  let { items }: { items: Array<{ label: string; href?: string | null }> } = $props();
</script>

<nav aria-label="Breadcrumb">
  <ol class="crumbs">
    {#each items as item, index}
      <li aria-current={index === items.length - 1 ? 'page' : undefined}>
        {#if index > 0}<i aria-hidden="true">/</i>{/if}
        {#if index < items.length - 1 && item.href}
          <a href={item.href} title={item.label}>{item.label}</a>
        {:else}
          <span title={item.label}>{item.label}</span>
        {/if}
      </li>
    {/each}
  </ol>
</nav>

<style>
  .crumbs { display: flex; gap: var(--space-2); align-items: center; color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); height: 16px; margin: 0; padding: 0; list-style: none; }
  li { display: flex; gap: var(--space-2); align-items: center; min-width: 0; }
  a, span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .crumbs a { color: var(--text-secondary); text-decoration: none; }
  .crumbs a:hover, .crumbs a:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  .crumbs i { font-style: normal; color: var(--text-disabled); }
</style>

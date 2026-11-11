<script lang="ts">
  import type { RecapTile } from './recap-view';
  let { tiles }: { tiles: RecapTile[] } = $props();
  function comparison(text: string, subject: string, tone: RecapTile['tone']): string {
    const percent = text.match(/\d+%/)?.[0];
    return percent ? `${percent} ${tone === 'up' ? 'above' : tone === 'down' ? 'below' : 'difference from'} ${subject}` : text;
  }
</script>

<div class="recap-tiles">
  {#each tiles as tile}
    <dl class="v4-card recap-tile" data-key={tile.key} data-tone={tile.tone}>
      <dt>{tile.label}{#if tile.partial}<span class="recap-partial"> · Partial</span>{/if}</dt>
      <dd>
        {#if tile.comparison === 'party' && tile.tone !== null}
          <span class="recap-tile-value recap-comparison">
            <span class="sr-only">{comparison(tile.value, 'party average', tile.tone)}</span>
            <span aria-hidden="true">{tile.value}</span>
          </span>
        {:else}<span class="recap-tile-value">{tile.value}</span>{/if}
        {#if tile.detail}
          {#if tile.comparison === 'usual' && tile.tone !== null}
            <span class="recap-tile-detail recap-comparison">
              <span class="sr-only">{comparison(tile.detail, 'your usual', tile.tone)}</span>
              <span aria-hidden="true">{tile.detail}</span>
            </span>
          {:else}<span class="recap-tile-detail">{tile.detail}</span>{/if}
        {/if}
      </dd>
    </dl>
  {/each}
</div>

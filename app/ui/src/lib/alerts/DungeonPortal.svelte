<script lang="ts">
  import type { DungeonPortalSprite } from '../bridge/dungeon-portal';
  import SpriteImage from '../components/SpriteImage.svelte';
  let { portal = null }: { portal?: DungeonPortalSprite | null } = $props();
</script>
{#if portal}<SpriteImage sprite={portal.sprite} sheetUrl={portal.sheetUrl} alt="" fit={24} />{/if}

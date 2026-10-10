<script lang="ts">
  import type { Client } from '../bridge/client';
  import { createDungeonPortalSprite, type DungeonPortalSprite } from '../bridge/dungeon-portal';
  import SpriteImage from '../components/SpriteImage.svelte';
  let { client, dungeon }: { client?: Client; dungeon: string | null } = $props();
  let portalClient = $derived(client);
  let portal = $state.raw<DungeonPortalSprite | null>(null);
  let lookup = $state.raw<ReturnType<typeof createDungeonPortalSprite>>();
  $effect(() => {
    const owner = portalClient;
    portal = null;
    if (!owner) { lookup = undefined; return; }
    const instance = createDungeonPortalSprite(owner, value => { portal = value; });
    lookup = instance;
    return () => instance.dispose();
  });
  $effect(() => { lookup?.update(dungeon); });
</script>
{#if portal}<SpriteImage sprite={portal.sprite} sheetUrl={portal.sheetUrl} alt={portal.name} fit={16} />{:else}<i class="g portal" aria-hidden="true"></i>{/if}

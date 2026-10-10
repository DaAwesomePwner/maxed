import type { Client } from './client';
import type { VisualPage } from '../bindings/VisualPage';
import type { SpriteRef } from '../bindings/SpriteRef';
import { spriteUrl } from './visuals';

export interface DungeonPortalSprite { sprite: SpriteRef; sheetUrl: string; name: string }
type PortalClient = Pick<Client, 'getDungeonPortalType' | 'onVisualsChanged'>;

/** One lifecycle owns the portal type and optional sprite read. Retired dungeon
 * or catalog requests cannot publish either stage; missing assets mean no icon. */
function createPortalLookup<T>(client: PortalClient, receive: (value: T | null) => void,
  resolve: (type: number, dungeon: string) => Promise<T | null>) {
  let dungeon: string | null = null, revision = 0, disposed = false;
  async function load() {
    const request = ++revision, name = dungeon;
    receive(null);
    if (name === null) return;
    try {
      const type = await client.getDungeonPortalType(name);
      if (disposed || request !== revision || type === null || !Number.isInteger(type) || type < 0 || type > 65535) return;
      const value = await resolve(type, name);
      if (!disposed && request === revision) receive(value);
    } catch {
      if (!disposed && request === revision) receive(null);
    }
  }
  const release = client.onVisualsChanged(() => { if (!disposed) void load(); });
  return {
    update(next: string | null) {
      if (disposed || next === dungeon) return;
      dungeon = next; void load();
    },
    dispose() { if (disposed) return; disposed = true; revision++; release(); },
  };
}

/** Home includes the portal type in its existing batched visuals lookup. */
export function createDungeonPortal(client: PortalClient, receive: (type: number | null) => void) {
  return createPortalLookup(client, receive, async type => type);
}

/** Alerts and pickers can resolve just the portal sprite through the same cache. */
export function createDungeonPortalSprite(client: PortalClient & Pick<Client, 'getObjectVisuals'>,
  receive: (portal: DungeonPortalSprite | null) => void) {
  return createPortalLookup(client, receive, async (type, dungeon) => {
    const page = await client.getObjectVisuals([type]);
    return dungeonPortalSpriteFromPage(page, type, dungeon);
  });
}

/** Shared by single-alert lookups and catalog-wide picker batches. */
export function dungeonPortalSpriteFromPage(page: VisualPage, type: number, dungeon: string): DungeonPortalSprite | null {
  const visual = page.objects.find(row => row.object_type === type);
  const sheetUrl = visual?.sprite ? spriteUrl(page.client_fingerprint, visual.sprite) : null;
  return visual?.sprite && sheetUrl ? { sprite: visual.sprite, sheetUrl, name: visual.name ?? dungeon } : null;
}

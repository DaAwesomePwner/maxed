import type { DungeonDef } from '../../src/lib/bindings/DungeonDef';

// Invented catalog entries default to no dungeon metadata.
export const dungeonDef = (patch: Pick<DungeonDef, 'name'> & Partial<DungeonDef>): DungeonDef => ({
  aliases: [], kind: 'dungeon', completion_index: null, portal_type: null,
  difficulty: null, exalt: false, exalt_gateway: false, category: null, ...patch,
});

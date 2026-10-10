import type { ObjectVisual } from '../bindings/ObjectVisual';
import type { SpriteRef } from '../bindings/SpriteRef';
import type { VisualPage } from '../bindings/VisualPage';

export const SPRITE_ORIGIN = 'http://maxed-sprite.localhost';
/** Stable keys prevent snapshot numeric updates from repeating catalog reads. */
export function objectVisualTypes(types: (number | null | undefined)[]): number[] {
  return [...new Set(types.filter((type): type is number => type !== null && type !== undefined))].sort((a, b) => a - b);
}
/** Largest whole-number zoom (1-16, SpriteImage's range) that fits a square box. */
export function fittedSpriteScale(sprite: SpriteRef | null, box: number): number {
  const side = sprite ? Math.max(sprite.width, sprite.height) : 8;
  return Math.min(16, Math.max(1, Math.floor(box / side)));
}
export function spriteUrl(fingerprint: string, sprite: SpriteRef): string | null {
  if (!/^[a-f0-9]{64}$/.test(fingerprint) || !['characters.png', 'characters_masks.png', 'groundTiles.png', 'mapObjects.png', 'enchantPips.png'].includes(sprite.sheet)) return null;
  if (![sprite.x, sprite.y, sprite.width, sprite.height].every(Number.isSafeInteger)
    || sprite.x < 0 || sprite.y < 0 || sprite.width < 1 || sprite.height < 1
    || sprite.width > 256 || sprite.height > 256 || sprite.x + sprite.width > 16384 || sprite.y + sprite.height > 16384) return null;
  return `${SPRITE_ORIGIN}/${fingerprint}/${sprite.sheet}/${sprite.x}-${sprite.y}-${sprite.width}-${sprite.height}.png`;
}

/** The asset bridge supplies a local URL. Remote/data URLs are not image sources. */
export function localSheetUrl(source: string | null): string | null {
  if (!source || typeof window === 'undefined') return null;
  try {
    const url = new URL(source, window.location.href);
    if (url.username || url.password) return null;
    const asset = (url.protocol === 'asset:' && url.hostname === 'localhost')
      || (['http:', 'https:'].includes(url.protocol) && url.hostname === 'asset.localhost' && !url.port);
    const sameOrigin = ['http:', 'https:'].includes(url.protocol) && url.origin === window.location.origin;
    return asset || sameOrigin || url.origin === SPRITE_ORIGIN ? url.href : null;
  } catch { return null; }
}

/** Each lookup checks the native fingerprint, including entirely cached pages.
 * Requests serialize so an older response cannot restore a retired generation. */
export function createVisualCache(fetch: (types: number[]) => Promise<VisualPage>) {
  const rows = new Map<number, ObjectVisual>();
  let fingerprint: string | undefined;
  let epoch = 0;
  let tail: Promise<unknown> = Promise.resolve();
  function accept(page: VisualPage) {
    if (page.client_fingerprint !== fingerprint) { rows.clear(); fingerprint = page.client_fingerprint; }
    for (const row of page.objects) {
      rows.delete(row.object_type);
      rows.set(row.object_type, structuredClone(row));
      if (rows.size > 256) rows.delete(rows.keys().next().value!);
    }
  }
  return {
    clear() { epoch++; rows.clear(); fingerprint = undefined; },
    get(types: number[]): Promise<VisualPage> {
      const requested = [...new Set(types)];
      if (requested.length > 128 || requested.some(type => !Number.isInteger(type) || type < 0 || type > 65535)) return Promise.reject(new Error('Request at most 128 unique object types in the u16 range'));
      const token = epoch;
      const work = tail.then(async () => {
        if (token !== epoch) throw new Error('Asset selection changed. Retry the lookup.');
        const first = await fetch(fingerprint === undefined ? requested : []);
        if (token !== epoch) throw new Error('Asset selection changed. Retry the lookup.');
        accept(first);
        // Fetch the whole requested page on a miss: a concurrently published
        // generation must not combine old cached rows with new missing rows.
        if (requested.some(type => !rows.has(type))) {
          const page = await fetch(requested);
          if (token !== epoch) throw new Error('Asset selection changed. Retry the lookup.');
          accept(page);
        }
        const objects = requested.map(type => {
          const row = rows.get(type);
          if (!row) throw new Error('Object visuals are unavailable. Retry the lookup.');
          rows.delete(type); rows.set(type, row);
          return structuredClone(row);
        });
        return { client_fingerprint: fingerprint!, objects };
      });
      tail = work.catch(() => {});
      return work;
    },
  };
}

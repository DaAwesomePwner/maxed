import type { RecapImageSettings } from '../../bindings/RecapImageSettings';
import { SPRITE_ORIGIN } from '../../bridge/visuals';
import { formatDps, formatShare } from '../../styles/format';
import { rankedDrops } from '../v3/loot-rank';
import { signedPercent } from './hud';
import type { RunRecapView, VisitRecapView } from './recap-view';

export interface RecapImageModel { width: number; height: number; scale: 2; blocks: RecapImageBlock[] }
export type RecapImageBlock =
  | { kind: 'banner'; title: string; outcome: string; tone: string; clearTime: string | null; pb: boolean; y: number }
  | { kind: 'stats'; items: Array<{ label: string; value: string; partial: boolean }>; y: number }
  | { kind: 'fights'; rows: Array<{ name: string; time: string; dps: string; vsParty: string; spriteUrl: string | null }>; partial: boolean; more: number; y: number }
  | { kind: 'party'; rows: Array<{ rank: number; label: string; share: string; shareValue: number | null }>; y: number }
  | { kind: 'loot'; items: Array<{ name: string; spriteUrl: string | null }>; more: number; partial: boolean; y: number }
  | { kind: 'footer'; text: string; y: number };
export interface RecapImageContext {
  character: string | null; className: string | null; date: string;
  spriteUrl: (type: number) => string | null;
  playerClassName: (classId: number | null) => string | null;
  /** Catalog labels for loot and otherwise unnamed targets; never guessed from IDs. */
  objectName?: (type: number) => string | null;
}
export const defaultRecapImageSettings: RecapImageSettings = { names: true, loot: true, fights: true };

/** Capture timestamps are decimal microseconds. Use the local calendar date. */
export function recapDate(at: string | null): string {
  if (at === null || !/^\d+$/.test(at)) return '—';
  const date = new Date(Number(BigInt(at) / 1000n));
  return Number.isFinite(date.getTime()) ? new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(date) : '—';
}

export function recapImageModel(view: RunRecapView | VisitRecapView, options: RecapImageSettings,
  context: RecapImageContext): RecapImageModel {
  const blocks: RecapImageBlock[] = [];
  let y = 0;
  const add = (block: RecapImageBlock, height: number) => { blocks.push(block); y += height; };
  const targetName = (type: number | null) => type === null ? null : context.objectName?.(type);
  const targetSprite = (type: number | null) => type === null ? null : context.spriteUrl(type);
  add({ kind: 'banner', title: view.title, outcome: view.kind === 'run' ? view.outcomeLabel : 'Realm visit',
    tone: view.kind === 'run' ? view.outcome ?? 'unknown' : 'visit',
    clearTime: view.kind === 'run' ? view.died ? `Died at ${view.died.elapsed}` : view.clearTime : view.timeHere,
    pb: view.kind === 'run' && view.pb, y }, 148);
  const tile = (key: string) => view.tiles.find(tile => tile.key === key);
  const stat = (key: string, label: string, field: 'value' | 'detail' = 'value') => {
    const source = tile(key);
    return { label, value: source?.[field] ?? '—', partial: source?.partial ?? false };
  };
  const items = view.kind === 'run' ? [
    stat('rank', 'Rank'), stat('dps', 'Your DPS'), stat('rank', 'Share', 'detail'), stat('fame', 'Fame'),
  ] : [
    stat('dps', 'Your DPS'), stat('best-rank', 'Best rank'), stat('events', 'Events killed'), stat('fame', 'Fame'),
  ];
  add({ kind: 'stats', items, y }, 94);
  if (options.fights) {
    const source = view.kind === 'run' ? view.fights : view.events.filter(row => row.state === 'killed');
    const rows = source.slice(0, 8).map(row => ({ name: targetName(row.targetType) ?? row.name ?? '—', time: row.kill ?? '—',
      dps: formatDps(row.dps), vsParty: signedPercent(row.vsParty), spriteUrl: targetSprite(row.targetType) }));
    const partial = view.kind === 'run' ? view.fightsPartial : tile('events')?.partial ?? false;
    const more = source.length - rows.length;
    add({ kind: 'fights', rows, partial, more, y }, 48 + Math.max(1, rows.length) * 64 + (more ? 32 : 0));
  }
  if (view.kind === 'run') {
    const rows = view.party.rows.filter(row => row.rank !== null).slice(0, 5).map(row => ({ rank: row.rank!,
      label: row.isLocal || options.names ? row.name ?? '—' : `#${row.rank} · ${context.playerClassName(row.classId) ?? '—'}`,
      share: formatShare(row.share), shareValue: row.share }));
    add({ kind: 'party', rows, y }, 48 + Math.max(1, rows.length) * 40);
  }
  if (options.loot) {
    const drops = rankedDrops(view.drops), items: Array<{ name: string; spriteUrl: string | null }> = [];
    let represented = 0;
    for (const drop of drops) {
      if (items.length === 8) break;
      const contents = drop.contents_known
        ? (drop.contents.length ? drop.contents.map(item => item.item_type) : drop.items) : [];
      const types = contents.length ? contents : [drop.bag_type];
      const shown = types.slice(0, 8 - items.length);
      for (const type of shown) {
        items.push({ name: targetName(type) ?? '—', spriteUrl: targetSprite(type) });
      }
      if (shown.length === types.length) represented++;
    }
    const more = drops.length - represented;
    add({ kind: 'loot', items, more, partial: view.kind === 'visit' && view.dropsPartial, y },
      48 + Math.max(1, Math.ceil(items.length / 2)) * 48 + (more ? 32 : 0));
  }
  add({ kind: 'footer', text: `${context.character ?? '—'} · ${context.className ?? '—'} · ${context.date} · Maxed`, y }, 64);
  return { width: 640, height: y, scale: 2, blocks };
}

function isSpriteUrl(source: string): boolean {
  try { const url = new URL(source); return url.origin === SPRITE_ORIGIN && !url.username && !url.password; }
  catch { return false; }
}
export async function loadSprite(url: string): Promise<HTMLImageElement> {
  if (!isSpriteUrl(url)) throw new Error('Unavailable sprite');
  const image = new Image();
  image.crossOrigin = 'anonymous';
  image.src = url;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      image.decode(),
      new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error('Sprite timed out')), 2000); }),
    ]);
  } finally { clearTimeout(timer); }
  return image;
}

// The shareable PNG uses the app's contract palette (tokens.css): graphite surfaces, gold for you, neutral party.
const displayFont = '"Segoe UI", system-ui, sans-serif';
const textFont = displayFont;
const colors = { background: '#0f0f10', panel: '#19191b', text: '#ededed', secondary: '#b4b4ba', muted: '#96969c',
  gold: '#f2c14e', bar: '#36363a', green: '#4cc38a', red: '#f2777a', warning: '#e8a25a', border: '#2a2a2d' };

/** Call only from the copy action. The detached canvas never captures the DOM. */
export async function renderRecapImage(model: RecapImageModel,
  loadImage: (url: string) => Promise<HTMLImageElement>): Promise<Blob> {
  const canvas = document.createElement('canvas');
  canvas.width = model.width * model.scale; canvas.height = model.height * model.scale;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('Canvas unavailable');
  ctx.scale(model.scale, model.scale);
  ctx.imageSmoothingEnabled = false;
  ctx.textBaseline = 'middle';
  const urls = new Set(model.blocks.flatMap(block => block.kind === 'fights' ? block.rows.map(row => row.spriteUrl)
    : block.kind === 'loot' ? block.items.map(item => item.spriteUrl) : []).filter((url): url is string => url !== null && isSpriteUrl(url)));
  const sprites = new Map(await Promise.all([...urls].map(async url => {
    try { return [url, await loadImage(url)] as const; } catch { return [url, null] as const; }
  })));
  const rect = (x: number, y: number, w: number, h: number, color: string) => { ctx.fillStyle = color; ctx.fillRect(x, y, w, h); };
  const text = (value: string, x: number, y: number, width: number, size = 16, color: string = colors.text, title = false) => {
    ctx.font = `${title ? 500 : 400} ${size}px ${title ? displayFont : textFont}`; ctx.fillStyle = color;
    let fitted = value;
    if (ctx.measureText(fitted).width > width) {
      const points = Array.from(value);
      while (points.length && ctx.measureText(`${points.join('')}…`).width > width) points.pop();
      fitted = points.join('');
      fitted += '…';
    }
    ctx.fillText(fitted, x, y);
    return ctx.measureText(fitted).width;
  };
  const spriteText = (item: { name: string; spriteUrl: string | null }, x: number, y: number, width: number) => {
    const image = item.spriteUrl ? sprites.get(item.spriteUrl) : null;
    let inset = 0;
    if (image) {
      const w = image.naturalWidth || image.width, h = image.naturalHeight || image.height;
      if (w > 0 && h > 0) {
        const scale = Math.min(32 / w, 32 / h);
        ctx.drawImage(image, x, y - h * scale / 2, w * scale, h * scale);
        inset = 42;
      }
    }
    text(item.name, x + inset, y, width - inset);
  };
  rect(0, 0, model.width, model.height, colors.background);
  for (const block of model.blocks) {
    const y = block.y;
    switch (block.kind) {
      case 'banner': {
        const accent = block.tone === 'died' ? colors.red : block.tone === 'cleared' ? colors.green : colors.secondary;
        rect(0, y, model.width, 148, colors.panel); rect(0, y, model.width, 4, accent);
        text(block.outcome, 24, y + 30, 592, 13, accent, true);
        text(block.title, 24, y + 65, 592, 28, colors.text, true);
        if (block.clearTime !== null) text(block.clearTime, 24, y + 111, 440, 28, block.tone === 'died' ? colors.red : colors.text, true);
        if (block.pb) {
          rect(568, y + 98, 48, 26, colors.green); rect(569, y + 99, 46, 24, colors.panel);
          text('PB', 582, y + 111, 32, 15, colors.green, true);
        }
        break;
      }
      case 'stats':
        block.items.forEach((item, i) => {
          const x = 24 + i * 148;
          const labelWidth = text(item.label, x, y + 27, item.partial ? 72 : 136, 13, colors.muted);
          if (item.partial) text('Partial', x + labelWidth + 8, y + 27, 56, 12, colors.warning);
          text(item.value, 24 + i * 148, y + 57, 136, item.value.length > 12 ? 14 : 20, item.label === 'Your DPS' ? colors.gold : colors.text, true);
        });
        break;
      case 'fights':
        rect(24, y, 592, 1, colors.border);
        text('Fight by fight', 24, y + 25, 348, 16, colors.text, true);
        if (block.partial) text('Partial', 184, y + 25, 80, 12, colors.warning);
        text('Your DPS', 424, y + 25, 86, 12, colors.muted); text('vs party', 536, y + 25, 80, 12, colors.muted);
        if (!block.rows.length) text('—', 24, y + 70, 592, 16, colors.muted);
        block.rows.forEach((row, i) => {
          const top = y + 48 + i * 64;
          spriteText(row, 24, top + 16, 380);
          text(row.dps, 424, top + 16, 90, 16, colors.gold); text(row.vsParty, 536, top + 16, 80, 16, colors.secondary);
          text(row.time, 24, top + 42, 592, 13, colors.muted);
        });
        if (block.more) text(`+${block.more} more fights`, 24, y + 48 + block.rows.length * 64 + 16, 592, 13, colors.muted);
        break;
      case 'party':
        rect(24, y, 592, 1, colors.border); text('Party · top 5', 24, y + 25, 592, 16, colors.text, true);
        if (!block.rows.length) text('—', 24, y + 68, 592, 16, colors.muted);
        block.rows.forEach((row, i) => {
          const top = y + 48 + i * 40, share = row.shareValue;
          rect(24, top, 592, 30, colors.panel);
          if (share !== null && Number.isFinite(share)) rect(24, top, 592 * Math.min(1, Math.max(0, share)), 30, colors.bar);
          text(row.label, 36, top + 15, 462, 14); text(row.share, 536, top + 15, 72, 14, colors.secondary);
        });
        break;
      case 'loot':
        rect(24, y, 592, 1, colors.border); text('Loot', 24, y + 25, 592, 16, colors.text, true);
        if (block.partial) text('Partial', 84, y + 25, 80, 12, colors.warning);
        if (!block.items.length) text('—', 24, y + 72, 592, 16, colors.muted);
        block.items.forEach((item, i) => spriteText(item, 24 + (i % 2) * 300, y + 72 + Math.floor(i / 2) * 48, 284));
        if (block.more) text(`+${block.more} more ${block.more === 1 ? 'bag' : 'bags'}`, 24, y + 48 + Math.max(1, Math.ceil(block.items.length / 2)) * 48 + 16, 592, 13, colors.muted);
        break;
      case 'footer':
        rect(0, y, model.width, 64, colors.panel); text(block.text, 24, y + 32, 592, 13, colors.muted);
        break;
    }
  }
  return new Promise<Blob>((resolve, reject) => canvas.toBlob(blob => {
    if (blob) resolve(blob); else reject(new Error('PNG unavailable'));
  }, 'image/png'));
}

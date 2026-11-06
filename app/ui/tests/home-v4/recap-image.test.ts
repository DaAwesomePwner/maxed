import { afterEach, expect, it, vi } from 'vitest';
import { loadSprite, recapImageModel, renderRecapImage, recapDate } from '../../src/lib/home/v4/recap-image';
import { context, options, runView, sprite, visitView } from './recap-image-fixtures';
import { homeSnapshot } from '../home/fixtures';

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
it('blocks_follow_spec_order', () => {
  const model = recapImageModel(runView(), options, context);
  expect(model).toMatchObject({ width: 640, scale: 2 });
  expect(model.blocks.map(block => block.kind)).toEqual(['banner', 'stats', 'fights', 'party', 'loot', 'footer']);
  expect(model.blocks.map(block => block.y)).toEqual([...model.blocks.map(block => block.y)].sort((a, b) => a - b));
  expect(model.blocks.at(-1)!.y).toBeLessThan(model.height);
  expect(model.blocks[1]).toMatchObject({ items: [
    { value: '#2 of 8' }, { value: '1,234' }, { value: '25% of damage' }, { value: '+12' },
  ] });
  expect(model.blocks.at(-1)).toMatchObject({ text: 'Synthetic Hero · Synthetic Knight · Oct 13, 2026 · MAXED' });
});
it('fights_and_loot_toggles_remove_blocks', () => {
  const full = recapImageModel(runView(), options, context);
  const small = recapImageModel(runView(), { ...options, fights: false, loot: false }, context);
  expect(small.blocks.map(block => block.kind)).toEqual(['banner', 'stats', 'party', 'footer']);
  expect(small.height).toBeLessThan(full.height);
});
it('names_off_uses_rank_and_class', () => {
  const view = runView(); view.party.rows.unshift({ rank: null, name: 'Synthetic unranked', classId: 1, share: null, isLocal: false });
  const party = recapImageModel(view, { ...options, names: false }, context).blocks.find(block => block.kind === 'party')!;
  expect(party.rows.map(row => row.label)).toEqual(['#1 · Synthetic Knight', 'Synthetic player 2', '#3 · —', '#4 · Synthetic Knight', '#5 · Synthetic Knight']);
  view.party.rows[1].name = null;
  expect(recapImageModel(view, options, context).blocks.find(block => block.kind === 'party')!.rows[0].label).toBe('—');
});
it('unknowns_are_dashes', () => {
  const view = runView(); view.tiles = []; view.fights[0] = { ...view.fights[0], name: null, targetType: null, kill: '—', dps: null, vsParty: null };
  const model = recapImageModel(view, options, { ...context, character: null, className: null, date: '—' });
  expect(model.blocks.find(block => block.kind === 'stats')!.items.map(item => item.value)).toEqual(['—', '—', '—', '—']);
  expect(model.blocks.find(block => block.kind === 'fights')!.rows[0]).toMatchObject({ name: '—', time: '—', dps: '—', vsParty: '—', spriteUrl: null });
  expect(model.blocks.at(-1)).toMatchObject({ text: '— · — · — · MAXED' });
});
it('visit_model_uses_event_rows', () => {
  const view = visitView();
  view.tiles = ['fame', 'events', 'best-rank', 'dps'].map((key, i) => ({ key: key as 'fame' | 'events' | 'best-rank' | 'dps',
    label: 'Unrelated', value: String(i), detail: null, tone: null, partial: false, comparison: null }));
  const model = recapImageModel(view, options, context);
  expect(model.blocks.map(block => block.kind)).toEqual(['banner', 'stats', 'fights', 'loot', 'footer']);
  expect(model.blocks[0]).toMatchObject({ outcome: 'REALM VISIT', clearTime: '2:00', pb: false });
  expect(model.blocks.find(block => block.kind === 'stats')!.items.map(item => item.value)).toEqual(['3', '2', '1', '0']);
  expect(model.blocks.find(block => block.kind === 'fights')!.rows).toEqual([
    { name: 'Synthetic object 30', time: '0:45', dps: '100', vsParty: '−25%', spriteUrl: sprite },
  ]);
});
function canvas(blob: Blob | null = new Blob(['synthetic'], { type: 'image/png' })) {
  const ctx = { fillText: vi.fn(), drawImage: vi.fn(), fillRect: vi.fn(), scale: vi.fn(),
    measureText: (s: string) => ({ width: s.length * 8 }), imageSmoothingEnabled: true };
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
  const toBlob = vi.spyOn(HTMLCanvasElement.prototype, 'toBlob').mockImplementation(function (callback) { callback(blob); });
  return { ctx, toBlob };
}
it('sprite_failure_draws_name', async () => {
  const { ctx, toBlob } = canvas(), loader = vi.fn().mockRejectedValue(new Error('missing synthetic tile'));
  const blob = await renderRecapImage(recapImageModel(runView(), options, context), loader);
  expect(blob.type).toBe('image/png'); expect(toBlob).toHaveBeenCalledWith(expect.any(Function), 'image/png');
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'Synthetic object 30')).toBe(true);
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'Synthetic object 21')).toBe(true);
  expect(ctx.drawImage).not.toHaveBeenCalled(); expect(ctx.imageSmoothingEnabled).toBe(false);
  expect(ctx.scale).toHaveBeenCalledWith(2, 2);
  const element = toBlob.mock.contexts[0] as HTMLCanvasElement;
  expect(element.width).toBe(1280);
});
it('rejects_null_blob_and_never_loads_non_sprite_origins', async () => {
  const { ctx } = canvas(null), loader = vi.fn();
  await expect(renderRecapImage(recapImageModel(runView(), options, { ...context, spriteUrl: () => 'https://example.com/tile.png' }), loader)).rejects.toThrow();
  expect(loader).not.toHaveBeenCalled(); expect(ctx.drawImage).not.toHaveBeenCalled();
});
it('load_sprite_sets_cors_before_src_and_decodes', async () => {
  const writes: string[] = [], decode = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('Image', class { set crossOrigin(value: string) { writes.push(`cors:${value}`); }
    set src(value: string) { writes.push(`src:${value}`); } decode = decode; });
  await loadSprite(sprite);
  expect(writes).toEqual(['cors:anonymous', `src:${sprite}`]); expect(decode).toHaveBeenCalledOnce();
  await expect(loadSprite('https://example.com/tile.png')).rejects.toThrow();
});
it('date_uses_local_medium_format_and_preserves_unknowns', () => {
  const at = String(Date.parse('2026-10-14T01:00:00Z') * 1000);
  expect(recapDate(at)).toBe(new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(new Date(Number(at) / 1000)));
  for (const invalid of [null, '', 'broken', '9'.repeat(40)]) expect(recapDate(invalid)).toBe('—');
});

it.each(['run', 'visit'] as const)('partial_tiles_are_marked (%s)', async kind => {
  const view = kind === 'run' ? runView() : visitView();
  if (view.kind === 'visit') view.tiles = ['dps', 'best-rank', 'events', 'fame'].map(key => ({
    key: key as 'dps' | 'best-rank' | 'events' | 'fame', label: 'Synthetic label', value: '—',
    detail: null, tone: null, partial: false, comparison: null,
  }));
  view.tiles = view.tiles.map(tile => ({ ...tile, partial: tile.key === 'rank' || tile.key === 'dps' }));
  const model = recapImageModel(view, options, context), stats = model.blocks.find(block => block.kind === 'stats')!;
  expect(stats.items.map(item => item.partial)).toEqual(kind === 'run' ? [true, true, true, false] : [true, false, false, false]);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn().mockRejectedValue(new Error('Synthetic missing sprite')));
  const markers = ctx.fillText.mock.calls.filter(call => call[0] === 'Partial');
  expect(markers).toHaveLength(kind === 'run' ? 3 : 1);
  for (const [i, item] of stats.items.entries()) {
    const label = ctx.fillText.mock.calls.find(call => call[2] === stats.y + 27 && call[1] === 24 + i * 148)!;
    expect(label).toBeDefined();
    expect(markers.some(call => call[2] === label[2] && call[1] > label[1] && call[1] < 24 + (i + 1) * 148)).toBe(item.partial);
  }
});

it.each(['run', 'visit'] as const)('partial_fights_block_is_marked (%s)', async kind => {
  const view = kind === 'run' ? runView() : visitView();
  if (view.kind === 'run') view.fightsPartial = true;
  else view.tiles = [{ key: 'events', label: 'Events', value: '1', detail: null, tone: null, partial: true, comparison: null }];
  const model = recapImageModel(view, options, context), fights = model.blocks.find(block => block.kind === 'fights')!;
  expect(fights.partial).toBe(true);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn().mockRejectedValue(new Error('Synthetic missing sprite')));
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'Partial' && call[2] === fights.y + 25)).toBe(true);
  const complete = recapImageModel(kind === 'run' ? runView() : visitView(), options, context);
  expect(complete.blocks.find(block => block.kind === 'fights')!.partial).toBe(false);
  ctx.fillText.mockClear(); await renderRecapImage(complete, vi.fn().mockRejectedValue(new Error('Synthetic missing sprite')));
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'Partial')).toBe(false);
});

it('loot_block_is_bounded_and_ranked', async () => {
  const view = runView(), base = homeSnapshot().drops[0];
  view.drops = Array.from({ length: 25 }, (_, i) => ({ ...base, id: `synthetic-drop-${i}`, bag_type: 1000 + i,
    bag_rank: Math.floor(i / 5), at: String(1000000 + i), contents_known: i === 24,
    // Unknown contents must ignore stale item projections.
    contents: [0, 1, 2].map(n => ({ ...base.contents[0], item_type: 1000 + i * 10 + n })), items: [9999] }));
  view.best = { drop: view.drops[0] };
  const model = recapImageModel(view, options, context), loot = model.blocks.find(block => block.kind === 'loot')!;
  expect(loot.items.map(item => item.name)).toEqual([1240, 1241, 1242, 1023, 1022, 1021, 1020, 1019].map(type => `Synthetic object ${type}`));
  expect(loot.more).toBe(19);
  const footer = model.blocks.find(block => block.kind === 'footer')!;
  expect(footer.y - loot.y).toBe(48 + 4 * 48 + 32);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn().mockRejectedValue(new Error('Synthetic missing sprite')));
  const more = ctx.fillText.mock.calls.filter(call => call[0] === '+19 more bags');
  expect(more).toHaveLength(1);
  expect(more[0][2]).toBeGreaterThan(loot.y + 72 + 3 * 48);
  expect(more[0][2]).toBeLessThan(footer.y);
});

it('loot_block_omits_more_when_all_fit', async () => {
  const model = recapImageModel(runView(), options, context), loot = model.blocks.find(block => block.kind === 'loot')!;
  expect(loot.items).toEqual([{ name: 'Synthetic object 21', spriteUrl: sprite }]);
  expect(loot.more).toBe(0);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn().mockRejectedValue(new Error('Synthetic missing sprite')));
  expect(ctx.fillText.mock.calls.some(call => /more bags/.test(call[0]))).toBe(false);
});

it('partial_visit_loot_is_marked_in_the_model_and_header', async () => {
  const view = visitView(); view.dropsPartial = true;
  const model = recapImageModel(view, options, context), loot = model.blocks.find(block => block.kind === 'loot')!;
  expect(loot.partial).toBe(true);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn());
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'Partial' && call[2] === loot.y + 25)).toBe(true);
  expect(recapImageModel(runView(), options, context).blocks.find(block => block.kind === 'loot')!.partial).toBe(false);
});

it.each(['run', 'visit'] as const)('image_fights_are_capped_with_a_remaining_count: %s', async kind => {
  const view = kind === 'run' ? runView() : visitView();
  if (view.kind === 'run') view.fights = Array.from({ length: 11 }, (_, i) => ({ ...view.fights[0], fightId: `synthetic-${i}` }));
  else view.events = Array.from({ length: 11 }, (_, i) => ({ ...view.events[0], key: `synthetic-${i}`, state: 'killed' }));
  const model = recapImageModel(view, options, context), fights = model.blocks.find(block => block.kind === 'fights')!;
  expect(fights.rows).toHaveLength(8); expect(fights.more).toBe(3);
  const next = model.blocks[model.blocks.indexOf(fights) + 1];
  expect(next.y - fights.y).toBe(48 + 8 * 64 + 32);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn());
  const more = ctx.fillText.mock.calls.filter(call => call[0] === '+3 more fights');
  expect(more).toHaveLength(1); expect(more[0][2]).toBeLessThan(next.y);
});

it('partially_displayed_bag_counts_as_more_and_uses_singular', async () => {
  const view = runView(); view.drops[0].items = Array.from({ length: 9 }, (_, i) => 100 + i);
  const model = recapImageModel(view, options, context), loot = model.blocks.find(block => block.kind === 'loot')!;
  expect(loot.items).toHaveLength(8); expect(loot.more).toBe(1);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn());
  expect(ctx.fillText.mock.calls.filter(call => call[0] === '+1 more bag')).toHaveLength(1);
  expect(ctx.fillText.mock.calls.some(call => call[0] === '+1 more bags')).toBe(false);
});

it('death_image_uses_elapsed_death_time', async () => {
  const view = runView(); view.outcome = 'died'; view.outcomeLabel = 'DIED'; view.clearTime = null; view.pb = false;
  view.died = { at: '75000000', elapsed: '1:14' };
  const model = recapImageModel(view, options, context);
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn());
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'DIED AT 1:14')).toBe(true);
});

it('party_bars_use_numeric_share_and_omit_unknowns', async () => {
  const view = runView(); view.party.rows = view.party.rows.slice(0, 2);
  view.party.rows[0].share = .123456; view.party.rows[1].share = null;
  const model = recapImageModel(view, options, context), party = model.blocks.find(block => block.kind === 'party')!;
  expect(party.rows.map(row => row.shareValue)).toEqual([.123456, null]);
  party.rows[0].share = '75,0%'; party.rows[1].share = '25%';
  const { ctx } = canvas(); await renderRecapImage(model, vi.fn().mockRejectedValue(new Error('Synthetic missing sprite')));
  const first = ctx.fillRect.mock.calls.filter(call => call[1] === party.y + 48);
  expect(first).toHaveLength(2); expect(first[1][2]).toBeCloseTo(592 * .123456);
  expect(ctx.fillRect.mock.calls.filter(call => call[1] === party.y + 88)).toEqual([[24, party.y + 88, 592, 30]]);
});

it('sprite_timeout_draws_name', async () => {
  vi.useFakeTimers(); const { ctx } = canvas();
  vi.stubGlobal('Image', class { decode = () => new Promise<void>(() => {}); });
  let rendered = false;
  const rendering = renderRecapImage(recapImageModel(runView(), options, context), loadSprite).then(() => { rendered = true; });
  await vi.advanceTimersByTimeAsync(1999); expect(rendered).toBe(false);
  await vi.advanceTimersByTimeAsync(1); expect(rendered).toBe(true);
  await rendering;
  expect(ctx.drawImage).not.toHaveBeenCalled();
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'Synthetic object 30')).toBe(true);
  expect(ctx.fillText.mock.calls.some(call => call[0] === 'Synthetic object 21')).toBe(true);
  expect(vi.getTimerCount()).toBe(0);
});

it.each(['resolve', 'reject'] as const)('sprite_decode_clears_timeout (%s)', async outcome => {
  vi.useFakeTimers(); const decode = vi.fn();
  if (outcome === 'resolve') decode.mockResolvedValue(undefined);
  else decode.mockRejectedValue(new Error('Synthetic decode failure'));
  vi.stubGlobal('Image', class { decode = decode; });
  const clear = vi.spyOn(globalThis, 'clearTimeout');
  if (outcome === 'resolve') await expect(loadSprite(sprite)).resolves.toBeDefined();
  else await expect(loadSprite(sprite)).rejects.toThrow('Synthetic decode failure');
  expect(clear).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0);
});

it('sprite_success_draws_image', async () => {
  const { ctx } = canvas(), image = { naturalWidth: 8, naturalHeight: 8 } as HTMLImageElement;
  const model = recapImageModel(runView(), options, context), fights = model.blocks.find(block => block.kind === 'fights')!;
  const loader = vi.fn().mockResolvedValue(image); await renderRecapImage(model, loader);
  expect(loader).toHaveBeenCalledExactlyOnceWith(sprite);
  expect(ctx.drawImage).toHaveBeenCalledWith(image, 24, fights.y + 48, 32, 32);
  expect(ctx.fillText).toHaveBeenCalledWith('Synthetic object 30', 66, fights.y + 64);
  expect(ctx.imageSmoothingEnabled).toBe(false);
});

it('truncates_names_by_code_point', async () => {
  const view = runView(); view.fights[0].name = 'A' + '😀'.repeat(40);
  const { ctx } = canvas(), model = recapImageModel(view, options, { ...context, spriteUrl: () => null, objectName: () => null });
  const fights = model.blocks.find(block => block.kind === 'fights')!;
  await renderRecapImage(model, vi.fn());
  expect(ctx.fillText).toHaveBeenCalledWith('A' + '😀'.repeat(22) + '…', 24, fights.y + 64);
});

import { dateRange } from '../../src/lib/runs/filters';
import { expect, it } from 'vitest';
import { emptyListState, normalizeListState, type LootPosition } from '../../src/lib/navigation/state';

const defaults = { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 };
const valid: LootPosition = {
  range: null,
  feed: { filter: { min_rank: 255, place: { kind: 'dungeon', name: 'Synthetic' }, item_type: 65535,
    enchanted_only: true, range: { start: '0', end: '9223372036854775807' }, cursor: { at: '12', id: 'bag' }, limit: 10 }, previous: [null, { at: '15', id: 'older' }], selectedDropId: null },
  item: { type: 0, previous: [null, { at: '12', id: 'bag' }] }, statsRange: { first: '2026-10-01', last: '2026-10-05' }, collectionDungeon: 'Synthetic',
};
const normalize = (lootPosition: unknown) => normalizeListState({ lootPosition }).lootPosition;
it.each(['2026-13-45', '2026-02-30', '0000-00-00', '0000-01-01'])('rejects impossible stats date %s in either endpoint', day => {
  for (const statsRange of [{ first: day, last: '9999-12-31' }, { first: '2026-01-01', last: day }]) {
    expect(normalize({ ...valid, statsRange })?.statsRange).toBeNull();
  }
});
it('rejects reversed stats dates', () => {
  expect(normalize({ ...valid, statsRange: { first: '2026-10-05', last: '2026-10-01' } })?.statsRange).toBeNull();
});
it('preserves a valid leap day and equal stats endpoints', () => {
  const statsRange = { first: '2028-02-29', last: '2028-02-29' };
  expect(normalize({ ...valid, statsRange })?.statsRange).toEqual(statsRange);
});
it('defaults absent and malformed positions to null', () => {
  expect(emptyListState().lootPosition).toBeNull();
  for (const value of [undefined, null, [], 1, 'bad']) expect(normalize(value)).toBeNull();
});
it('round-trips valid state without retaining unknown fields or references', () => {
  const input = structuredClone(valid);
  const output = normalize({ ...input, extra: 'discard' });
  expect(output).toEqual(valid);
  input.feed.filter.cursor!.id = 'changed';
  input.item!.previous[1]!.id = 'changed';
  expect(output).toEqual(valid);
});
it('normalizes malformed fields individually and caps previous cursors at 100', () => {
  const output = normalize({ feed: { filter: { min_rank: 256, place: { kind: 'other' }, item_type: -1,
    enchanted_only: 'true', range: { start: '2', end: '1' }, cursor: { at: 'bad', id: 'bag' }, limit: 999 }, previous: Array(150).fill(null) },
    item: { type: 65536, previous: [] }, statsRange: { first: 'yesterday', last: '2026-10-05' }, collectionDungeon: 'unsafe\u0000' });
  expect(output).toEqual({ feed: { filter: defaults, previous: Array(100).fill(null), selectedDropId: null }, item: null, range: null, statsRange: null, collectionDungeon: null });
});
it.each([{ at: '-1', id: 'bag' }, { at: '01', id: 'bag' }, { at: '9223372036854775808', id: 'bag' },
  { at: '1', id: '\u0080' }, { at: '1', id: 'é'.repeat(129) }, { at: '1', id: '' }])('rejects unsafe cursors %j', cursor => {
  const output = normalize({ ...valid, feed: { filter: { ...valid.feed.filter, cursor }, previous: [cursor] }, item: { type: 7, previous: [cursor] } });
  expect(output?.feed.filter.cursor).toBeNull();
  expect(output?.feed.previous).toEqual([]);
  expect(output?.item?.previous).toEqual([]);
  expect(output?.statsRange).toEqual(valid.statsRange);
});
it.each([0, 51, 1.5, '10', null])('defaults invalid limit %j', limit => {
  expect(normalize({ ...valid, feed: { ...valid.feed, filter: { ...valid.feed.filter, limit } } })?.feed.filter.limit).toBe(50);
});
it.each([1, 50])('preserves valid limit %s and outside place', limit => {
  expect(normalize({ ...valid, feed: { ...valid.feed, filter: { ...valid.feed.filter, limit, place: { kind: 'outside' } } } })?.feed.filter).toEqual({ ...valid.feed.filter, limit, place: { kind: 'outside' } });
});
it('defaults incomplete position fields and invalid ranges safely', () => {
  expect(normalize({})).toEqual({ feed: { filter: defaults, previous: [], selectedDropId: null }, item: null, range: null, statsRange: null, collectionDungeon: null });
  for (const range of [{ start: 'no', end: '2' }, { start: '1', end: '1' }, { start: '1', end: '9223372036854775808' }]) {
    expect(normalize({ ...valid, feed: { ...valid.feed, filter: { ...valid.feed.filter, range, place: { kind: 'dungeon', name: '' } } } })?.feed.filter).toEqual({ ...valid.feed.filter, range: null, place: null });
  }
});
it('caps item previous cursors and strips extra cursor fields', () => {
  const cursor = { at: '1', id: 'bag', extra: 'discard' };
  const output = normalize({ ...valid, item: { type: 7, previous: Array(150).fill(cursor) } });
  expect(output?.item).toEqual({ type: 7, previous: Array(100).fill({ at: '1', id: 'bag' }) });
});

it('migrates legacy stats dates into the shared range', () => {
  expect(normalize({ statsRange: { first: '2026-09-20', last: '2026-10-02' } })?.range).toEqual(dateRange('2026-09-20', '2026-10-02'));
});
it('prefers a legacy feed range over stats dates', () => {
  const { range: _range, ...legacy } = valid;
  expect(normalize(legacy)?.range).toEqual(valid.feed.filter.range);
});
it('preserves an explicit shared all-time range and rejects malformed shared ranges', () => {
  expect(normalize(valid)?.range).toBeNull();
  expect(normalize({ ...valid, range: { start: '2', end: '1' } })?.range).toBeNull();
});

it('normalizes legacy selection to null and retains only safe selected drop ids', () => {
  expect(normalize({ feed: { filter: defaults, previous: [] } })?.feed.selectedDropId).toBeNull();
  expect(normalize({ feed: { selectedDropId: 'synthetic-bag', seedApplied: true } })?.feed).toMatchObject({ selectedDropId: 'synthetic-bag', seedApplied: true });
  for (const selectedDropId of ['', 'x'.repeat(257), 7, 'unsafe\u0000']) expect(normalize({ feed: { selectedDropId } })?.feed.selectedDropId).toBeNull();
});

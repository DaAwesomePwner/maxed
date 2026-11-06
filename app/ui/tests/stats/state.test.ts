import { expect, it } from 'vitest';
import { defaultStatsPosition, normalizeListState } from '../../src/lib/navigation/state';

const valid = {
  range: '30d', characterId: 'preview-alpha', mapSort: { key: 'fame', descending: false }, minMapRuns: 5, includeEmpty: true,
  sessions: { cursor: { started: '5', id: 's5' }, previous: [null, { started: '9', id: 's9' }] },
};
it('keeps a valid stats position', () => {
  expect(normalizeListState({ statsPosition: valid }).statsPosition).toEqual(valid);
});
it.each([
  { ...valid, range: 'year' },
  { ...valid, characterId: 'bad\u0000id' },
  { ...valid, mapSort: { key: 'name', descending: true } },
  { ...valid, includeEmpty: 'yes' },
  { ...valid, sessions: { cursor: { started: '-1', id: 's' }, previous: [] } },
  { ...valid, sessions: { cursor: null, previous: [{ started: '1', id: '' }] } },
])('drops a malformed stats position %#', statsPosition => {
  expect(normalizeListState({ statsPosition }).statsPosition).toBeNull();
});
it('defaults to all time, all characters, fame per hour descending', () => {
  expect(defaultStatsPosition()).toEqual({ range: 'all', characterId: null, mapSort: { key: 'rate', descending: true },
    minMapRuns: 5, includeEmpty: false, sessions: { cursor: null, previous: [] } });
});

import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { advance, click, deferred, fight, flush, follow, mountRuns, ready, region, run, saved } from '../runs/fixtures';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });
const primary = () => within(screen.getByRole('navigation', { name: 'Navigation' }));
const history = () => within(screen.getByRole('group', { name: 'History' }));
const fightHref = '#/runs/run-1/fights/fight-1';
const parentLink = (kind: 'fight' | 'player') => {
  const scope = within(screen.getByRole('main'));
  if (kind === 'fight') return scope.queryByRole('link', { name: 'Run' });
  const crumbs = scope.queryByRole('navigation', { name: 'Breadcrumb' });
  return crumbs ? within(crumbs).queryAllByRole('link').find(link => link.getAttribute('href') === fightHref) ?? null : null;
};
const assertParent = (kind: 'fight' | 'player') => expect(parentLink(kind)?.getAttribute('href')).toBe(kind === 'fight' ? '#/runs/run-1' : fightHref);

it.each(['fight', 'player'] as const)('nested %s accepts only matching context and retires revised relationships', async kind => {
  const pending = deferred<HistoryReply<FightView | null>>();
  const page = mountRuns({ hash: fightHref + (kind === 'player' ? '/players/7' : ''), queries: { getFight: () => pending.promise } });
  await flush();
  expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
  expect(parentLink(kind)).toBeNull();
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  pending.resolve(ready(fight())); await flush();
  assertParent(kind);
  page.queries.getFight.mockResolvedValue(ready(fight({ run_id: 'different-run' })));
  page.event({ run_updated: run() }, '13'); page.setStore(saved('13')); await advance();
  expect(parentLink(kind)).toBeNull();
  expect(screen.getByRole('heading', { name: kind === 'fight' ? 'Fight Unavailable' : 'Player Unavailable' })).toBeTruthy();
  expect(page.queries.getFight).toHaveBeenCalledTimes(2);
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledTimes(kind === 'player' ? 1 : 0);
});

it.each([
  ['fight', 'run'], ['fight', 'fight'], ['fight', 'missing'],
  ['player', 'run'], ['player', 'fight'], ['player', 'missing'],
] as const)('%s rejects %s mismatch or missing context and accepts recovery', async (kind, mismatch) => {
  const value = mismatch === 'missing' ? null : fight(mismatch === 'run' ? { run_id: 'other-run' } : { id: 'other-fight' });
  const page = mountRuns({ hash: fightHref + (kind === 'player' ? '/players/7' : ''), queries: { getFight: async () => ready(value) } });
  await flush();
  expect(parentLink(kind)).toBeNull();
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  page.queries.getFight.mockResolvedValue(ready(fight()));
  page.event({ run_updated: run() }, '13'); page.setStore(saved('13')); await advance();
  assertParent(kind);
  expect(page.queries.getFight).toHaveBeenCalledTimes(2);
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledTimes(kind === 'player' ? 1 : 0);
});

it.each(['entry', 'namespace'] as const)('late %s context cannot validate the current route', async retirement => {
  const old = deferred<HistoryReply<FightView | null>>(), current = deferred<HistoryReply<FightView | null>>();
  const page = mountRuns({ hash: fightHref, queries: { getFight: () => old.promise } });
  await flush();
  page.queries.getFight.mockImplementation(() => current.promise);
  if (retirement === 'entry') {
    await follow('#/runs', primary()); await click('Back', history());
  } else {
    const snapshot = { ...page.snapshot, stream_id: 'replacement-detail-namespace', sequence: '1' };
    page.setStore(saved('1', snapshot.stream_id)); page.publish(snapshot); await flush();
    await follow('#/runs', primary()); await follow('#/runs/run-1'); await fireEvent.click(screen.getAllByRole('tab')[1]); await flush(); await follow(fightHref, region('Fight damage'));
  }
  expect(page.queries.getFight).toHaveBeenCalledTimes(2);
  old.resolve(ready(fight())); await flush();
  expect(parentLink('fight')).toBeNull();
  current.resolve(ready(fight())); await flush();
  assertParent('fight');
});

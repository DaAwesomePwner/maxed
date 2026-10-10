import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { createHistoryCache, type HistoryCache } from '../../src/lib/runs/history-cache';
import { invalidatesSavedHistory } from '../../src/lib/shell/history-invalidation';
import { deferred, homeSnapshot, microtasks, mountPhase2, saved } from '../phase2/fixtures';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

const cache = () => createHistoryCache({ invalidates: invalidatesSavedHistory, route: () => ({ kind: 'runs' }) });
function seed(history: HistoryCache) {
  history.put('loot', { query: { kind: 'drops', filter: { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 20 } }, route: { kind: 'loot', view: 'feed' }, required: null, value: { kind: 'drops', page: { rows: [], next: null } } });
  history.put('runs', { query: { kind: 'runs', filter: { dungeon: null, outcome: null, range: null, sort: 'newest', cursor: null, limit: 20 } }, route: { kind: 'runs' }, required: null, value: { kind: 'runs', page: { rows: [], next: null } } });
}
function dirty(history: HistoryCache, expected: boolean) {
  for (const key of ['loot', 'runs']) expect(history.get(key)?.dirtyAll).toBe(expected);
}

it('bag_event_dirties_cached_loot_entry_while_on_runs', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  const sequence = String(BigInt(page.snapshot.sequence) + 1n);
  page.event({ bag_dropped: homeSnapshot().drops[0] }, sequence);
  await microtasks();
  expect(historyCache.get('loot')?.dirtySince).toBe(sequence);
  expect(historyCache.get('runs')?.dirtySince).toBe(sequence);
});

it('chat_settings_and_clear_invalidate_all', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks();
  const settingsClient = { ...page.client };
  seed(historyCache);
  await settingsClient.updateChatSettings(page.settings.chat);
  dirty(historyCache, true);
  seed(historyCache);
  await settingsClient.clearChatHistory();
  dirty(historyCache, true);
  seed(historyCache);
  vi.spyOn(page.transport, 'invoke').mockRejectedValueOnce(new Error('Synthetic clear failure'));
  await expect(settingsClient.clearChatHistory()).rejects.toThrow('Synthetic clear failure');
  dirty(historyCache, true);
});

it('fingerprint_change_invalidates_all', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  page.visuals.mockResolvedValue({ client_fingerprint: 'changed-invented', objects: [] });
  page.publish({ ...page.snapshot, sequence: String(BigInt(page.snapshot.sequence) + 1n),
    character: { ...page.snapshot.character!, class_id: 999 } });
  await microtasks();
  dirty(historyCache, true);
});

it('stream_change_clears', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  page.publish({ ...page.snapshot, stream_id: 'next-invented', sequence: '1' });
  await microtasks();
  expect(historyCache.get('loot')).toBeUndefined();
  expect(historyCache.get('runs')).toBeUndefined();
});

it('visual_key_change_with_same_fingerprint_keeps_entries_clean', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  page.visuals.mockClear();
  page.publish({ ...page.snapshot, sequence: String(BigInt(page.snapshot.sequence) + 1n),
    character: { ...page.snapshot.character!, class_id: 999 } });
  dirty(historyCache, false);
  await microtasks();
  expect(page.visuals).toHaveBeenCalledOnce();
  expect(await page.visuals.mock.results[0].value).toMatchObject({ client_fingerprint: 'phase2-invented' });
  dirty(historyCache, false);
});

it('connecting_epoch_clears_before_a_new_snapshot_arrives', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  const pending = deferred<typeof page.snapshot>();
  vi.spyOn(page.client, 'querySnapshot').mockReturnValueOnce(pending.promise);
  const next = { ...page.snapshot, stream_id: 'next-invented', sequence: '1' };
  page.transport.publishEngineEvent({ stream_id: next.stream_id, sequence: next.sequence,
    at: next.at, event: { capture_state_changed: next.capture } });
  expect(historyCache.get('loot')).toBeUndefined();
  expect(historyCache.get('runs')).toBeUndefined();
  pending.resolve(next);
  await microtasks();
});

it('visual_mutations_invalidate_and_unmount_releases_listeners', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  await expect(page.client.setAssetsPath('synthetic-assets')).rejects.toThrow('Preview Only');
  await microtasks(); dirty(historyCache, true);
  page.unmount(); seed(historyCache);
  await expect(page.client.setAssetsPath('other-synthetic-assets')).rejects.toThrow('Preview Only');
  await page.client.clearChatHistory();
  dirty(historyCache, false);
});

it('preview_change_clears_and_reconnect_replaces_listeners', async () => {
  const historyCache = cache(), invalidated = vi.spyOn(historyCache, 'invalidateAll');
  const page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  await page.rerender({ preview: true });
  await microtasks();
  expect(historyCache.get('loot')).toBeUndefined();
  expect(historyCache.get('runs')).toBeUndefined();
  seed(historyCache); invalidated.mockClear();
  await page.client.clearChatHistory();
  expect(invalidated).toHaveBeenCalledTimes(1);
  dirty(historyCache, true);
});

it('history_mutations_notify_after_settling_including_failed_settings', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  let reject!: (reason: Error) => void;
  vi.spyOn(page.transport, 'invoke').mockImplementationOnce(() => new Promise((_resolve, fail) => { reject = fail; }));
  const pending = page.client.updateChatSettings(page.settings.chat);
  dirty(historyCache, false);
  reject(new Error('Synthetic settings failure'));
  await expect(pending).rejects.toThrow('Synthetic settings failure');
  dirty(historyCache, true);
});


it('sequence_gap_resync_marks_cached_entries_dirty', async () => {
  const historyCache = cache(), page = mountPhase2({ historyCache });
  await microtasks(); seed(historyCache);
  const pending = deferred<typeof page.snapshot>();
  vi.spyOn(page.client, 'querySnapshot').mockReturnValueOnce(pending.promise);
  page.event({ capture_state_changed: page.snapshot.capture }, String(BigInt(page.snapshot.sequence) + 2n));
  dirty(historyCache, true);
  pending.resolve(page.snapshot); await microtasks();
});

it.each(['chat', 'events'])('shared_%s_summary_refreshes_on_another_route', async kind => {
  const method = kind === 'chat' ? 'getChatOverview' : 'getEventSummary';
  const page = mountPhase2({ hash: kind === 'chat' ? '#/logs/chat/guild' : '#/logs/keys',
    configure(client) { vi.spyOn(client, method); } });
  await microtasks();
  expect(page.client[method]).toHaveBeenCalledOnce();
  if (kind === 'chat') page.event({ chat: { id: 'synthetic-chat', session_id: 'synthetic', run_id: null,
    map: null, at: '1', channel: 'party', direction: null, sender_kind: 'player', sender: 'Synthetic',
    counterpart: null, is_local: false, stars: null, supporter: false, text: 'hello', truncated: false } }, '13');
  else page.event({ instance_event: { id: 'synthetic-left', session_id: 'synthetic', run_id: null,
    map: null, at: '1', provisional: true,
    kind: { type: 'player_left', object_id: 1, player_name: 'Synthetic', class_id: null } } }, '13');
  await microtasks();
  expect(page.client[method]).toHaveBeenCalledOnce();
  const href = kind === 'chat' ? '#/logs/chat/party' : '#/logs/events';
  const link = screen.getAllByRole('link').find(link => link.getAttribute('href') === href)!;
  await fireEvent.click(link); await microtasks();
  expect(page.client[method]).toHaveBeenCalledTimes(2);
});

it('party_chat_while_in_settings_refreshes_guild_overview_on_return', async () => {
  const page = mountPhase2({ hash: '#/logs/chat/guild', configure(client) { vi.spyOn(client, 'getChatOverview'); } });
  await microtasks();
  expect(page.client.getChatOverview).toHaveBeenCalledOnce();
  const settings = screen.getAllByRole('link').find(link => link.getAttribute('href') === '#/settings')!;
  await fireEvent.click(settings); await microtasks();
  expect(page.browser.adapter.hash).toBe('#/settings');
  page.event({ chat: { id: 'synthetic-chat', session_id: 'synthetic', run_id: null,
    map: null, at: '1', channel: 'party', direction: null, sender_kind: 'player', sender: 'Synthetic',
    counterpart: null, is_local: false, stars: null, supporter: false, text: 'hello', truncated: false } }, '13');
  page.setStore(saved('13', page.snapshot.stream_id));
  await microtasks();
  expect(page.client.getChatOverview).toHaveBeenCalledOnce();
  page.browser.adapter.go(-1); await microtasks();
  expect(page.browser.adapter.hash).toBe('#/logs/chat/guild');
  expect(page.client.getChatOverview).toHaveBeenCalledTimes(2);
});

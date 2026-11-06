import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { AlertPage } from '../../src/lib/bindings/AlertPage';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { advance, alert, click, deferred, disabled, edit, fight, flush, follow, injectChicagoDate,
  main, microtasks, mountPhase2, preferences, ready, saved, traverse } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

const primary = () => within(screen.getByRole('complementary', { name: 'Character' }));
const history = () => within(screen.getByRole('group', { name: 'History' }));

const log = () => main().getByRole('region', { name: /alert log/i });
const logRows = () => [...log().querySelectorAll<HTMLElement>('[data-row-id]')];

it('phase2_alert_policy_disabled_os_and_history', async () => {
  const rows = Array.from({ length: 50 }, (_, i) => alert({ id: `log-${i}`, explanation: `Saved invented moment ${i}`, details: i === 0 ? null : alert().details }));
  const page = mountPhase2({ hash: '#/alerts', alerts: async () => ready({ rows, next: null }) }); await flush();
  expect(page.listAlerts).toHaveBeenCalledWith(expect.objectContaining({ limit: 50 }));
  expect(logRows()).toHaveLength(50);
  expect(new Set(logRows().map(row => row.dataset.rowId)).size).toBe(50);
  expect(page.listAlerts).toHaveBeenCalledTimes(1);
  await follow('#/settings', primary()); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  for (const name of [/white bag/i, /personal best/i]) {
    const select = main().getByRole('combobox', { name });
    expect([...select.querySelectorAll('option')].map(option => option.value)).toEqual(['off', 'quiet_log_only', 'toast', 'banner']);
  }
  const windows = main().getByRole('checkbox', { name: /Windows Notifications/i });
  expect(disabled(windows)).toBe(true); expect((windows as HTMLInputElement).checked).toBe(false);
  expect(main().queryByText(/silent Windows Notifications/i)).toBeNull();
  expect(page.permission).not.toHaveBeenCalled(); expect(page.sound).not.toHaveBeenCalled();
  expect(screen.queryByRole('button', { name: /dismiss.*(moment|alert)/i })).toBeNull();
});

it.each([
  ['2026-03-08', '2026-03-08T06:00:00Z', '2026-03-09T05:00:00Z', 23],
  ['2026-11-01', '2026-11-01T05:00:00Z', '2026-11-02T06:00:00Z', 25],
] as const)('phase2_alert_filter_dst_cursor_and_literal_search (%s)', async (day, start, end, hours) => {
  injectChicagoDate();
  const page = mountPhase2({ hash: '#/alerts' }); await flush();
  await click('Custom…'); await edit('From', day); await edit('Through', day); await click('Use dates'); await edit('Search alerts', '  100%_literal  ');
  const before = page.listAlerts.mock.calls.length;
  expect(before).toBe(2); await advance(250);
  const filter = page.listAlerts.mock.lastCall![0];
  expect(filter).toMatchObject({ query: '100%_literal', cursor: null, limit: 50,
    range: { start: String(BigInt(Date.parse(start)) * 1000n), end: String(BigInt(Date.parse(end)) * 1000n) } });
  expect(BigInt(filter.range!.end) - BigInt(filter.range!.start)).toBe(BigInt(hours) * 3600000000n);
  await edit('Search alerts', 'x'.repeat(129)); await advance(250);
  expect(page.listAlerts).toHaveBeenCalledTimes(before + 1);
  expect(main().getByText('Invalid search.')).toBeTruthy();
  await edit('Search alerts', '\ud83d\ude80'.repeat(128)); await advance(250);
  expect(page.listAlerts.mock.lastCall![0].query).toBe('\ud83d\ude80'.repeat(128));
});

it('alert filters, native cursor and selection restore on Back', async () => {
  const cursor = { at: '4000000', id: 'cursor-1' };
  const page = mountPhase2({ hash: '#/alerts', alerts: async filter => ready({
    rows: Array.from({ length: 8 }, (_, i) => alert({ id: `${filter.cursor ? 'next' : 'first'}-${i}`, explanation: `Invented alert ${i}` })),
    next: filter.cursor ? null : cursor,
  }) }); await flush();
  await edit('Search alerts', 'Invented'); await advance(250);
  await click('Older alerts');
  const focused = logRows()[0];
  const action = focused.querySelector<HTMLElement>('a,button,[tabindex]') ?? focused;
  action.focus(); const id = focused.dataset.rowId;
  await follow('#/settings', primary()); await click('Back', history());
  expect(page.listAlerts.mock.lastCall![0]).toMatchObject({ query: 'Invented', cursor });
  expect(document.activeElement?.closest<HTMLElement>('[data-row-id]')?.dataset.rowId).toBe(id);
  expect(page.browser.adapter.state).toMatchObject({ alertsPosition: { previous: [null] } });
  expect(page.listAlerts.mock.lastCall![0].cursor).toEqual(cursor);
  await edit('Kind', 'death');
  expect(page.listAlerts.mock.lastCall![0]).toMatchObject({ kind: 'death', cursor: null });
  await advance(250); // router.save persists the browser entry after its debounce.
  expect(page.browser.adapter.state).toMatchObject({ alertsPosition: { previous: [], tile: 'all' } });
});

it('phase2_alert_quiet_source_revision_durable_invalidation integrated with the real log', async () => {
  const page = mountPhase2({ hash: '#/alerts' }); await flush();
  page.event({ fight_ended: fight() }, '13');
  page.event({ fight_ended: fight() }, '13');
  page.event({ drop_updated: page.snapshot.drops[0] }, '14'); await flush();
  expect(page.listAlerts).toHaveBeenCalledTimes(1);
  page.setStore(saved('13', 'live-phase2')); await advance(); expect(page.listAlerts).toHaveBeenCalledTimes(1);
  page.setStore(saved('14', 'live-phase2')); await advance(); expect(page.listAlerts).toHaveBeenCalledTimes(2);
  expect(page.sound).not.toHaveBeenCalled(); expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  const pending = deferred<HistoryReply<AlertPage>>(); page.listAlerts.mockImplementationOnce(() => pending.promise);
  page.event({ character_merged: { provisional_id: 'old', verified_id: 'new' } }, '15');
  page.setStore(saved('15', 'live-phase2')); await advance(); expect(page.listAlerts).toHaveBeenCalledTimes(3);
  page.event({ player_died: { at: '10000000', character_id: null, run_id: 'run-1' } }, '16');
  page.setStore(saved('16', 'live-phase2')); await advance(); expect(page.listAlerts).toHaveBeenCalledTimes(3);
  pending.resolve(ready({ rows: [alert({ explanation: 'Captured durable checkpoint' })], next: null })); await flush();
  expect(page.listAlerts).toHaveBeenCalledTimes(4);
  page.setStore({ ...saved('16', 'live-phase2'), state: 'failed', message: 'Storage unavailable' });
  page.event({ fight_ended: fight() }, '17'); await advance();
  expect(logRows()).toHaveLength(1); expect(main().getAllByText(/Couldn.t save/).length).toBeGreaterThan(0);
  page.event({ fight_updated: fight() }, '18');
  page.publish({ ...page.snapshot, sequence: '19', at: '11000000' }); await advance();
  expect(page.listAlerts).toHaveBeenCalledTimes(4);
});

it('phase2_alert_watermark_busy_and_namespace', async () => {
  const page = mountPhase2({ hash: '#/alerts', alerts: async () => ({ status: 'busy' }) }); await flush();
  await advance(5000); expect(page.listAlerts).toHaveBeenCalledTimes(2);
  await advance(60000); expect(page.listAlerts).toHaveBeenCalledTimes(2);
  expect(main().getByRole('button', { name: /Retry/i })).toBeTruthy();
  const pending = deferred<HistoryReply<AlertPage>>(); page.listAlerts.mockImplementationOnce(() => pending.promise);
  await click(/Retry/i); expect(page.listAlerts).toHaveBeenCalledTimes(3);
  page.setStore(saved('1', 'replacement'));
  page.publish({ ...page.snapshot, stream_id: 'replacement', sequence: '1' }); await flush();
  pending.resolve(ready({ rows: [alert({ explanation: 'Retired private result' })], next: null })); await flush();
  expect(screen.queryByText('Retired private result')).toBeNull();
  page.unmount(); await microtasks(); expect(vi.getTimerCount()).toBe(0);
});

it('Superseded is silent and never retries itself', async () => {
  const page = mountPhase2({ hash: '#/alerts', alerts: async () => ({ status: 'superseded' }) }); await flush();
  await advance(60000); expect(page.listAlerts).toHaveBeenCalledTimes(1);
  expect(main().queryByRole('alert')).toBeNull();
});

it('phase2_alert_preferences_revision_and_destinations', async () => {
  const rows = [alert(), alert({ id: 'death', kind: 'death', details: { type: 'death', character_id: null } }),
    alert({ id: 'bag', kind: 'white_bag', details: { type: 'white_bag', drop_id: 'bag-1', bag_type: 200,
      ordinal: null, window: null, source_enemy: null, source_estimated: true, contents_known: false } }),
    alert({ id: 'legacy', details: null, run_id: null, explanation: '<img src=x onerror=alert(1)>' })];
  const pending = deferred<void>();
  const page = mountPhase2({ hash: '#/alerts', alerts: async () => ready({ rows, next: null }), configure: client => {
    client.updateAlertPreferences = vi.fn(() => pending.promise);
  } }); await flush();
  const hrefs = [...log().querySelectorAll('a')].map(link => link.getAttribute('href'));
  expect(hrefs).toContain('#/runs/run-1/fights/fight-1'); expect(hrefs).toContain('#/runs/run-1/death');
  expect(hrefs).toContain('#/runs/run-1'); expect(hrefs.some(href => href?.includes('/loot'))).toBe(false);
  expect(log().querySelector('img[src="x"]')).toBeNull(); expect(log().textContent).not.toContain('<img src=x onerror=alert(1)>');
  await follow('#/settings', primary()); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  const loudness = main().getByRole('combobox', { name: /personal best/i });
  await fireEvent.change(loudness, { target: { value: 'toast' } }); await click('Save');
  await fireEvent.change(loudness, { target: { value: 'quiet_log_only' } });
  pending.resolve(); await flush();
  expect((loudness as HTMLSelectElement).value).toBe('quiet_log_only');
  expect(page.client.updateAlertPreferences).toHaveBeenCalledWith(preferences({ personal_best: 'toast' }));
  expect(page.client.updateSettings).not.toHaveBeenCalled(); expect(page.permission).not.toHaveBeenCalled();
});

it.each([true, false])('all test actions guard preview/demo and unsupported native delivery (demo=%s)', async demo => {
  const page = mountPhase2({ hash: '#/settings', demo }); await flush(); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  const tests = main().getAllByRole('button', { name: /test/i });
  expect(tests.length).toBeGreaterThanOrEqual(4);
  for (const button of tests) { expect(disabled(button)).toBe(true); await fireEvent.click(button); }
  expect(page.sound).not.toHaveBeenCalled(); expect(page.permission).not.toHaveBeenCalled();
  if (demo) {
    expect(disabled(main().getByRole('button', { name: 'Save' }))).toBe(true);
    expect(page.client.updateAlertPreferences).not.toHaveBeenCalled();
  }
});

it('eligible live per-kind tests use only the existing guarded native command', async () => {
  const page = mountPhase2({ hash: '#/alerts', configure: client => {
    client.queryAlertDelivery = vi.fn(async () => ({ permission: 'unavailable' as const, sound_available: true,
      message: null, flawless_unavailable: 'Flawless evidence is unavailable.' }));
    client.testAlertDelivery = vi.fn(async () => ({ permission: 'unavailable' as const, sound_available: true,
      message: 'Synthetic guarded sound accepted', flawless_unavailable: 'Flawless evidence is unavailable.' }));
  } }); await flush();
  await follow('#/settings', primary()); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  for (const [label, kind] of [[/test.*white bag/i, 'white_bag'], [/test.*personal best/i, 'personal_best'], [/test.*key popped/i, 'key_popped']] as const) {
    const button = main().getByRole('button', { name: label }); expect(disabled(button)).toBe(false);
    await fireEvent.click(button); await flush();
    expect(page.client.testAlertDelivery).toHaveBeenLastCalledWith(kind);
  }
  expect(disabled(main().getByRole('button', { name: /test.*flawless/i }))).toBe(true);
  expect(page.client.testAlertDelivery).toHaveBeenCalledTimes(3);
  expect(page.permission).not.toHaveBeenCalled(); expect(page.listAlerts).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  vi.mocked(page.client.testAlertDelivery).mockRejectedValueOnce(new Error('PRIVATE device failure'));
  await click(/test.*white bag/i);
  expect(main().getAllByText(/failed|unavailable|couldn.t/i).length).toBeGreaterThan(0);
  expect(document.body.textContent).not.toContain('PRIVATE');
});

it('available Windows Notifications save the toggle and test through guarded native delivery', async () => {
  const status = { permission: 'granted' as const, sound_available: false, message: null,
    flawless_unavailable: 'Flawless evidence is unavailable.' };
  const page = mountPhase2({ hash: '#/settings', configure: client => {
    client.queryAlertDelivery = vi.fn(async () => status);
    client.testAlertDelivery = vi.fn(async () => status);
  } }); await flush(); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  const windows = main().getByRole('checkbox', { name: /Windows Notifications/i });
  expect(disabled(windows)).toBe(false);
  const test = main().getByRole('button', { name: /^Test Notification$/ });
  expect(disabled(test)).toBe(true);
  await fireEvent.click(windows); await flush();
  await click('Save');
  expect(page.client.updateAlertPreferences).toHaveBeenLastCalledWith(expect.objectContaining({ windows_notifications: true }));
  expect(disabled(test)).toBe(false);
  await fireEvent.click(test); await flush();
  expect(page.client.testAlertDelivery).toHaveBeenCalledWith('white_bag');
  await fireEvent.change(main().getByRole('combobox', { name: /white bag/i }), { target: { value: 'off' } });
  await click('Save');
  await fireEvent.click(test); await flush();
  expect(page.client.testAlertDelivery).toHaveBeenLastCalledWith('personal_best');
  await fireEvent.change(main().getByRole('combobox', { name: /personal best/i }), { target: { value: 'quiet_log_only' } });
  await fireEvent.change(main().getByRole('combobox', { name: /^key popped$/i }), { target: { value: 'off' } });
  await click('Save');
  expect(disabled(test)).toBe(false);
  await fireEvent.click(test); await flush();
  expect(page.client.testAlertDelivery).toHaveBeenLastCalledWith('whisper');
  await fireEvent.change(main().getByRole('combobox', { name: /^whisper$/i }), { target: { value: 'off' } });
  await click('Save');
  expect(disabled(test)).toBe(true);
});

it('accepted alert preferences reconfigure the mounted host without replaying history', async () => {
  const page = mountPhase2({ hash: '#/settings' }); await flush(); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  page.event({ alert_fired: alert({ id: 'visible-before-save', explanation: 'Visible live moment' }) }, '13');
  page.event({ alert_fired: alert({ id: 'waiting-before-save', explanation: 'Waiting live moment' }) }, '14'); await flush();
  expect(screen.getByRole('button', { name: /dismiss/i })).toBeTruthy();
  await fireEvent.change(main().getByRole('combobox', { name: /personal best/i }), { target: { value: 'quiet_log_only' } });
  await click('Save');
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  await fireEvent.change(main().getByRole('combobox', { name: /personal best/i }), { target: { value: 'banner' } });
  await click('Save'); await advance(9000);
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  await follow('#/logs/alerts'); await follow('#/settings', primary()); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
  expect(page.client.updateSettings).not.toHaveBeenCalled();
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
});

it('Death has no preference or Test entry while legacy history still renders', async () => {
  mountPhase2({ hash: '#/alerts', alerts: async () => ready({ rows: [alert({ kind: 'death', details: { type: 'death', character_id: null } })], next: null }) }); await flush();
  expect(log().textContent).toContain('Death');
  await follow('#/settings', primary()); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  expect(main().queryByRole('combobox', { name: /^death/i })).toBeNull();
  expect(main().queryByRole('button', { name: /test.*death/i })).toBeNull();
});

it('Key popped exposes loudness, catalog dungeon selection and a guarded Test', async () => {
  const page = mountPhase2({ hash: '#/settings', configure: client => {
    client.listCatalogDungeonOptions = vi.fn(async () => ['Lost Halls', 'The Void'].map(name => ({ name, aliases: [], kind: 'dungeon' as const, completion_index: null })));
    client.queryAlertDelivery = vi.fn(async () => ({ permission: 'unavailable' as const, sound_available: true,
      message: null, flawless_unavailable: 'Flawless evidence is unavailable.' }));
  } }); await flush(); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  const level = main().getByRole('combobox', { name: 'Key Popped' });
  const filter = main().getByRole('combobox', { name: /key popped dungeons/i });
  expect((filter as HTMLSelectElement).value).toBe('all');
  await fireEvent.change(level, { target: { value: 'banner' } });
  await fireEvent.change(filter, { target: { value: 'selected' } }); await flush();
  await fireEvent.click(main().getByRole('checkbox', { name: 'Lost Halls' }));
  await click('Save');
  expect(page.client.updateAlertPreferences).toHaveBeenLastCalledWith(expect.objectContaining({ key_popped: 'banner', key_pop_dungeons: ['Lost Halls'] }));
  await click(/test.*key popped/i);
  expect(page.client.testAlertDelivery).toHaveBeenCalledWith('key_popped');
  await fireEvent.change(filter, { target: { value: 'all' } }); await click('Save');
  expect(page.client.updateAlertPreferences).toHaveBeenLastCalledWith(expect.objectContaining({ key_pop_dungeons: null }));
});


it('Key Popped Kind survives Back navigation', async () => {
  const page = mountPhase2({ hash: '#/alerts' }); await flush();
  await edit('Kind', 'key_popped'); await advance(250);
  await follow('#/settings', primary()); await click('Back', history());
  expect(page.listAlerts.mock.lastCall![0]).toMatchObject({ kind: 'key_popped' });
  expect((main().getByRole('combobox', { name: 'Kind' }) as HTMLSelectElement).value).toBe('key_popped');
});


it('saving a dungeon filter removes visible and queued key-pop banners without removing PB', async () => {
  const page = mountPhase2({ hash: '#/settings', configure: client => {
    client.listCatalogDungeonOptions = vi.fn(async () => ['Lost Halls', 'The Void'].map(name => ({ name, aliases: [], kind: 'dungeon' as const, completion_index: null })));
  } }); await flush(); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  await fireEvent.change(main().getByRole('combobox', { name: 'Key Popped' }), { target: { value: 'banner' } });
  await click('Save');
  const key = { kind: 'key_popped' as const, details: { type: 'key_popped' as const, dungeon: 'Lost Halls' }, run_id: null };
  page.event({ alert_fired: alert({ ...key, id: 'visible-key' }) }, '13');
  page.event({ alert_fired: alert({ ...key, id: 'waiting-key' }) }, '14');
  page.event({ alert_fired: alert({ id: 'waiting-pb' }) }, '15'); await flush();
  expect(screen.getByText('Lost Halls key popped')).toBeTruthy();
  await fireEvent.change(main().getByRole('combobox', { name: /key popped dungeons/i }), { target: { value: 'selected' } }); await flush();
  await fireEvent.click(main().getByRole('checkbox', { name: 'The Void' })); await click('Save');
  expect(page.client.updateAlertPreferences).toHaveBeenLastCalledWith(expect.objectContaining({ key_pop_dungeons: ['The Void'] }));
  expect(screen.queryByText('Lost Halls key popped')).toBeNull();
  const dismiss = screen.getByRole('button', { name: /dismiss/i });
  await fireEvent.click(dismiss); await flush();
  expect(screen.queryByRole('button', { name: /dismiss/i })).toBeNull();
});

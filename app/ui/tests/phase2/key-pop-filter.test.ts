import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { alert, deferred, flush, follow, main, mountPhase2, preferences, primary, ready } from './fixtures';
import { createAlertSession } from '../../src/lib/alerts/session';
import { get } from 'svelte/store';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });
const pop = () => alert({ kind: 'key_popped', details: { type: 'key_popped', dungeon: 'Lost Halls' }, run_id: null });

it.each([
  { only: true, undoBeforeSave: false }, { only: false, undoBeforeSave: false },
  { only: true, undoBeforeSave: true }, { only: false, undoBeforeSave: true },
])('translates banner Mute and Undo into the dirty mode (initial Only=$only, Undo before Save=$undoBeforeSave)', async ({ only, undoBeforeSave }) => {
  const page = mountPhase2({ hash: '#/settings', configure: client => {
    client.listCatalogDungeonOptions = vi.fn(async () => [{ name: 'Lost Halls', aliases: [], kind: 'dungeon' as const, completion_index: null, portal_type: null }]);
  } });
  page.setSettings({ alerts: preferences({ key_popped: 'banner', key_pop_dungeons: only ? ['Lost Halls'] : null }) });
  await flush();
  await fireEvent.change(main().getByRole('combobox', { name: 'White Bag' }), { target: { value: 'off' } });
  await fireEvent.change(main().getByRole('combobox', { name: 'Key Popped dungeons' }), { target: { value: only ? 'all' : 'selected' } });
  if (!only) await fireEvent.click(main().getByRole('checkbox', { name: 'Lost Halls' }));
  page.event({ alert_fired: pop() }, '13'); await flush();
  await fireEvent.click(within(screen.getByRole('group', { name: 'Alert' })).getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(screen.getByText('Muted Lost Halls.')).toBeTruthy();
  expect((main().getByRole('combobox', { name: 'Key Popped dungeons' }) as HTMLSelectElement).value).toBe(only ? 'except' : 'selected');
  expect(Boolean(main().queryByRole('button', { name: 'Remove Lost Halls' }))).toBe(only);
  if (undoBeforeSave) { await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush(); }
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  if (!undoBeforeSave) {
    expect(page.settings.alerts.key_pop_dungeons).toEqual(only ? null : []);
    if (only) expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['Lost Halls']);
    await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  }
  expect(screen.getByText('Restored Lost Halls.')).toBeTruthy();
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_dungeons).toEqual(only ? null : ['Lost Halls']);
  if (only) expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual([]);
});

it('blocks Save when translating a mute into the draft mode exceeds the combined cap', async () => {
  const names = Array.from({ length: 256 }, (_, i) => `Draft dungeon ${i}`);
  const page = mountPhase2({ hash: '#/settings', configure: client => {
    client.listCatalogDungeonOptions = vi.fn(async () => names.map(name => ({ name, aliases: [], kind: 'dungeon' as const, completion_index: null, portal_type: null })));
  } });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: ['Lost Halls'] }) }); await flush();
  await fireEvent.change(main().getByRole('combobox', { name: 'Key Popped dungeons' }), { target: { value: 'except' } });
  await fireEvent.click(main().getByRole('button', { name: 'Select all' }));
  page.event({ alert_fired: pop() }, '13'); await flush();
  await fireEvent.click(within(screen.getByRole('group', { name: 'Alert' })).getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(main().getByText(/256.*combined/i)).toBeTruthy();
  expect(page.client.updateAlertPreferences).toHaveBeenCalledTimes(1);
  await fireEvent.click(main().getByRole('button', { name: 'Remove Draft dungeon 0' }));
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toBeNull();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toHaveLength(256);
  expect(page.settings.alerts.key_pop_excluded_dungeons).toContain('Lost Halls');
});

it.each([false, true])('queues a dirty form save behind a pending mute and Undo reverses only the mute (Only=%s)', async only => {
  const page = mountPhase2({ hash: '#/settings',
    configure: client => { client.listCatalogDungeonOptions = vi.fn(async () => ['Lost Halls', 'The Void', 'Extra dungeon'].map(name => ({
      name, aliases: [], kind: 'dungeon' as const, completion_index: null, portal_type: null,
    }))); } });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: only ? ['Lost Halls', 'The Void'] : null,
    key_pop_excluded_dungeons: only ? [] : ['The Void'] }) });
  await flush();
  await fireEvent.change(main().getByRole('combobox', { name: 'White Bag' }), { target: { value: 'off' } });
  await fireEvent.click(main().getByRole('checkbox', { name: 'Extra dungeon' }));
  const muteWrite = deferred<void>();
  const persist = vi.mocked(page.client.updateAlertPreferences).getMockImplementation()!;
  vi.mocked(page.client.updateAlertPreferences).mockImplementationOnce(async next => { await muteWrite.promise; await persist(next); });
  page.event({ alert_fired: pop() }, '13'); await flush();
  await fireEvent.click(within(screen.getByRole('group', { name: 'Alert' })).getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(screen.queryByText('Muted Lost Halls.')).toBeNull();
  const save = main().getByRole('button', { name: 'Save' }) as HTMLButtonElement;
  expect(save.disabled).toBe(false);
  await fireEvent.click(save); await flush();
  expect(page.client.updateAlertPreferences).toHaveBeenCalledTimes(1);
  muteWrite.resolve(); await flush();
  expect(page.client.updateAlertPreferences).toHaveBeenCalledTimes(2);
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_dungeons).toEqual(only ? ['The Void', 'Extra dungeon'] : null);
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(only ? [] : ['The Void', 'Lost Halls', 'Extra dungeon']);
  expect(screen.getByText('Muted Lost Halls.')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_dungeons).toEqual(only ? ['Lost Halls', 'The Void', 'Extra dungeon'] : null);
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(only ? [] : ['The Void', 'Extra dungeon']);
  expect(screen.getByText('Restored Lost Halls.')).toBeTruthy();
});

it('searches names and aliases, edits chips and visible results, and retains mode choices', async () => {
  const page = mountPhase2({ hash: '#/settings', configure: client => {
    client.listCatalogDungeonOptions = vi.fn(async () => [
      { name: 'Lost Halls', aliases: ['LH'], kind: 'dungeon' as const, completion_index: null },
      { name: 'The Void', aliases: ['Void'], kind: 'dungeon' as const, completion_index: null },
    ]);
  } }); await flush();
  const mode = main().getByRole('combobox', { name: 'Key Popped dungeons' });
  await fireEvent.change(mode, { target: { value: 'selected' } }); await flush();
  const emptySelection = 'Choose dungeons to receive key-pop alerts. None selected means no alerts.';
  expect(within(main().getByRole('group', { name: 'Chosen dungeons' })).getByText(emptySelection)).toBeTruthy();
  expect(within(main().getByRole('group', { name: 'Chosen dungeons' })).queryByRole('alert')).toBeNull();
  const search = main().getByRole('searchbox', { name: 'Search dungeons' });
  await fireEvent.input(search, { target: { value: 'lh' } });
  expect(main().queryByRole('checkbox', { name: 'The Void' })).toBeNull();
  await fireEvent.click(main().getByRole('button', { name: 'Select all' }));
  expect(main().getByRole('button', { name: 'Remove Lost Halls' })).toBeTruthy();
  expect(main().queryByText(emptySelection)).toBeNull();
  await fireEvent.input(search, { target: { value: 'void' } });
  await fireEvent.click(main().getByRole('button', { name: 'Select all' }));
  await fireEvent.click(main().getByRole('button', { name: 'Clear' }));
  expect(main().getByRole('button', { name: 'Remove Lost Halls' })).toBeTruthy();
  expect(main().queryByRole('button', { name: 'Remove The Void' })).toBeNull();
  await fireEvent.change(mode, { target: { value: 'all' } });
  await fireEvent.change(mode, { target: { value: 'selected' } });
  expect(main().getByRole('button', { name: 'Remove Lost Halls' })).toBeTruthy();
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toEqual(['Lost Halls']);
  await fireEvent.click(main().getByRole('button', { name: 'Remove Lost Halls' }));
  expect(within(main().getByRole('group', { name: 'Chosen dungeons' })).getByText(emptySelection)).toBeTruthy();
  await fireEvent.change(mode, { target: { value: 'except' } });
  await fireEvent.click(main().getByRole('button', { name: 'Select all' }));
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toBeNull();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['The Void']);
});

it.each(['banner', 'toast', 'log'] as const)('mutes from %s, saves immediately and undoes after the alert disappears', async surface => {
  const page = mountPhase2({ hash: '#/logs/alerts', alerts: async () => ready({ rows: surface === 'log' ? [pop()] : [], next: null }) });
  page.setSettings({ alerts: preferences({ key_popped: surface === 'toast' ? 'toast' : 'banner' }) });
  await flush();
  if (surface !== 'log') { page.event({ alert_fired: pop() }, '13'); await flush(); }
  const area = surface === 'log' ? main().getByRole('region', { name: /alert log/i }) : screen.getByRole('group', { name: 'Alert' });
  await fireEvent.click(within(area).getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['Lost Halls']);
  expect(screen.getByText('Muted Lost Halls.')).toBeTruthy();
  await follow('#/settings', primary());
  expect((main().getByRole('combobox', { name: 'Key Popped dungeons' }) as HTMLSelectElement).value).toBe('except');
  expect(main().getByRole('button', { name: 'Remove Lost Halls' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual([]);
  expect(main().queryByRole('button', { name: 'Remove Lost Halls' })).toBeNull();
});

it('Only mute removes the dungeon; a failed save retains retry and Undo restores it', async () => {
  const page = mountPhase2({ hash: '#/logs/alerts', alerts: async () => ready({ rows: [pop()], next: null }) });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: ['Lost Halls', 'The Void'] }) });
  await flush();
  vi.mocked(page.client.updateAlertPreferences).mockRejectedValueOnce(new Error('Synthetic failure'));
  await fireEvent.click(main().getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toEqual(['Lost Halls', 'The Void']);
  expect(screen.getByText('Couldn’t save. Retry.')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry mute' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toEqual(['The Void']);
  await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toEqual(['Lost Halls', 'The Void']);
});

it('normalizes old preferences before editing or applying policy', async () => {
  const page = mountPhase2({ hash: '#/settings' });
  const old = preferences();
  delete (old as Partial<typeof old>).key_pop_excluded_dungeons;
  page.setSettings({ alerts: old });
  const session = createAlertSession(page.client); await session.refresh();
  expect(get(session.preferences)?.key_pop_excluded_dungeons).toEqual([]);
  session.dispose();
});

it('excluded dungeons suppress visible and queued pops while new dungeons still alert', async () => {
  const page = mountPhase2({ hash: '#/logs/alerts' });
  page.setSettings({ alerts: preferences({ key_pop_excluded_dungeons: ['Lost Halls'] }) });
  await flush();
  page.event({ alert_fired: pop() }, '13'); await flush();
  expect(screen.queryByRole('group', { name: 'Alert' })).toBeNull();
  page.event({ alert_fired: alert({ ...pop(), id: 'unknown', details: { type: 'key_popped', dungeon: 'Unknown dungeon' } }) }, '14'); await flush();
  expect(screen.getByRole('button', { name: 'Mute Unknown dungeon' })).toBeTruthy();
});

it('Undo preserves unrelated saved preferences and another excluded dungeon', async () => {
  const page = mountPhase2({ hash: '#/logs/alerts', alerts: async () => ready({ rows: [pop()], next: null }) });
  page.setSettings({ alerts: preferences({ key_pop_excluded_dungeons: ['The Void'] }) });
  await flush();
  await fireEvent.click(main().getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  await follow('#/settings', primary());
  await fireEvent.change(main().getByRole('combobox', { name: 'White Bag' }), { target: { value: 'off' } });
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['The Void']);
});

it('shows retry after failed readback and restores policy before retrying mute', async () => {
  const page = mountPhase2({ hash: '#/logs/alerts', alerts: async () => ready({ rows: [pop()], next: null }) });
  await flush();
  vi.mocked(page.client.querySettings).mockRejectedValueOnce(new Error('Synthetic readback failure'));
  await fireEvent.click(main().getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(screen.getByRole('button', { name: 'Retry mute' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry mute' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['Lost Halls']);
  await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual([]);
});

it('retry follows a newly saved filter mode and Undo reverses that retry', async () => {
  const page = mountPhase2({ hash: '#/logs/alerts', alerts: async () => ready({ rows: [pop()], next: null }) });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: ['Lost Halls'] }) });
  await flush();
  vi.mocked(page.client.updateAlertPreferences).mockRejectedValueOnce(new Error('Synthetic failure'));
  await fireEvent.click(main().getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  await follow('#/settings', primary());
  await fireEvent.change(main().getByRole('combobox', { name: 'Key Popped dungeons' }), { target: { value: 'all' } });
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry mute' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['Lost Halls']);
  await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual([]);
});

it('renders valid legacy selections with duplicate names as one removable chip', async () => {
  const page = mountPhase2({ hash: '#/settings' });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: ['Lost Halls', 'Lost Halls'] }) });
  await flush();
  expect(main().getAllByRole('button', { name: 'Remove Lost Halls' })).toHaveLength(1);
  await fireEvent.click(main().getByRole('button', { name: 'Remove Lost Halls' }));
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toEqual([]);
});

it.each([false, true])('rebases dirty settings onto a log mute (Only=%s)', async only => {
  const page = mountPhase2({ hash: '#/alerts',
    // Legacy combined Alerts page; unreachable with the production logs flag on.
    features: { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: false }, alerts: async () => ready({ rows: [pop()], next: null }),
    configure: client => { client.listCatalogDungeonOptions = vi.fn(async () => ['Lost Halls', 'The Void'].map(name => ({ name, aliases: [], kind: 'dungeon' as const, completion_index: null }))); } });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: only ? ['Lost Halls', 'The Void'] : null }) });
  await flush();
  await fireEvent.change(main().getByRole('combobox', { name: 'White Bag' }), { target: { value: 'off' } });
  if (only) {
    // Equal arrays with new identities are not local edits.
    await fireEvent.click(main().getByRole('checkbox', { name: 'The Void' }));
    await fireEvent.click(main().getByRole('checkbox', { name: 'The Void' }));
  }
  await fireEvent.click(main().getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  const mode = main().getByRole('combobox', { name: 'Key Popped dungeons' }) as HTMLSelectElement;
  expect(mode.value).toBe(only ? 'selected' : 'except');
  expect(Boolean(main().queryByRole('button', { name: 'Remove Lost Halls' }))).toBe(!only);
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_dungeons).toEqual(only ? ['The Void'] : null);
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(only ? [] : ['Lost Halls']);
});

it('preserves a saved block list when mounting in Only then switching to All except', async () => {
  const page = mountPhase2({ hash: '#/settings' });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: ['Lost Halls'], key_pop_excluded_dungeons: ['The Void'] }) });
  await flush();
  await fireEvent.change(main().getByRole('combobox', { name: 'Key Popped dungeons' }), { target: { value: 'except' } });
  expect(main().getByRole('button', { name: 'Remove The Void' })).toBeTruthy();
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['The Void']);
});

it.each([false, true])('merges dirty picker additions and removals with a log mute (Only=%s)', async only => {
  const names = ['Lost Halls', 'The Void', 'Old choice', 'Added B', 'Added A'];
  const page = mountPhase2({ hash: '#/alerts',
    // Legacy combined Alerts page; unreachable with the production logs flag on.
    features: { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: false }, alerts: async () => ready({ rows: [pop()], next: null }),
    configure: client => { client.listCatalogDungeonOptions = vi.fn(async () => names.map(name => ({ name, aliases: [], kind: 'dungeon' as const, completion_index: null }))); } });
  const initial = ['The Void', 'Old choice', 'The Void'];
  page.setSettings({ alerts: preferences({ key_pop_dungeons: only ? ['Lost Halls', ...initial] : null,
    key_pop_excluded_dungeons: only ? [] : initial }) });
  await flush();
  for (const name of ['Added B', 'Added A', 'Old choice']) {
    await fireEvent.click(main().getByRole('checkbox', { name }));
  }
  await fireEvent.click(main().getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(screen.getByText('Muted Lost Halls.')).toBeTruthy();
  expect(Boolean(main().queryByRole('button', { name: 'Remove Lost Halls' }))).toBe(!only);
  expect(main().getByRole('button', { name: 'Remove Added B' })).toBeTruthy();
  expect(main().queryByRole('button', { name: 'Remove Old choice' })).toBeNull();
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.key_pop_dungeons).toEqual(only ? ['The Void', 'Added B', 'Added A'] : null);
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(only ? [] : ['The Void', 'Lost Halls', 'Added B', 'Added A']);
});

it.each([false, true])('blocks saving an over-cap rebase until a selection is removed without losing the mute (queued=%s)', async queued => {
  const page = mountPhase2({ hash: '#/settings',
    configure: client => { client.listCatalogDungeonOptions = vi.fn(async () => [
      { name: 'Extra dungeon', aliases: [], kind: 'dungeon' as const, completion_index: null },
    ]); } });
  page.setSettings({ alerts: preferences({ key_pop_excluded_dungeons: Array.from({ length: 255 }, (_, i) => `Excluded ${i}`) }) });
  await flush();
  await fireEvent.click(main().getByRole('checkbox', { name: 'Extra dungeon' }));
  const muteWrite = deferred<void>();
  if (queued) {
    const persist = vi.mocked(page.client.updateAlertPreferences).getMockImplementation()!;
    vi.mocked(page.client.updateAlertPreferences).mockImplementationOnce(async next => { await muteWrite.promise; await persist(next); });
  }
  page.event({ alert_fired: pop() }, '13'); await flush();
  await fireEvent.click(within(screen.getByRole('group', { name: 'Alert' })).getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(page.client.updateAlertPreferences).toHaveBeenCalledTimes(1);
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  if (queued) { muteWrite.resolve(); await flush(); }
  expect(main().getByText(/256.*combined/i)).toBeTruthy();
  expect(page.client.updateAlertPreferences).toHaveBeenCalledTimes(1);
  await fireEvent.click(main().getByRole('button', { name: 'Remove Excluded 0' }));
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toHaveLength(256);
  expect(page.settings.alerts.key_pop_excluded_dungeons).toContain('Lost Halls');
  expect(page.settings.alerts.key_pop_excluded_dungeons).toContain('Extra dungeon');
  expect(page.settings.alerts.key_pop_excluded_dungeons).not.toContain('Excluded 0');
});

it('retains mute retry after a failed pending write while the queued form save succeeds', async () => {
  const page = mountPhase2({ hash: '#/settings' });
  await flush();
  await fireEvent.change(main().getByRole('combobox', { name: 'White Bag' }), { target: { value: 'off' } });
  const muteWrite = deferred<void>();
  vi.mocked(page.client.updateAlertPreferences).mockImplementationOnce(async () => {
    await muteWrite.promise; throw new Error('Synthetic persistence failure');
  });
  page.event({ alert_fired: pop() }, '13'); await flush();
  await fireEvent.click(within(screen.getByRole('group', { name: 'Alert' })).getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  muteWrite.resolve(); await flush();
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual([]);
  expect(screen.queryByText('Muted Lost Halls.')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry mute' })); await flush();
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual(['Lost Halls']);
  await fireEvent.click(screen.getByRole('button', { name: 'Undo' })); await flush();
  expect(page.settings.alerts.white_bag).toBe('off');
  expect(page.settings.alerts.key_pop_excluded_dungeons).toEqual([]);
});

it('rejects picker selections over the combined cap without creating an invalid draft', async () => {
  const page = mountPhase2({ hash: '#/settings', configure: client => {
    client.listCatalogDungeonOptions = vi.fn(async () => [
      { name: 'Extra dungeon', aliases: [], kind: 'dungeon' as const, completion_index: null },
    ]);
  } });
  page.setSettings({ alerts: preferences({ key_pop_dungeons: ['Lost Halls'],
    key_pop_excluded_dungeons: Array.from({ length: 255 }, (_, i) => `Excluded ${i}`) }) });
  await flush();
  await fireEvent.click(main().getByRole('checkbox', { name: 'Extra dungeon' })); await flush();
  expect(main().getByText(/256.*combined/i)).toBeTruthy();
  expect(main().queryByRole('button', { name: 'Remove Extra dungeon' })).toBeNull();
  expect((main().getByRole('checkbox', { name: 'Extra dungeon' }) as HTMLInputElement).checked).toBe(false);
  await fireEvent.click(main().getByRole('button', { name: 'Select all' })); await flush();
  expect(main().queryByRole('button', { name: 'Remove Extra dungeon' })).toBeNull();
  expect(page.client.updateAlertPreferences).not.toHaveBeenCalled();
});

it('reports a full filter when Mute would exceed the cap without attempting a save', async () => {
  const page = mountPhase2({ hash: '#/logs/alerts', alerts: async () => ready({ rows: [pop()], next: null }) });
  page.setSettings({ alerts: preferences({ key_pop_excluded_dungeons: Array.from({ length: 256 }, (_, i) => `Excluded ${i}`) }) });
  await flush();
  await fireEvent.click(main().getByRole('button', { name: 'Mute Lost Halls' })); await flush();
  expect(screen.getByText(/256.*combined/i)).toBeTruthy();
  expect(page.client.updateAlertPreferences).not.toHaveBeenCalled();
  expect(page.settings.alerts.key_pop_excluded_dungeons).toHaveLength(256);
});

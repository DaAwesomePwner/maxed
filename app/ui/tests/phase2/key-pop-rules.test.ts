import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { writable } from 'svelte/store';
import KeyPopRules from '../../src/lib/alerts/KeyPopRules.svelte';
import DungeonPicker from '../../src/lib/alerts/DungeonPicker.svelte';
import { keyPopAllows, allowsAlert, createAlertPreferences, emptyAlertPreferences } from '../../src/lib/alerts/preferences';
import { dungeonCatalog } from '../../src/lib/alerts/dungeon-catalog';
import { mutedDungeons } from '../../src/lib/logs/keys-screen';
import { dungeonDef } from '../fixtures/dungeon';
import { preferences, alert, flush } from './fixtures';
import type { AlertSession } from '../../src/lib/alerts/session';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); dungeonCatalog.set(undefined); vi.clearAllTimers(); vi.useRealTimers(); });
const meta = (difficulty: number | null, exalt = false, exalt_gateway = false) => ({ difficulty, exalt, exalt_gateway });
it('mirrors selected precedence, exclusions, exalt/gateway and threshold boundaries', () => {
  const p = preferences({ key_pop_min_difficulty: 7, key_pop_exalt_only: true });
  expect(keyPopAllows({ ...p, key_pop_dungeons: ['Synthetic'], key_pop_excluded_dungeons: ['Synthetic'] }, 'Synthetic', null)).toBe(true);
  expect(keyPopAllows({ ...p, key_pop_dungeons: [] }, 'Synthetic', meta(10, true))).toBe(false);
  expect(keyPopAllows({ ...p, key_pop_excluded_dungeons: ['Synthetic'] }, 'Synthetic', meta(10, true))).toBe(false);
  expect(keyPopAllows(p, 'Synthetic', meta(7, true))).toBe(true);
  expect(keyPopAllows(p, 'Synthetic', meta(7, false, true))).toBe(true);
  expect(keyPopAllows(p, 'Synthetic', meta(6.5, true))).toBe(false);
  expect(keyPopAllows(p, 'Synthetic', meta(10))).toBe(false);
  expect(keyPopAllows(p, 'Synthetic', null)).toBe(false);
  expect(keyPopAllows({ ...p, key_pop_exalt_only: false }, 'Synthetic', null)).toBe(true);
  expect(keyPopAllows({ ...p, key_pop_exalt_only: false }, 'Synthetic', meta(null))).toBe(true);
  expect(keyPopAllows(p, 'Synthetic', undefined)).toBe(true);
  expect(keyPopAllows({ ...p, key_pop_excluded_dungeons: ['Synthetic'] }, 'Synthetic', undefined)).toBe(false);
});
it('applies loaded metadata to delivery and mute counts; unloaded delivery fails open', () => {
  const p = preferences({ key_pop_min_difficulty: 7 });
  const pop = alert({ kind: 'key_popped', details: { type: 'key_popped', dungeon: 'Synthetic' } });
  expect(allowsAlert(p, pop)).toBe(true);
  const rows = [dungeonDef({ name: 'Synthetic', difficulty: 6 })];
  dungeonCatalog.set(rows);
  expect(allowsAlert(p, pop)).toBe(false);
  expect(mutedDungeons(p, ['Synthetic'], rows)).toEqual(new Set(['Synthetic']));
});
it('preserves lists in presets, produces half steps and previews unknown difficulty', async () => {
  dungeonCatalog.set([dungeonDef({ name: 'Low', difficulty: 6 }), dungeonDef({ name: 'High', difficulty: 8, exalt: true }), dungeonDef({ name: 'Unknown', difficulty: null })]);
  let p = preferences({ key_pop_excluded_dungeons: ['Elsewhere'] });
  const edits = vi.fn(patch => { p = { ...p, ...patch }; });
  const view = render(KeyPopRules, { preferences: p, onEdit: edits });
  await fireEvent.change(screen.getByLabelText('Preset'), { target: { value: 'exalt' } });
  expect(p.key_pop_exalt_only).toBe(true); expect(p.key_pop_min_difficulty).toBeNull();
  expect(p.key_pop_excluded_dungeons).toEqual(['Elsewhere']);
  await view.rerender({ preferences: p });
  expect(screen.getByText(/1 of 3 dungeons will notify/)).toBeTruthy();
  await fireEvent.change(screen.getByLabelText('Preset'), { target: { value: 'everything' } });
  expect(p.key_pop_exalt_only).toBe(false); expect(p.key_pop_min_difficulty).toBeNull();
  await fireEvent.change(screen.getByLabelText('Preset'), { target: { value: 'custom' } });
  await view.rerender({ preferences: p });
  expect(screen.getByText(/2 of 3 dungeons will notify/)).toBeTruthy();
  expect(screen.getByText(/1 with unknown difficulty/)).toBeTruthy();
  await fireEvent.input(screen.getByRole('slider'), { target: { value: '7.3' } });
  expect(p.key_pop_min_difficulty).toBe(7.5);
  await view.rerender({ preferences: { ...p, key_pop_dungeons: ['High'] } });
  expect(screen.getByLabelText('Preset').closest('fieldset')?.disabled).toBe(true);
  expect(screen.getByText(/selected list overrides/)).toBeTruthy();
});
it('keeps local rules through a concurrent mute and applies both new fields', async () => {
  const store = writable(preferences());
  const update = vi.fn(async value => { store.set(typeof value === 'function' ? value()! : value); });
  const session: AlertSession = { preferences: store, status: writable('ready'), update, refresh: async () => {}, dispose() {} };
  let state = emptyAlertPreferences();
  const editor = createAlertPreferences(session, value => { state = value; });
  editor.edit({ key_pop_min_difficulty: 7.5, key_pop_exalt_only: true });
  store.set(preferences({ key_pop_excluded_dungeons: ['Synthetic'] }));
  await editor.apply();
  expect(state.saved).toMatchObject({ key_pop_min_difficulty: 7.5, key_pop_exalt_only: true, key_pop_excluded_dungeons: ['Synthetic'] });
  expect(state.dirty).toBe(false); editor.dispose();
});
it('shows picker difficulty, exalt badges and rule-filtered rows', async () => {
  const { createClient } = await import('../../src/lib/bridge/client');
  const { createFixtureTransport } = await import('../../src/lib/bridge/fixture');
  const { homeSnapshot } = await import('./fixtures');
  const client = createClient(createFixtureTransport(homeSnapshot()));
  client.listCatalogDungeonOptions = vi.fn(async () => [dungeonDef({ name: 'Low', difficulty: 6, exalt_gateway: true }), dungeonDef({ name: 'High', difficulty: 8, exalt: true })]);
  render(DungeonPicker, { client, preferences: preferences({ key_pop_min_difficulty: 7, key_pop_excluded_dungeons: ['Other'] }), disabled: false, onEdit: vi.fn() });
  await flush();
  expect(screen.getByText('6 / 10')).toBeTruthy();
  expect(screen.getByText('Exalt gateway')).toBeTruthy();
  expect(screen.getByText('Exalt')).toBeTruthy();
  expect(screen.getByText('Filtered by rules')).toBeTruthy();
});

it('saves the panel fields through the settings editor', async () => {
  const { mountPhase2, main } = await import('./fixtures');
  const page = mountPhase2({ hash: '#/settings' });
  await flush(); await fireEvent.click(screen.getByRole('tab', { name: 'Notifications' }));
  await fireEvent.change(main().getByLabelText('Preset'), { target: { value: 'custom' } });
  await fireEvent.input(main().getByRole('slider', { name: 'Minimum difficulty rating' }), { target: { value: '7.5' } });
  await fireEvent.click(main().getByRole('checkbox', { name: 'Exalt dungeons only' }));
  await fireEvent.click(main().getByRole('button', { name: 'Save' })); await flush();
  expect(page.settings.alerts).toMatchObject({ key_pop_min_difficulty: 7.5, key_pop_exalt_only: true });
});

it('labels rule-filtered saved alerts instead of offering an ineffective Unmute', async () => {
  const { default: AlertTable } = await import('../../src/lib/alerts/AlertTable.svelte');
  render(AlertTable, { rows: [alert({ kind: 'key_popped', details: { type: 'key_popped', dungeon: 'Synthetic' } })],
    selectedId: null, onSelect: vi.fn(), muted: () => true, filtered: () => true, onMute: vi.fn(), canMute: true });
  expect(screen.getByText('Filtered by rules')).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Unmute Synthetic' })).toBeNull();
});

it('disables the banner mute for a rule-filtered dungeon and explains it', async () => {
  const { default: MuteDungeon } = await import('../../src/lib/alerts/MuteDungeon.svelte');
  const { DUNGEON_MUTE, createDungeonMute } = await import('../../src/lib/alerts/dungeon-mute');
  const session: AlertSession = { preferences: writable(preferences({ key_pop_exalt_only: true })), status: writable('ready'), update: vi.fn(), refresh: async () => {}, dispose() {} };
  const mute = createDungeonMute(session, () => true);
  dungeonCatalog.set([dungeonDef({ name: 'Synthetic', difficulty: 7, exalt: false })]);
  render(MuteDungeon, { props: { dungeon: 'Synthetic' }, context: new Map([[DUNGEON_MUTE, mute]]) });
  expect(screen.getByText('Filtered by rules')).toBeTruthy();
  expect((screen.getByRole('button', { name: 'Mute Synthetic' }) as HTMLButtonElement).disabled).toBe(true);
  mute.dispose();
});

it('shares catalog reads, invalidates on asset changes and drops retired metadata', async () => {
  const { loadDungeonCatalog, watchDungeonCatalog } = await import('../../src/lib/alerts/dungeon-catalog');
  const { createClient } = await import('../../src/lib/bridge/client');
  const { createFixtureTransport } = await import('../../src/lib/bridge/fixture');
  const { homeSnapshot } = await import('./fixtures');
  const { get } = await import('svelte/store');
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const rows = [dungeonDef({ name: 'Synthetic', difficulty: 7 })];
  client.listCatalogDungeonOptions = vi.fn(async () => rows);
  let changed = () => {};
  const unsubscribe = vi.fn();
  client.onVisualsChanged = vi.fn(callback => { changed = callback; return unsubscribe; });
  const releaseA = watchDungeonCatalog(client), releaseB = watchDungeonCatalog(client);
  await loadDungeonCatalog(client);
  expect(client.listCatalogDungeonOptions).toHaveBeenCalledTimes(1);
  expect(get(dungeonCatalog)).toEqual(rows);
  changed(); await loadDungeonCatalog(client);
  expect(client.listCatalogDungeonOptions).toHaveBeenCalledTimes(2);
  releaseA(); expect(unsubscribe).not.toHaveBeenCalled();
  releaseB(); expect(unsubscribe).toHaveBeenCalledOnce();
  expect(get(dungeonCatalog)).toBeUndefined();
});

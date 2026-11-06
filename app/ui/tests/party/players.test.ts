import { readFileSync } from 'node:fs';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Party from '../../src/routes/Party.svelte';
import PlayersView from '../../src/lib/party/PlayersView.svelte';
import type { SetupStatus } from '../../src/lib/bindings/SetupStatus';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { setNativePresentationVisible } from '../../src/lib/bridge/visibility';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { PartyPlayerRow } from '../../src/lib/bindings/PartyPlayerRow';
import type { PartyPlayerView } from '../../src/lib/bindings/PartyPlayerView';
import type { PartyPlayersPage } from '../../src/lib/bindings/PartyPlayersPage';
import { productionFeatures } from '../../src/lib/navigation/routes';
import { fakeHistory } from '../navigation/fake-history';
import { homeSnapshot } from '../home/fixtures';
import { deferred, flush, ready, run } from '../runs/fixtures';

const row = (name: string): PartyPlayerRow => ({ key: name.toLowerCase(), name, runs_together: 8,
  deaths: 1, left_early: 2, last_seen: '1000000', last_map: 'Synthetic Sanctum' });
function fixture() {
  const transport = createFixtureTransport(homeSnapshot());
  const client = createClient(transport);
  const querySetup = client.querySetup;
  vi.spyOn(client, 'querySetup').mockImplementation(async () => ({ ...await querySetup(), demo: false }));
  const players = vi.spyOn(client, 'getPartyPlayers').mockResolvedValue(ready({ rows: [row('SynthAlpha')], next: null }));
  const player = vi.spyOn(client, 'getPartyPlayer').mockResolvedValue(ready(null));
  const clear = vi.spyOn(client, 'clearPartyHistory').mockResolvedValue(undefined);
  return { client, transport, players, player, clear };
}
const table = () => within(screen.getByRole('table', { name: 'Party players' }));
beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); setNativePresentationVisible(true); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('reads setup once without settings, adapters, capture subscriptions or extraction polling', async () => {
  const page = fixture(), setup = await page.client.querySetup();
  const read = vi.mocked(page.client.querySetup).mockResolvedValue({ ...setup, assets: 'extracting' });
  read.mockClear();
  const settings = vi.spyOn(page.client, 'querySettings'), adapters = vi.spyOn(page.client, 'listAdapters');
  const capture = vi.spyOn(page.client, 'onCapture');
  render(PlayersView, { client: page.client }); await flush();
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(read).toHaveBeenCalledTimes(1);
  expect(settings).not.toHaveBeenCalled(); expect(adapters).not.toHaveBeenCalled();
  expect(capture).not.toHaveBeenCalled();
});

it.each(['success', 'failure'] as const)('keeps Players clear disabled while setup is pending, then handles %s', async outcome => {
  const page = fixture(), setup = await page.client.querySetup(), pending = deferred<SetupStatus>();
  vi.mocked(page.client.querySetup).mockReturnValue(pending.promise);
  render(PlayersView, { client: page.client }); await flush();
  const button = screen.getByRole('button', { name: 'Clear party history…' }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  button.click(); expect(screen.queryByRole('dialog')).toBeNull();
  if (outcome === 'success') pending.resolve(setup);
  else pending.reject(new Error('PRIVATE'));
  await flush();
  expect(button.disabled).toBe(outcome === 'failure');
  if (outcome === 'failure') {
    const hint = screen.getByText('Could not check Demo Mode. Reload to try again.');
    expect(hint.hidden).toBe(false); expect(button.getAttribute('aria-describedby')).toBe(hint.id);
    expect(document.body.textContent).not.toContain('PRIVATE');
  }
});

it.each(['success', 'failure'] as const)('ignores an obsolete setup %s after the client changes', async outcome => {
  const page = fixture(), replacement = fixture(), pending = deferred<SetupStatus>();
  const setup = await page.client.querySetup();
  vi.mocked(page.client.querySetup).mockReturnValue(pending.promise);
  const view = render(PlayersView, { client: page.client }); await flush();
  await view.rerender({ client: replacement.client }); await flush();
  if (outcome === 'success') pending.resolve({ ...setup, demo: true });
  else pending.reject(new Error('PRIVATE'));
  await flush();
  expect((screen.getByRole('button', { name: 'Clear party history…' }) as HTMLButtonElement).disabled).toBe(false);
  expect(screen.queryByText('Could not check Demo Mode. Reload to try again.')).toBeNull();
});

it('does not restore a dropped row after clearing history and marking the player again', async () => {
  const page = fixture(), player = { ...row('SynthAlpha'), mark: 'watched' as const };
  page.players.mockResolvedValue(ready({ rows: [player], next: null }));
  page.player.mockResolvedValue(ready({ row: player, first_seen: '1000000', runs: [] }));
  vi.spyOn(page.client, 'setPartyMark').mockResolvedValue(undefined);
  render(PlayersView, { client: page.client, playerKey: player.key }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Watched' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Watch' })); await flush();
  expect(table().queryByText(player.name)).toBeNull();
  page.players.mockResolvedValue(ready({ rows: [], next: null }));
  page.player.mockResolvedValue(ready({ row: { ...row('SynthAlpha'), runs_together: 0, deaths: 0,
    left_early: 0, last_map: null, note: 'Retained synthetic note' }, first_seen: '1000000', runs: [] }));
  await fireEvent.click(screen.getByRole('button', { name: 'Clear party history…' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm' })); await flush();
  const pending = deferred<ReturnType<typeof ready<PartyPlayersPage>>>();
  page.players.mockReturnValue(pending.promise);
  await fireEvent.click(screen.getByRole('button', { name: 'Watch' })); await flush();
  expect(table().queryByText(player.name)).toBeNull();
  expect(table().queryByText('8')).toBeNull();
  pending.resolve(ready({ rows: [], next: null })); await flush();
  expect(table().queryByText(player.name)).toBeNull();
});

it('discards a dropped row once page 1 supplies a fresh copy', async () => {
  const page = fixture(), player = { ...row('SynthAlpha'), mark: 'watched' as const };
  page.players.mockResolvedValue(ready({ rows: [player], next: null }));
  page.player.mockResolvedValue(ready({ row: player, first_seen: '1000000', runs: [] }));
  page.client.queryStore = async () => ({ state: 'ready', queued: 0, durable: '3', stream_id: 'synth-store', message: '' });
  vi.spyOn(page.client, 'setPartyMark').mockResolvedValue(undefined);
  render(PlayersView, { client: page.client, playerKey: player.key }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Watched' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Watch' })); await flush();
  expect(table().queryByText(player.name)).toBeNull();
  const refresh = async () => {
    page.transport.publishEngineEvent({ stream_id: 'synth-store', sequence: '3', at: '1', event: { run_ended: run({ id: 'synth-run' }) } });
    await flush();
  };
  page.players.mockResolvedValue(ready({ rows: [{ ...player, runs_together: 9 }], next: null }));
  await refresh();
  expect(table().getByText('9')).toBeTruthy();
  page.players.mockResolvedValue(ready({ rows: [], next: null }));
  await refresh();
  const pending = deferred<ReturnType<typeof ready<PartyPlayersPage>>>();
  page.players.mockReturnValue(pending.promise);
  await fireEvent.click(screen.getByRole('button', { name: 'Watch' })); await flush();
  expect(table().queryByText(player.name)).toBeNull();
  pending.resolve(ready({ rows: [], next: null })); await flush();
});

it('keeps the Players table in a bounded horizontal scroller with the side panel open', async () => {
  const page = fixture();
  const view = render(Party, { client: page.client, route: { kind: 'party-player', key: 'synthalpha' } }); await flush();
  // Vitest omits component CSS. Apply the actual stylesheet for this DOM check.
  // jsdom cannot verify geometry; horizontal overflow is checked manually in a browser.
  const style = document.createElement('style');
  style.textContent = readFileSync('src/lib/party/PlayersView.svelte', 'utf8').match(/<style>([\s\S]*?)<\/style>/)![1];
  view.container.append(style);
  expect(screen.getByRole('complementary', { name: 'Player details' })).toBeTruthy();
  const wrapper = screen.getByRole('table', { name: 'Party players' }).parentElement!;
  expect(getComputedStyle(wrapper).overflowX).toBe('auto');
  expect(getComputedStyle(wrapper).maxWidth).toBe('100%');
  expect(getComputedStyle(wrapper.parentElement!).minWidth).toBe('0');
  const layout = wrapper.closest('.layout')!;
  expect(getComputedStyle(layout).gridTemplateColumns).toBe('minmax(0, 1fr) 290px');
  expect(getComputedStyle(layout).minWidth).toBe('0');
  expect(getComputedStyle(screen.getByRole('region', { name: 'Party players' })).minWidth).toBe('0');
  layout.classList.remove('with-panel');
  expect(getComputedStyle(layout).gridTemplateColumns).toBe('minmax(0, 1fr)');
});

it.each(([
  ['Watched', 'watched', 'Watch'], ['Banned', 'banned', 'Ban'], ['With notes', null, 'note'],
] as const).flatMap(([filter, mark, action]) => [1, 2].map(pages => ({ filter, mark, action, pages }))))('restores a dropped $filter row with $pages pages loaded', async ({ filter, mark, action, pages }) => {
  const page = fixture();
  const player = { ...row('SynthAlpha'), mark, note: 'Synthetic note' };
  const first = { ...row('SynthFirst'), mark, note: 'Synthetic note' };
  const cursor = { sort_value: '0', key: first.key };
  page.players.mockImplementation(async (_query, _filter, _sort, after) => ready(pages === 1
    ? { rows: [player], next: null }
    : after ? { rows: [player], next: null } : { rows: [first], next: cursor }));
  page.player.mockResolvedValue(ready({ row: player, first_seen: '1000000', runs: [] }));
  const save = vi.spyOn(page.client, 'setPartyMark').mockResolvedValue(undefined);
  render(Party, { client: page.client, route: { kind: 'party-player', key: player.key } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: filter })); await flush();
  if (pages === 2) { await fireEvent.click(screen.getByRole('button', { name: 'Load more' })); await flush(); }
  expect(table().getByText('SynthAlpha')).toBeTruthy();
  const panel = screen.getByRole('complementary', { name: 'Player details' });
  async function change(note: string) {
    if (action === 'note') {
      const field = within(panel).getByRole('textbox', { name: 'Private note' });
      await fireEvent.input(field, { target: { value: note } }); await fireEvent.blur(field);
    } else await fireEvent.click(within(panel).getByRole('button', { name: action }));
    await flush();
  }
  await change('');
  expect(table().queryByText('SynthAlpha')).toBeNull();
  const reads = page.players.mock.calls.length;
  await change('Synthetic note');
  expect(save).toHaveBeenCalledTimes(2);
  expect(page.players).toHaveBeenCalledTimes(reads + 1);
  expect(page.players).toHaveBeenLastCalledWith('', filter === 'With notes' ? 'with_notes' : mark, 'last_seen', null);
  expect(table().getAllByText('SynthAlpha')).toHaveLength(1);
  expect(screen.getByRole('complementary', { name: 'Player details' })).toBe(panel);
});

it.each([
  ['Watched', 'watched', 'Ban'], ['Watched', 'watched', 'Watch'],
  ['Banned', 'banned', 'Watch'], ['Banned', 'banned', 'Ban'],
  ['With notes', null, 'clear-note'], ['All', 'watched', 'Ban'],
] as const)('rechecks %s after a %s row is edited via %s', async (filter, mark, action) => {
  const page = fixture();
  const player = { ...row('SynthAlpha'), mark, note: 'Synthetic note' };
  page.players.mockResolvedValue(ready({ rows: [player], next: null }));
  page.player.mockResolvedValue(ready({ row: player, first_seen: '1000000', runs: [] }));
  const save = vi.spyOn(page.client, 'setPartyMark').mockResolvedValue(undefined);
  render(Party, { client: page.client, route: { kind: 'party-player', key: player.key } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: filter })); await flush();
  expect(table().getByText('SynthAlpha')).toBeTruthy();
  if (action === 'clear-note') {
    const note = screen.getByRole('textbox', { name: 'Private note' });
    await fireEvent.input(note, { target: { value: '' } });
    await fireEvent.blur(note);
  } else await fireEvent.click(screen.getByRole('button', { name: action }));
  await flush();
  expect(save).toHaveBeenCalledTimes(1);
  expect(table().queryByText('SynthAlpha') !== null).toBe(filter === 'All');
  if (filter === 'All') expect(table().getByText('Banned')).toBeTruthy();
});

it('debounces search for 250 ms and appends unique players with the opaque cursor unchanged', async () => {
  const page = fixture(), cursor = { sort_value: '4598175219545276417', key: 'synthalpha' };
  page.players.mockResolvedValueOnce(ready({ rows: [row('SynthAlpha')], next: cursor }));
  render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  expect(page.players).toHaveBeenLastCalledWith('', 'all', 'last_seen', null);
  page.players.mockResolvedValueOnce(ready({ rows: [row('SynthAlpha'), row('SynthBravo')], next: null }));
  await fireEvent.click(screen.getByRole('button', { name: 'Load more' })); await flush();
  expect(page.players).toHaveBeenLastCalledWith('', 'all', 'last_seen', cursor);
  expect(table().getAllByRole('row')).toHaveLength(3);
  expect(table().getAllByText('SynthAlpha')).toHaveLength(1);
  await fireEvent.input(screen.getByRole('searchbox', { name: 'Find a player' }), { target: { value: 'Synth' } });
  await vi.advanceTimersByTimeAsync(249); expect(page.players).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1); await flush();
  expect(page.players).toHaveBeenLastCalledWith('Synth', 'all', 'last_seen', null);
  expect(table().queryByText('SynthBravo')).toBeNull();
});

it('passes each filter and sortable column to the server and resets paging', async () => {
  const page = fixture(); render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  for (const [label, filter] of [['Watched', 'watched'], ['Banned', 'banned'], ['With notes', 'with_notes'], ['All', 'all']]) {
    await fireEvent.click(screen.getByRole('button', { name: label })); await flush();
    expect(page.players).toHaveBeenLastCalledWith('', filter, 'last_seen', null);
  }
  for (const [label, sort] of [['Runs together', 'runs_together'], ['Deaths', 'deaths'], ['Left early', 'left_early'], ['Avg share', 'avg_share'], ['Last seen', 'last_seen']]) {
    await fireEvent.click(table().getByRole('button', { name: label })); await flush();
    expect(page.players).toHaveBeenLastCalledWith('', 'all', sort, null);
    expect(table().getByRole('button', { name: label }).closest('th')?.getAttribute('aria-sort')).toBe('descending');
  }
});

it.each([false, true])('clears history with marks_and_notes=%s and refreshes', async checked => {
  const page = fixture(); render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear party history…' }));
  const dialog = within(screen.getByRole('dialog', { name: 'Clear party history' }));
  expect(dialog.getByText("Keeping marks and notes also keeps each marked or noted player's name and first and last seen times. Their last profile, scope and map are removed.")).toBeTruthy();
  const checkbox = dialog.getByRole('checkbox', { name: 'Also clear marks and notes' }) as HTMLInputElement;
  expect(checkbox.checked).toBe(false);
  if (checked) await fireEvent.click(checkbox);
  expect(dialog.queryByText(/Keeping marks and notes/) !== null).toBe(!checked);
  await fireEvent.click(dialog.getByRole('button', { name: 'Confirm' })); await flush();
  expect(page.clear).toHaveBeenCalledWith({ marks_and_notes: checked });
  expect(page.players).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('cancels clear without writing and retains the dialog after a failed clear', async () => {
  const page = fixture(); render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear party history…' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' })); expect(page.clear).not.toHaveBeenCalled();
  page.clear.mockRejectedValueOnce(new Error('synthetic failure'));
  await fireEvent.click(screen.getByRole('button', { name: 'Clear party history…' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm' })); await flush();
  expect(screen.getByRole('dialog')).toBeTruthy(); expect(screen.getByRole('alert')).toBeTruthy();
  expect(page.players).toHaveBeenCalledTimes(1);
});

it('ignores an obsolete page after the filter changes', async () => {
  const page = fixture(), pending = deferred<ReturnType<typeof ready<PartyPlayersPage>>>();
  page.players.mockResolvedValueOnce(ready({ rows: [row('SynthAlpha')], next: { sort_value: '0', key: 'synthalpha' } }))
    .mockImplementationOnce(() => pending.promise).mockResolvedValue(ready({ rows: [row('SynthWatched')], next: null }));
  render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Watched' })); await flush();
  pending.resolve(ready({ rows: [row('SynthOld')], next: null })); await flush();
  expect(table().getByText('SynthWatched')).toBeTruthy(); expect(table().queryByText('SynthOld')).toBeNull();
});

it('navigates a player row through Shell, pages all runs uniquely and links to Party runs', async () => {
  const page = fixture(), browser = fakeHistory('#/party/players');
  const runs = Array.from({ length: 20 }, (_, index) => ({ run_id: `synth-${index}`, dungeon: `Synthetic map ${index}`,
    started: '1000000', died: false, left_early: false }));
  const cursor = { started: '1000000', run_id: 'synth-19' };
  const detail: PartyPlayerView = { row: row('SynthAlpha'), first_seen: '1000000', runs, next: cursor };
  page.player.mockResolvedValueOnce(ready(detail)).mockResolvedValueOnce(ready({ ...detail, runs: [runs[19],
    { ...runs[0], run_id: 'synth-20', dungeon: 'Synthetic final map' }], next: null }));
  render(Shell, { client: page.client, preview: true, features: { ...productionFeatures, party: true }, historyAdapter: browser.adapter }); await flush();
  await fireEvent.click(table().getByText('SynthAlpha')); await flush();
  expect(browser.adapter.hash).toBe('#/party/players/synthalpha');
  const panel = within(screen.getByRole('complementary', { name: 'Player details' }));
  const history = within(panel.getByRole('region', { name: 'Runs together' }));
  expect(history.getAllByRole('link')).toHaveLength(20);
  await fireEvent.click(history.getByRole('button', { name: 'Load more' })); await flush();
  expect(page.player).toHaveBeenLastCalledWith('synthalpha', cursor);
  expect(history.getAllByRole('link')).toHaveLength(21);
  expect(history.getByRole('link', { name: 'Synthetic final map' }).getAttribute('href')).toBe('#/party/runs/synth-20');
  await fireEvent.click(panel.getByRole('button', { name: 'Close player details' })); await flush();
  expect(browser.adapter.hash).toBe('#/party/players');
});

it('opens a direct player route even when the player is not on the first list page', async () => {
  const page = fixture(); page.player.mockResolvedValue(ready({ row: row('SynthBravo'), first_seen: '1000000', runs: [] }));
  render(Party, { client: page.client, route: { kind: 'party-player', key: 'synthbravo' } }); await flush();
  expect(within(screen.getByRole('complementary')).getByRole('heading', { name: 'SynthBravo' })).toBeTruthy();
});

it.each([false, true])('refreshes Players and open player history after run-end durability (hidden=%s)', async hidden => {
  const page = fixture(); let durable = '1';
  page.client.queryStore = async () => ({ state: 'ready', queued: 0, durable, stream_id: 'synth-store', message: '' });
  render(Party, { client: page.client, route: { kind: 'party-player', key: 'synthalpha' } }); await flush();
  if (hidden) setNativePresentationVisible(false);
  page.transport.publishEngineEvent({ stream_id: 'synth-store', sequence: '3', at: '1', event: { run_ended: run({ id: 'synth-run' }) } });
  await vi.advanceTimersByTimeAsync(1000); await flush(); expect(page.players).toHaveBeenCalledTimes(1);
  if (hidden) { setNativePresentationVisible(true); await flush(); expect(page.players).toHaveBeenCalledTimes(1); }
  durable = '3'; await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(page.players).toHaveBeenCalledTimes(2); expect(page.player).toHaveBeenCalledTimes(2);
});

it('refreshes marks after editing and discards erased marks and notes in the open panel after clear', async () => {
  const page = fixture(), detail: PartyPlayerView = { row: { ...row('SynthAlpha'), mark: 'watched', note: 'Synthetic saved note' }, first_seen: '1000000', runs: [] };
  const mark = vi.spyOn(page.client, 'setPartyMark').mockResolvedValue(undefined);
  page.player.mockResolvedValueOnce(ready(detail)).mockResolvedValueOnce(ready({ ...detail, row: row('SynthAlpha') }));
  render(Party, { client: page.client, route: { kind: 'party-player', key: 'synthalpha' } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Ban' })); await flush();
  expect(mark).toHaveBeenCalledWith('synthalpha', 'SynthAlpha', 'banned', 'Synthetic saved note');
  expect(page.players).toHaveBeenCalledTimes(1);
  expect(table().getByText('Banned')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear party history…' }));
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Also clear marks and notes' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm' })); await flush();
  expect((screen.getByRole('textbox', { name: 'Private note' }) as HTMLTextAreaElement).value).toBe('');
  expect(screen.getByRole('button', { name: 'Watch' }).getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByRole('button', { name: 'Ban' }).getAttribute('aria-pressed')).toBe('false');
});

it('ignores an old run page after navigating to a different player', async () => {
  const page = fixture(), pending = deferred<ReturnType<typeof ready<PartyPlayerView | null>>>();
  const detail: PartyPlayerView = { row: row('SynthAlpha'), first_seen: '1000000', runs: [], next: { started: '1000000', run_id: 'synth-first' } };
  page.player.mockResolvedValueOnce(ready(detail)).mockImplementationOnce(() => pending.promise)
    .mockResolvedValueOnce(ready({ row: row('SynthBravo'), first_seen: '2000000', runs: [] }));
  const component = render(Party, { client: page.client, route: { kind: 'party-player', key: 'synthalpha' } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Load more' }));
  await component.rerender({ client: page.client, route: { kind: 'party-player', key: 'synthbravo' } }); await flush();
  pending.resolve(ready({ ...detail, runs: [{ run_id: 'synth-old', dungeon: 'Synthetic old map', started: '1000000', died: false, left_early: false }] })); await flush();
  expect(screen.getByRole('heading', { name: 'SynthBravo' })).toBeTruthy(); expect(screen.queryByText('Synthetic old map')).toBeNull();
});

it('retries a failed detail refresh as a read and preserves unsaved notes', async () => {
  const page = fixture(), detail: PartyPlayerView = { row: row('SynthAlpha'), first_seen: '1000000', runs: [] };
  const mark = vi.spyOn(page.client, 'setPartyMark').mockResolvedValue(undefined);
  page.player.mockResolvedValueOnce(ready(detail)).mockRejectedValueOnce(new Error('synthetic read failure')).mockResolvedValue(ready(detail));
  page.client.queryStore = async () => ({ state: 'ready', queued: 0, durable: '3', stream_id: 'synth-store', message: '' });
  render(Party, { client: page.client, route: { kind: 'party-player', key: 'synthalpha' } }); await flush();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Private note' }), { target: { value: 'Synthetic unsaved draft' } });
  page.transport.publishEngineEvent({ stream_id: 'synth-store', sequence: '3', at: '1', event: { run_ended: run({ id: 'synth-run' }) } }); await flush();
  await fireEvent.click(within(screen.getByRole('complementary')).getByRole('button', { name: 'Retry' })); await flush();
  expect(page.player).toHaveBeenCalledTimes(3); expect(mark).not.toHaveBeenCalled();
  expect((screen.getByRole('textbox', { name: 'Private note' }) as HTMLTextAreaElement).value).toBe('Synthetic unsaved draft');
});


it('ignores a slower earlier search response after the newer search finishes', async () => {
  const page = fixture(), pending = deferred<ReturnType<typeof ready<PartyPlayersPage>>>();
  render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  page.players.mockImplementationOnce(() => pending.promise).mockResolvedValueOnce(ready({ rows: [row('SynthBravo')], next: null }));
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'Alpha' } });
  await vi.advanceTimersByTimeAsync(250);
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'Bravo' } });
  await vi.advanceTimersByTimeAsync(250); await flush();
  pending.resolve(ready({ rows: [row('SynthAlpha')], next: null })); await flush();
  expect(table().getByText('SynthBravo')).toBeTruthy(); expect(table().queryByText('SynthAlpha')).toBeNull();
  expect(screen.getByRole('group', { name: 'Player filters' })).toBeTruthy();
});

it('syncs clear state on Escape and native close, restores focus and shows late errors', async () => {
  const page = fixture(); render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  const trigger = screen.getByRole('button', { name: 'Clear party history…' }); trigger.focus();
  await fireEvent.click(trigger);
  await fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(screen.queryByRole('dialog')).toBeNull(); expect(document.activeElement).toBe(trigger);
  const pending = deferred<void>(); page.clear.mockImplementationOnce(() => pending.promise);
  await fireEvent.click(trigger); await fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  await fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(screen.getByRole('dialog')).toBeTruthy();
  await fireEvent(screen.getByRole('dialog'), new Event('close'));
  expect(screen.queryByRole('dialog')).toBeNull(); expect(document.activeElement).toBe(trigger);
  pending.reject(new Error('synthetic failure')); await flush();
  expect(screen.getByRole('alert').textContent).toBe('Could not clear party history. Retry to clear it.');
  await fireEvent.click(trigger); expect(screen.getByRole('dialog')).toBeTruthy();
});

it('keeps loaded player and run pages through mark edits and delayed refreshes', async () => {
  const page = fixture(), cursor = { sort_value: '0', key: 'synthalpha' };
  const runA = { run_id: 'synth-a', dungeon: 'Synthetic First', started: '1000000', died: false, left_early: false };
  const runB = { ...runA, run_id: 'synth-b', dungeon: 'Synthetic Second' };
  const detail: PartyPlayerView = { row: row('SynthBravo'), first_seen: '1000000', runs: [runA], next: { started: '1000000', run_id: 'synth-a' } };
  page.players.mockResolvedValueOnce(ready({ rows: [row('SynthAlpha')], next: cursor }))
    .mockResolvedValueOnce(ready({ rows: [row('SynthBravo')], next: null }));
  page.player.mockResolvedValueOnce(ready(detail)).mockResolvedValueOnce(ready({ ...detail, runs: [runB], next: null }));
  vi.spyOn(page.client, 'setPartyMark').mockResolvedValue(undefined);
  page.client.queryStore = async () => ({ state: 'ready', queued: 0, durable: '3', stream_id: 'synth-store', message: '' });
  render(Party, { client: page.client, route: { kind: 'party-player', key: 'synthbravo' } }); await flush();
  const history = within(screen.getByRole('region', { name: 'Runs together' }));
  await fireEvent.click(history.getByRole('button', { name: 'Load more' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Load more' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Watch' })); await flush();
  expect(table().getByText('SynthBravo')).toBeTruthy(); expect(table().getByText('Watched')).toBeTruthy();
  expect(page.players).toHaveBeenCalledTimes(2);
  const pendingPlayers = deferred<ReturnType<typeof ready<PartyPlayersPage>>>();
  const pendingDetail = deferred<ReturnType<typeof ready<PartyPlayerView | null>>>();
  page.players.mockImplementationOnce(() => pendingPlayers.promise); page.player.mockImplementationOnce(() => pendingDetail.promise);
  page.transport.publishEngineEvent({ stream_id: 'synth-store', sequence: '3', at: '1', event: { run_ended: run({ id: 'synth-run' }) } }); await flush();
  expect(table().getByText('SynthBravo')).toBeTruthy(); expect(history.getAllByRole('link')).toHaveLength(2);
  pendingPlayers.resolve(ready({ rows: [{ ...row('SynthAlpha'), runs_together: 9 }], next: cursor }));
  pendingDetail.resolve(ready({ ...detail, runs: [{ ...runA, dungeon: 'Synthetic Refreshed' }] })); await flush();
  expect(table().getByText('SynthBravo')).toBeTruthy(); expect(table().getByText('9')).toBeTruthy();
  expect(history.getByRole('link', { name: 'Synthetic Refreshed' })).toBeTruthy();
  expect(history.getByRole('link', { name: 'Synthetic Second' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Load more' })).toBeNull();
});

it('refreshes after a clear succeeds even when its native dialog already closed', async () => {
  const page = fixture(), pending = deferred<void>(); page.clear.mockImplementationOnce(() => pending.promise);
  render(Party, { client: page.client, route: { kind: 'party-players' } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear party history…' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
  await fireEvent(screen.getByRole('dialog'), new Event('close'));
  pending.resolve(); await flush();
  expect(screen.queryByRole('dialog')).toBeNull(); expect(screen.queryByRole('alert')).toBeNull();
  expect(page.players).toHaveBeenCalledTimes(2);
});


it.each([[true, false], [false, true], [false, false]])('gates Players clear with demo=%s and preview=%s', async (demo, preview) => {
  const page = fixture();
  const setup = await page.client.querySetup();
  vi.mocked(page.client.querySetup).mockResolvedValue({ ...setup, demo });
  if (preview) render(Shell, { client: page.client, preview, features: { ...productionFeatures, party: true },
    historyAdapter: fakeHistory('#/party/players').adapter });
  else render(Party, { client: page.client, route: { kind: 'party-players' } });
  await flush();
  const button = screen.getByRole('button', { name: 'Clear party history…' }) as HTMLButtonElement;
  expect(button.disabled).toBe(demo || preview);
  const hint = screen.queryByText(preview ? 'Clearing is not available in preview.' : 'Turn off Demo Mode to clear your party history.');
  expect(hint !== null).toBe(demo || preview);
  expect(button.getAttribute('aria-describedby')).toBe(hint?.id ?? null);
  if (hint) expect(hint.hidden).toBe(false);
  if (button.disabled) {
    button.click(); await flush();
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(page.clear).not.toHaveBeenCalled();
  }
});

import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import PartyLive from '../../src/lib/party/PartyLive.svelte';
import RunStory from '../../src/routes/RunStory.svelte';
import Shell from '../../src/lib/shell/Shell.svelte';
import { fakeHistory } from '../navigation/fake-history';
import type { PartyMember } from '../../src/lib/bindings/PartyMember';
import type { PartyView } from '../../src/lib/bindings/PartyView';
import type { PartyPlayerView } from '../../src/lib/bindings/PartyPlayerView';
import type { VisualPage } from '../../src/lib/bindings/VisualPage';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport, type PartyFixtureQueries } from '../../src/lib/bridge/fixture';
import { productionFeatures } from '../../src/lib/navigation/routes';
import { homeSnapshot } from '../home/fixtures';
import { deferred, flush, ready, run, story } from '../runs/fixtures';

const member = (name: string, result: 'pass' | 'fail' | 'check' | 'not_seen', patch: Partial<PartyMember> = {}): PartyMember => ({
  player_id: name, name, name_key: name.toLowerCase(), parse: { result, reasons: [] }, present: true,
  equipment: [], is_local: false, runs_together: 0, flags: [], ...patch,
});
function view(patch: Partial<PartyView> = {}): PartyView {
  return { scope_id: 'synth-scope', run_id: 'synth-run', map: 'Synthetic Sanctum', area_kind: 'dungeon', live: true,
    preset: { id: 'synth', name: 'Synthetic', source: 'default' }, assets_available: true, gear_recorded: true,
    classes: [], counts: { in_view: 4, pass: 1, fail: 1, check: 1, not_seen: 1, no_damage_seen: 1, left_early: 1 },
    members: [member('SynthAlpha', 'pass', { flags: ['no_damage_seen'] }),
      member('SynthBravo', 'fail', { maxed: 5, parse: { result: 'fail', reasons: [
        { kind: 'below_maxed', text: '5/8 < 6/8' }, { slot: 'weapon', kind: 'below_tier', text: 'weapon T9 < T11' }] },
        equipment: [{ slot: 0, item_type: 100, enchant: { state: 'missing', slots: null, applied: null, ids: [] } }] }),
      member('SynthCheck', 'check', { parse: { result: 'check', reasons: [{ slot: 'ring', kind: 'no_tier_data', text: 'ring: no tier data' }] } }),
      member('SynthSeen', 'not_seen')], ...patch };
}
function fixture(data = view(), overrides: PartyFixtureQueries = {}) {
  const queries = { getPartyLive: vi.fn(async () => ready(data)), getPartyRun: vi.fn(async () => ready(data)),
    getPartyPlayer: vi.fn(async (): Promise<ReturnType<typeof ready<PartyPlayerView | null>>> => ready(null)),
    setPartyMark: vi.fn(async () => {}), setPartyPresetPick: vi.fn(async () => {}), openRealmeye: vi.fn(async () => {}), ...overrides };
  const transport = createFixtureTransport(homeSnapshot(), { client_fingerprint: 'synth', objects: [
    { object_type: 100, name: 'Synthetic weapon', sprite: null, bag: null, item_tier: '9' }] }, undefined,
    undefined, undefined, undefined, undefined, {}, undefined, undefined, undefined, queries);
  const client = createClient(transport);
  const settings = client.querySettings;
  client.querySettings = async () => ({ ...await settings(), party: { presets: [{ id: 'synth', name: 'Synthetic', min_maxed: 6,
    min_tier: { weapon: 11 }, ut_st_meets: true, auto_apply: [] }] } });
  return { transport, client, queries };
}
async function mount(data = view(), saved = false, overrides: PartyFixtureQueries = {}) {
  const page = fixture(data, overrides);
  const component = render(PartyLive, { client: page.client, runId: saved ? 'synth-run' : undefined });
  await flush();
  return { ...page, ...component };
}
const roster = () => within(screen.getByRole('table', { name: 'Party roster' }));
const originalClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks();
  if (originalClipboard) Object.defineProperty(navigator, 'clipboard', originalClipboard);
  else Reflect.deleteProperty(navigator, 'clipboard');
});

it.each(['auto', 'picked', 'default'] as const)('lets a %s preset be pinned or reset without duplicate option labels', async source => {
  const page = await mount(view({ preset: { id: 'synth', name: 'Synthetic', source } }));
  const select = screen.getByRole('combobox', { name: 'Preset' }) as HTMLSelectElement;
  expect([...select.options].map(option => option.value)).toEqual(['', 'synth']);
  expect(select.value).toBe(source === 'picked' ? 'synth' : '');
  const labels = [...select.options].map(option => option.textContent);
  expect(new Set(labels).size).toBe(labels.length);
  expect(labels.filter(label => label?.startsWith('Preset: Synthetic'))).toHaveLength(1);
  expect(select.selectedOptions[0].textContent).toContain('Preset: Synthetic');
  expect(screen.getByText(source, { selector: '.caption' })).toBeTruthy();
  await fireEvent.change(select, { target: { value: source === 'picked' ? '' : 'synth' } });
  expect(page.queries.setPartyPresetPick).toHaveBeenCalledWith(source === 'picked' ? null : 'synth');
});

it('shows Preset: None when no preset resolves', async () => {
  await mount(view({ preset: null }));
  const select = screen.getByRole('combobox', { name: 'Preset' }) as HTMLSelectElement;
  expect(select.value).toBe('');
  expect(select.selectedOptions[0].textContent).toBe('Preset: None');
});

it('separates the roster hint sentences with a space', async () => {
  await mount();
  expect(screen.getByText(/Sorted by parse, then damage/).textContent)
    .toBe("Sorted by parse, then damage. Click a row for details. A double-click opens this run's player view.");
});

it('shows all parse pills, native reasons and shared gear hover with a failing tier', async () => {
  await mount();
  for (const label of ['Pass', 'Fail', 'Check', 'Not seen']) expect(roster().getByText(label)).toBeTruthy();
  expect(roster().getByText('5/8 < 6/8 · weapon T9 < T11')).toBeTruthy();
  expect(roster().getByText('ring: no tier data')).toBeTruthy();
  const tier = roster().getByText('T9');
  expect(tier.classList.contains('failing')).toBe(true);
  expect(tier.closest('[data-item-hover]')?.getAttribute('data-item-hover')).toBe('100');
});
it.each(['nexus', 'realm'] as const)('hides flags and shares in %s', async area_kind => {
  await mount(view({ area_kind }));
  expect(screen.queryByText('No damage seen')).toBeNull();
  expect(screen.queryByRole('columnheader', { name: 'Dmg share' })).toBeNull();
  expect(screen.queryByRole('button', { name: /^Flagged/ })).toBeNull();
});
it.each(['dungeon', 'nexus', 'realm'] as const)('keeps failure reasons alongside dungeon-only flags in %s', async area_kind => {
  const data = view({ area_kind }); data.members[1].flags = ['mostly_trash'];
  await mount(data);
  const notes = roster().getByText('5/8 < 6/8 · weapon T9 < T11').closest('td')!;
  expect(within(notes).queryByText('Mostly trash') !== null).toBe(area_kind === 'dungeon');
});
it('hides Parse and all parse counts without a preset', async () => {
  await mount(view({ preset: null }));
  expect(screen.queryByRole('columnheader', { name: 'Parse' })).toBeNull();
  const summary = within(screen.getByRole('region', { name: 'Party summary' }));
  for (const label of ['Pass', 'Fail', 'Check', 'Not seen']) expect(summary.queryByText(label)).toBeNull();
});
it('explains missing assets and hides counts', async () => {
  await mount(view({ assets_available: false }));
  expect(screen.getByText('Unavailable, game assets not extracted')).toBeTruthy();
  expect(within(screen.getByRole('region', { name: 'Party summary' })).queryByText('Pass')).toBeNull();
});
it('keeps legacy gear and saved-run departures explicit without live presence', async () => {
  await mount(view({ live: false, gear_recorded: false, members: [member('SynthAlpha', 'not_seen', { present: false, out_of_view_since: '1000000' })] }), true);
  expect(screen.getByText('Gear not recorded for this run')).toBeTruthy();
  expect(within(screen.getByRole('region', { name: 'Party summary' })).getByText('Left early')).toBeTruthy();
  expect(screen.queryByText(/^Out of view/)).toBeNull();
  expect(screen.queryByText('Live', { exact: true })).toBeNull();
});
it('omits the Live Left early count and shows departure and out-of-view row labels', async () => {
  await mount(view({ members: [member('SynthAlpha', 'pass', { present: false, out_of_view_since: '1000000' }),
    member('SynthBravo', 'pass', { departed_at: '1000000' })] }));
  expect(within(screen.getByRole('region', { name: 'Party summary' })).queryByText('Left early')).toBeNull();
  expect(screen.getByText(/^Out of view since \d{2}:\d{2}$/).closest('tr')?.classList.contains('out-of-view')).toBe(true);
  expect(screen.getByText('Left', { exact: true })).toBeTruthy();
});
it('shows a nameless member without player actions or history reads', async () => {
  const page = await mount(view({ members: [member('SynthNone', 'not_seen', { name: null, name_key: null })] }));
  await fireEvent.click(screen.getByText('No name yet'));
  await flush();
  expect(screen.queryByRole('button', { name: 'Watch' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Copy name' })).toBeNull();
  expect(screen.queryByRole('link', { name: 'Run view' })).toBeNull();
  expect(page.queries.getPartyPlayer).not.toHaveBeenCalled();
});
it('filters and copies failing names with confirmation', async () => {
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  await mount();
  await fireEvent.click(screen.getByRole('button', { name: 'Failing 1' }));
  expect(roster().queryByText('SynthAlpha')).toBeNull();
  expect(roster().getByText('SynthBravo')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Copy failing names' }));
  await flush();
  expect(writeText).toHaveBeenCalledWith('SynthBravo');
  expect(screen.getByText('Copied')).toBeTruthy();
});
it('keeps a trailing refresh: five notices within one second cause two reads', async () => {
  const page = await mount();
  await vi.advanceTimersByTimeAsync(1000);
  vi.mocked(page.queries.getPartyLive).mockClear();
  for (let i = 0; i < 5; i++) {
    page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
    await vi.advanceTimersByTimeAsync(200);
  }
  await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
  page.unmount();
  page.transport.publishPartyChanged({ scope_id: null });
  await vi.advanceTimersByTimeAsync(2000);
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
});
it('reads a new scope immediately and cancels a pending same-scope refresh', async () => {
  const page = await mount();
  await vi.advanceTimersByTimeAsync(1000);
  vi.mocked(page.queries.getPartyLive).mockClear();
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
  await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(100);
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
  await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(1);
  vi.mocked(page.queries.getPartyLive).mockResolvedValue(ready(view({ scope_id: 'next-scope', members: [] })));
  page.transport.publishPartyChanged({ scope_id: 'next-scope' });
  await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
  expect(screen.queryByText('SynthAlpha')).toBeNull();
  await vi.advanceTimersByTimeAsync(1000);
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
});
it('reads a new scope as soon as the in-flight read finishes', async () => {
  const page = await mount(), pending = deferred<ReturnType<typeof ready<PartyView>>>();
  await vi.advanceTimersByTimeAsync(1000);
  vi.mocked(page.queries.getPartyLive).mockClear();
  vi.mocked(page.queries.getPartyLive).mockReturnValueOnce(pending.promise)
    .mockResolvedValue(ready(view({ scope_id: 'next-scope', members: [] })));
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
  await flush();
  await vi.advanceTimersByTimeAsync(100);
  page.transport.publishPartyChanged({ scope_id: 'next-scope' });
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
  await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(1);
  pending.resolve(ready(view()));
  await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
  expect(screen.queryByText('SynthAlpha')).toBeNull();
  await vi.advanceTimersByTimeAsync(1000);
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
});
it('picks a preset and opens the current run player from the panel', async () => {
  const page = await mount();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Preset' }), { target: { value: 'synth' } });
  expect(page.queries.setPartyPresetPick).toHaveBeenCalledWith('synth');
  await fireEvent.click(roster().getByText('SynthAlpha'));
  await flush();
  expect(screen.getByRole('link', { name: 'Run view' }).getAttribute('href')).toBe('#/runs/synth-run/players/SynthAlpha');
});
it('saves notes on blur and keeps Watch and Ban mutually exclusive', async () => {
  const page = await mount();
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Private note' }), { target: { value: 'Synthetic note' } });
  expect(page.queries.setPartyMark).not.toHaveBeenCalled();
  await fireEvent.blur(screen.getByRole('textbox', { name: 'Private note' })); await flush();
  expect(page.queries.setPartyMark).toHaveBeenLastCalledWith('synthalpha', 'SynthAlpha', null, 'Synthetic note');
  await fireEvent.click(screen.getByRole('button', { name: 'Watch' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Ban' })); await flush();
  expect(page.queries.setPartyMark).toHaveBeenLastCalledWith('synthalpha', 'SynthAlpha', 'banned', 'Synthetic note');
  expect(screen.getByRole('button', { name: 'Watch' }).getAttribute('aria-pressed')).toBe('false');
  expect(screen.getByRole('button', { name: 'Ban' }).getAttribute('aria-pressed')).toBe('true');
});
it('disables invalid RealmEye names with an explanation', async () => {
  await mount(view({ members: [member('SynthInvalid1', 'pass')] }));
  await fireEvent.click(roster().getByText('SynthInvalid1')); await flush();
  const button = screen.getByRole('button', { name: 'Open on RealmEye' }) as HTMLButtonElement;
  expect(button.disabled).toBe(true); expect(button.title).toContain('1 to 10 letters');
});
it.each([false, true])('gates the Run story Party link: %s', async party => {
  const page = fixture();
  page.client.getRun = async () => ready(story());
  render(RunStory, { client: page.client, runId: 'synth-run', features: { ...productionFeatures, party } });
  await flush();
  const link = screen.queryByRole('link', { name: 'Party' });
  expect(link?.getAttribute('href') ?? null).toBe(party ? '#/party/runs/synth-run' : null);
});

it('preserves an unsaved note through a live refresh and queues blur before a mark change', async () => {
  const pending = deferred<void>();
  const setPartyMark = vi.fn().mockImplementationOnce(() => pending.promise).mockResolvedValue(undefined);
  const page = await mount(view(), false, { setPartyMark });
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  const note = screen.getByRole('textbox', { name: 'Private note' }) as HTMLTextAreaElement;
  await fireEvent.input(note, { target: { value: 'Synthetic draft' } });
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(note.value).toBe('Synthetic draft');
  expect(page.queries.getPartyPlayer).toHaveBeenCalledTimes(1);
  await fireEvent.blur(note);
  await fireEvent.click(screen.getByRole('button', { name: 'Watch' }));
  expect(setPartyMark).toHaveBeenCalledTimes(1);
  pending.resolve(); await flush();
  expect(setPartyMark.mock.calls).toEqual([
    ['synthalpha', 'SynthAlpha', null, 'Synthetic draft'], ['synthalpha', 'SynthAlpha', 'watched', 'Synthetic draft'],
  ]);
});

it('copies name and whisper text and opens valid RealmEye names', async () => {
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  const page = await mount();
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Copy name' })); await flush();
  expect(writeText).toHaveBeenLastCalledWith('SynthAlpha');
  expect(screen.getByText('Copied')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Copy /t' })); await flush();
  expect(writeText).toHaveBeenLastCalledWith('/t SynthAlpha ');
  await fireEvent.click(screen.getByRole('button', { name: 'Open on RealmEye' }));
  expect(page.queries.openRealmeye).toHaveBeenCalledWith('SynthAlpha');
});

it('shows history and only the five most recent linked runs', async () => {
  const detail: PartyPlayerView = { row: { key: 'synthalpha', name: 'SynthAlpha', runs_together: 6, deaths: 2,
    left_early: 1, avg_share: .125, last_seen: '1000000', note: 'Synthetic saved note', mark: 'watched' },
    first_seen: '1000000', guild: 'Synthetic guild', runs: Array.from({ length: 6 }, (_, index) => ({
      run_id: `synth-run-${index}`, dungeon: `Synthetic map ${index}`, started: '1000000', share: .1, died: false, left_early: false })) };
  await mount(view(), false, { getPartyPlayer: async () => ready(detail) });
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  const panel = within(screen.getByRole('complementary', { name: 'Player details' }));
  expect(panel.getByText('Synthetic guild')).toBeTruthy();
  expect(panel.getByText('2 of 6')).toBeTruthy();
  expect(panel.getByText('12.5%')).toBeTruthy();
  expect((panel.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Synthetic saved note');
  expect(panel.getByRole('button', { name: 'Watch' }).getAttribute('aria-pressed')).toBe('true');
  const recent = within(screen.getByRole('region', { name: 'Recent runs together' })).getAllByRole('link');
  expect(recent.map(link => link.getAttribute('href'))).toEqual(Array.from({ length: 5 }, (_, index) => `#/party/runs/synth-run-${index}`));
});

it('double-click navigates through the real Shell to this run player', async () => {
  const page = fixture();
  const browser = fakeHistory('#/party');
  render(Shell, { client: page.client, preview: true, features: { ...productionFeatures, party: true }, historyAdapter: browser.adapter });
  await flush();
  await fireEvent.doubleClick(roster().getByText('SynthAlpha')); await flush();
  expect(browser.adapter.hash).toBe('#/runs/synth-run/players/SynthAlpha');
  expect(screen.getByRole('heading', { level: 1, name: 'Player' })).toBeTruthy();
});

it('the scope menu offers the most recent ended run and navigates through Shell', async () => {
  const page = fixture();
  page.client.listRuns = async () => ready({ rows: [run({ id: 'open', ended: null }), run({ id: 'synth-saved' })], next: null });
  const browser = fakeHistory('#/party');
  render(Shell, { client: page.client, preview: true, features: { ...productionFeatures, party: true }, historyAdapter: browser.adapter });
  await flush();
  expect(screen.getByRole('option', { name: 'Last run: Invented Sanctum' })).toBeTruthy();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Party scope' }), { target: { value: 'synth-saved' } }); await flush();
  expect(browser.adapter.hash).toBe('#/party/runs/synth-saved');
  expect(page.queries.getPartyRun).toHaveBeenCalledWith('synth-saved');
});

it('refreshes a saved run on matching engine events, not roster notices or unrelated runs', async () => {
  const page = await mount(view({ live: false }), true);
  page.client.queryStore = async () => ({ state: 'ready', queued: 0, durable: '2', stream_id: homeSnapshot().stream_id, message: '' });
  const publish = (id: string) => page.transport.publishEngineEvent({ stream_id: homeSnapshot().stream_id, sequence: '2', at: '1', event: { run_ended: run({ id }) } });
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' }); publish('other-run');
  await vi.advanceTimersByTimeAsync(1000);
  expect(page.queries.getPartyRun).toHaveBeenCalledTimes(1);
  publish('synth-run'); await flush();
  expect(page.queries.getPartyRun).toHaveBeenCalledTimes(2);
});

it('ignores an old pending read after switching from live to a saved run', async () => {
  const pending = deferred<ReturnType<typeof ready<PartyView>>>();
  const page = fixture(view({ live: false, map: 'Saved synthetic map' }), { getPartyLive: () => pending.promise });
  const component = render(PartyLive, { client: page.client }); await flush();
  await component.rerender({ client: page.client, runId: 'synth-run' }); await flush();
  pending.resolve(ready(view({ map: 'Old synthetic map' }))); await flush();
  expect(screen.queryByRole('option', { name: 'Old synthetic map' })).toBeNull();
  expect(screen.getByRole('option', { name: 'Saved synthetic map' })).toBeTruthy();
});

it('waits for an invalidating event to become durable before refreshing a saved run', async () => {
  const page = await mount(view({ live: false }), true);
  let durable = '1';
  page.client.queryStore = async () => ({ state: 'ready', queued: 0, durable, stream_id: 'synth-store', message: '' });
  page.transport.publishEngineEvent({ stream_id: 'synth-store', sequence: '3', at: '1', event: { run_ended: run({ id: 'synth-run' }) } });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(page.queries.getPartyRun).toHaveBeenCalledTimes(1);
  expect(screen.getByText('Waiting for saved history to catch up.')).toBeTruthy();
  durable = '3';
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(page.queries.getPartyRun).toHaveBeenCalledTimes(2);
});

it('sorts failures before checks, unseen and passing players, then by damage and name', async () => {
  await mount(view({ members: [member('SynthZulu', 'fail', { damage: 10 }), member('SynthAlpha', 'pass', { damage: 200 }),
    member('SynthSeen', 'not_seen'), member('SynthBravo', 'fail', { damage: 100 }), member('SynthCheck', 'check'),
    member('SynthBeta', 'fail', { damage: 10 })] }));
  expect(roster().getAllByRole('row').slice(1).map(row => row.getAttribute('data-row-id')))
    .toEqual(['synthbravo', 'synthbeta', 'synthzulu', 'synthcheck', 'synthseen', 'synthalpha']);
});

it('does not store a Nexus bystander on unchanged note blur, including after a save', async () => {
  const page = await mount(view({ area_kind: 'nexus', run_id: null }));
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  const note = screen.getByRole('textbox', { name: 'Private note' });
  await fireEvent.blur(note); await flush();
  expect(page.queries.setPartyMark).not.toHaveBeenCalled();
  await fireEvent.input(note, { target: { value: 'Synthetic note' } });
  await fireEvent.blur(note); await flush();
  expect(page.queries.setPartyMark).toHaveBeenCalledTimes(1);
  await fireEvent.blur(note); await flush();
  expect(page.queries.setPartyMark).toHaveBeenCalledTimes(1);
});

it('does not save an unchanged loaded note', async () => {
  const detail: PartyPlayerView = { row: { key: 'synthalpha', name: 'SynthAlpha', runs_together: 1,
    deaths: 0, left_early: 0, last_seen: '1000000', note: 'Synthetic saved note' }, first_seen: '1000000', runs: [] };
  const page = await mount(view(), false, { getPartyPlayer: async () => ready(detail) });
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  await fireEvent.blur(screen.getByRole('textbox', { name: 'Private note' })); await flush();
  expect(page.queries.setPartyMark).not.toHaveBeenCalled();
});

it.each(['Watch', 'Ban'])('still deliberately saves %s for a bystander', async action => {
  const page = await mount(view({ area_kind: 'nexus', run_id: null }));
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  await fireEvent.click(screen.getByRole('button', { name: action })); await flush();
  expect(page.queries.setPartyMark).toHaveBeenCalledWith('synthalpha', 'SynthAlpha', action === 'Watch' ? 'watched' : 'banned', null);
});

it('hides local mark and note controls but keeps copy actions', async () => {
  const page = await mount(view({ members: [member('SynthLocal', 'pass', { is_local: true })] }));
  await fireEvent.click(roster().getByText('SynthLocal')); await flush();
  for (const name of ['Watch', 'Ban']) expect(screen.queryByRole('button', { name })).toBeNull();
  expect(screen.queryByRole('textbox', { name: 'Private note' })).toBeNull();
  for (const name of ['Copy name', 'Copy /t']) expect(screen.getByRole('button', { name })).toBeTruthy();
  expect(page.queries.setPartyMark).not.toHaveBeenCalled();
});

it.each(['Enter', ' '])('moves keyboard focus into the panel with %s and back to the originating row', async key => {
  await mount();
  const row = roster().getByText('SynthAlpha').closest('tr')!;
  row.focus(); await fireEvent.keyDown(row, { key }); await flush();
  const heading = screen.getByRole('heading', { name: 'SynthAlpha' });
  expect(heading.getAttribute('tabindex')).toBe('-1');
  expect(document.activeElement).toBe(heading);
  await fireEvent.click(screen.getByRole('button', { name: 'Close player details' })); await flush();
  expect(document.activeElement).toBe(row);
});

it('returns focus to the page heading when a scope change removes the selected row', async () => {
  const page = fixture();
  render(Shell, { client: page.client, preview: true, features: { ...productionFeatures, party: true }, historyAdapter: fakeHistory('#/party').adapter });
  await flush();
  const row = roster().getByText('SynthAlpha').closest('tr')!;
  row.focus(); await fireEvent.keyDown(row, { key: 'Enter' }); await flush();
  vi.mocked(page.queries.getPartyLive).mockResolvedValue(ready(view({ scope_id: 'next-scope', members: [] })));
  page.transport.publishPartyChanged({ scope_id: 'next-scope' });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(screen.queryByRole('complementary', { name: 'Player details' })).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole('heading', { level: 1, name: 'Party' }));
});

it.each(['scope', 'removed'] as const)('preserves outside focus when a background %s change closes the panel', async change => {
  const page = await mount();
  await fireEvent.click(roster().getByText('SynthAlpha')); await flush();
  const preset = screen.getByRole('combobox', { name: 'Preset' });
  preset.focus();
  const next = change === 'scope' ? view({ scope_id: 'next-scope' }) : view({ members: [] });
  vi.mocked(page.queries.getPartyLive).mockResolvedValue(ready(next));
  page.transport.publishPartyChanged({ scope_id: next.scope_id ?? null });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(screen.queryByRole('complementary', { name: 'Player details' })).toBeNull();
  expect(document.activeElement).toBe(preset);
});

it.each(['body', 'outside'] as const)('returns focus on user Close even when focus is on %s', async focus => {
  await mount();
  const row = roster().getByText('SynthAlpha').closest('tr')!;
  await fireEvent.click(row); await flush();
  if (focus === 'outside') screen.getByRole('combobox', { name: 'Preset' }).focus();
  else expect(document.activeElement).toBe(document.body);
  await fireEvent.click(screen.getByRole('button', { name: 'Close player details' })); await flush();
  expect(document.activeElement).toBe(row);
});

it('returns body focus to the retained row when a background scope change closes the panel', async () => {
  const page = await mount();
  const row = roster().getByText('SynthAlpha').closest('tr')!;
  await fireEvent.click(row); await flush();
  expect(document.activeElement).toBe(document.body);
  vi.mocked(page.queries.getPartyLive).mockResolvedValue(ready(view({ scope_id: 'next-scope' })));
  page.transport.publishPartyChanged({ scope_id: 'next-scope' });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(document.activeElement).toBe(row);
});

it('keeps the row, selection and note draft when a player rejoins with a new player ID', async () => {
  const page = await mount();
  const row = roster().getByText('SynthAlpha').closest('tr')!;
  await fireEvent.click(row); await flush();
  const note = screen.getByRole('textbox', { name: 'Private note' });
  await fireEvent.input(note, { target: { value: 'Synthetic draft' } });
  const next = view(); next.members[0].player_id = 'synth-rejoined';
  vi.mocked(page.queries.getPartyLive).mockResolvedValue(ready(next));
  page.transport.publishPartyChanged({ scope_id: next.scope_id ?? null });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(roster().getByText('SynthAlpha').closest('tr')).toBe(row);
  expect(row.getAttribute('aria-selected')).toBe('true');
  expect(screen.getByRole('textbox', { name: 'Private note' })).toBe(note);
  expect((note as HTMLTextAreaElement).value).toBe('Synthetic draft');
  expect(screen.getByRole('link', { name: 'Run view' }).getAttribute('href')).toContain('/players/synth-rejoined');
});

it('keeps tier labels while loading only newly observed visual types', async () => {
  const page = await mount();
  const pending = deferred<VisualPage>();
  const get = vi.spyOn(page.client, 'getObjectVisuals').mockReturnValue(pending.promise);
  const next = view(); next.members[0].class_id = 200;
  vi.mocked(page.queries.getPartyLive).mockResolvedValue(ready(next));
  page.transport.publishPartyChanged({ scope_id: next.scope_id ?? null });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(roster().getByText('T9')).toBeTruthy();
  expect(get.mock.calls).toEqual([[[200]]]);
  pending.resolve({ client_fingerprint: 'synth', objects: [{ object_type: 200, name: 'Synthetic class', sprite: null, bag: null }] });
  await flush(); expect(roster().getByText('T9')).toBeTruthy();
});

it('retains subscription errors after successful reads and retries the subscription once', async () => {
  const page = fixture(), pending = deferred<ReturnType<typeof ready<PartyView>>>();
  vi.mocked(page.queries.getPartyLive).mockReturnValueOnce(pending.promise);
  const subscribe = vi.spyOn(page.client, 'onPartyChanged').mockRejectedValueOnce(new Error('Synthetic subscription failure'));
  const component = render(PartyLive, { client: page.client }); await flush();
  expect(screen.getByRole('alert').textContent).toContain('Party updates unavailable');
  pending.resolve(ready(view())); await flush();
  expect(screen.getByRole('alert').textContent).toContain('Party updates unavailable');
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(subscribe).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole('alert')).toBeNull();
  await vi.advanceTimersByTimeAsync(1000); await flush();
  const reads = vi.mocked(page.queries.getPartyLive).mock.calls.length;
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(reads + 1);
  component.unmount(); page.transport.publishPartyChanged({ scope_id: 'synth-scope' });
  await vi.advanceTimersByTimeAsync(1000);
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(reads + 1);
});

it('shows saved-run breadcrumbs with a link back to the run', async () => {
  await mount(view({ live: false }), true);
  const crumbs = within(screen.getByRole('navigation', { name: 'Breadcrumb' }));
  expect(crumbs.getByRole('link', { name: 'Party' }).getAttribute('href')).toBe('#/party');
  expect(crumbs.getByRole('link', { name: 'Synthetic Sanctum' }).getAttribute('href')).toBe('#/runs/synth-run');
});

it.each([null, 'synth-run'])('only offers the double-click hint with a run: %s', async run_id => {
  await mount(view({ run_id }));
  expect(screen.queryByText(/double-click opens/) !== null).toBe(run_id !== null);
});

it('shows dungeon flags in Notes and the saved-run Left early count', async () => {
  await mount(view({ live: false, members: [member('SynthAlpha', 'pass', { flags: ['no_damage_seen'] }),
    member('SynthBravo', 'pass', { flags: ['mostly_trash'] }),
    member('SynthLeft', 'pass', { flags: ['left_early'], departed_at: '1000000', present: null })] }), true);
  for (const label of ['No damage seen', 'Mostly trash', 'Left early']) {
    expect(roster().getByText(label).closest('td')?.classList.contains('notes')).toBe(true);
  }
  const summary = within(screen.getByRole('region', { name: 'Party summary' }));
  expect(summary.getByText('Left early').nextElementSibling?.textContent).toBe('1');
});

it.each([false, true])('engine-event batches re-read saved runs only: saved=%s', async saved => {
  const page = fixture();
  const sequence = String(BigInt(homeSnapshot().sequence) + 1n);
  page.client.queryStore = async () => ({ state: 'ready', queued: 0, durable: sequence, stream_id: homeSnapshot().stream_id, message: '' });
  render(Shell, { client: page.client, preview: true, features: { ...productionFeatures, party: true },
    historyAdapter: fakeHistory(saved ? '#/party/runs/synth-run' : '#/party').adapter });
  await flush();
  const query = saved ? page.queries.getPartyRun : page.queries.getPartyLive;
  const reads = vi.mocked(query).mock.calls.length;
  expect(reads).toBeGreaterThan(0);
  page.transport.publishEngineEvent({ stream_id: homeSnapshot().stream_id, first_sequence: sequence, last_sequence: sequence,
    events: [{ stream_id: homeSnapshot().stream_id, sequence, at: '1', event: { run_ended: run({ id: 'synth-run' }) } }] });
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(query).toHaveBeenCalledTimes(reads + (saved ? 1 : 0));
});

it('shows counts for every filter and keeps them independent of the active filter', async () => {
  const data = view(); data.members[1].mark = 'watched';
  await mount(data);
  const labels = ['All 4', 'Failing 1', 'Check 1', 'Not seen 1', 'Flagged 1', 'Marked 1'];
  for (const name of labels) expect(screen.getByRole('button', { name })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Marked 1' }));
  for (const name of labels) expect(screen.getByRole('button', { name })).toBeTruthy();
  expect(roster().getAllByRole('row')).toHaveLength(2);
});

it.each(['auto', 'picked', 'default'] as const)('shows the active preset summary in one keyboard accessible control with %s source', async source => {
  const page = await mount(view({ preset: { id: 'synth', name: 'Synthetic', source } }));
  const select = screen.getByRole('combobox', { name: 'Preset' }) as HTMLSelectElement;
  select.focus(); expect(document.activeElement).toBe(select);
  expect(select.selectedOptions[0].textContent).toBe('Preset: Synthetic · 6/8 · T11+ weapon · UT/ST ok');
  expect(screen.getByText(source, { exact: true })).toBeTruthy();
  await fireEvent.change(select, { target: { value: source === 'picked' ? '' : 'synth' } }); await flush();
  expect(page.queries.setPartyPresetPick).toHaveBeenCalledWith(source === 'picked' ? null : 'synth');
});


it('throttles the trailing read when an in-flight read already returns the pending scope', async () => {
  const page = await mount(), pending = deferred<ReturnType<typeof ready<PartyView>>>();
  await vi.advanceTimersByTimeAsync(1000);
  vi.mocked(page.queries.getPartyLive).mockReturnValueOnce(pending.promise);
  page.transport.publishPartyChanged({ scope_id: 'synth-scope' }); await flush();
  await vi.advanceTimersByTimeAsync(100);
  page.transport.publishPartyChanged({ scope_id: 'next-scope' });
  pending.resolve(ready(view({ scope_id: 'next-scope' }))); await flush();
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(899);
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1);
  expect(page.queries.getPartyLive).toHaveBeenCalledTimes(3);
});

it('throttles null-scope notices when the live view is null', async () => {
  const getPartyLive = vi.fn(async () => ({ status: 'busy' as const }));
  const page = await mount(view(), false, { getPartyLive });
  page.transport.publishPartyChanged({ scope_id: null }); await flush();
  await vi.advanceTimersByTimeAsync(500);
  page.transport.publishPartyChanged({ scope_id: null }); await flush();
  expect(getPartyLive).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(500);
  expect(getPartyLive).toHaveBeenCalledTimes(2);
});

it.each(['auto', 'default'] as const)('describes an unpicked %s preset absent from the list', async source => {
  const page = await mount(view({ preset: { id: 'synth-missing', name: 'Synthetic missing', source } }));
  const select = screen.getByRole('combobox', { name: 'Preset', description: source }) as HTMLSelectElement;
  expect(select.value).toBe('');
  expect(select.selectedOptions[0].textContent).toBe('Preset: Synthetic missing');
  expect([...select.options].map(option => option.value)).toEqual(['', 'synth-missing', 'synth']);
  expect(select.getAttribute('aria-describedby')).toBe(screen.getByText(source, { selector: '.caption' }).id);
  await fireEvent.change(select, { target: { value: 'synth-missing' } });
  expect(page.queries.setPartyPresetPick).toHaveBeenCalledWith('synth-missing');
});

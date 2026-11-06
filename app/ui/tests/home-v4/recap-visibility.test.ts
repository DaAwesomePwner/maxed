import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { RunView } from '../../src/lib/bindings/RunView';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { setNativePresentationVisible } from '../../src/lib/bridge/visibility';
import { createRecapTracker } from '../../src/lib/home/v4/recap';
import { productionFeatures } from '../../src/lib/navigation/routes';
import Shell from '../../src/lib/shell/Shell.svelte';
import { catalog, fight, homeSnapshot, realmSnapshot } from '../home-v2/fixtures';
import { fakeHistory } from '../navigation/fake-history';

const projections = vi.hoisted(() => ({ observed: vi.fn(), published: vi.fn() }));
vi.mock('../../src/lib/home/v2/live', async importOriginal => {
  const actual = await importOriginal<typeof import('../../src/lib/home/v2/live')>();
  return { ...actual, createHomeLiveWindow: ((receive, observe) => actual.createHomeLiveWindow(
    window => { projections.published(window); receive(window); },
    observe ? { onEvent: observe.onEvent,
      onWindow: window => { projections.observed(window); observe.onWindow(window); } } : undefined,
  )) as typeof actual.createHomeLiveWindow };
});

vi.mock('../../src/lib/home/v4/recap', async importOriginal => {
  const actual = await importOriginal<typeof import('../../src/lib/home/v4/recap')>();
  return { ...actual, createRecapTracker: vi.fn(() => {
    const tracker = actual.createRecapTracker();
    return { ...tracker, observe: vi.fn(tracker.observe), onEvent: vi.fn(tracker.onEvent), setVisible: vi.fn(tracker.setVisible) };
  }) };
});
beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); setNativePresentationVisible(true); vi.clearAllTimers(); vi.useRealTimers(); vi.clearAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
async function openRecap(page: Awaited<ReturnType<typeof mount>>['page']) {
  if (!page.queryByRole('region', { name: 'Recap' })) {
    await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  }
}


async function mount(redesign = true, initial: LiveSnapshot = homeSnapshot()) {
  const source = { ...initial, fights: [], drops: [] };
  const local = { ...homeSnapshot().fights[0].players[0], dps: 4321, rank: 1 };
  source.run_totals = { fights: 1, kills: 0, total_damage: 9000, unattributed_damage: 0,
    players: [local], local, partial: false };
  const drop = { ...homeSnapshot().drops[0], id: 'synthetic-final-bag', at: '6600000', bag_rank: 255 };
  const itemType = drop.contents[0]?.item_type ?? drop.items[0];
  const transport = createFixtureTransport(source, { client_fingerprint: 'a'.repeat(64), objects: [
    catalog(300, 'Synthetic first encounter'), catalog(301, 'Synthetic last boss'),
    catalog(drop.bag_type, 'Synthetic white bag'), catalog(itemType, 'Synthetic prize'),
  ] });
  const client = createClient(transport);
  const page = render(Shell, { client, features: { ...productionFeatures, homeRedesign: redesign },
    preview: true, historyAdapter: fakeHistory('#/').adapter });
  await settle();
  let sequence = BigInt(source.sequence);
  const envelope = (event: EngineEvent, at: string): EventEnvelope => ({
    stream_id: source.stream_id, sequence: String(++sequence), at, event,
  });
  const send = (event: EngineEvent, at: string) => transport.publishEngineEvent(envelope(event, at));
  send({ map_entered: { at: '6000000', name: source.map!, kind: source.area_kind,
    realm_instance_key: source.area_kind === 'realm' ? 'synthetic-realm' : null } }, '6000000');
  if (source.run) send({ run_started: source.run }, '6000000');
  send({ character_updated: source.character! }, '6000000');
  transport.publishSnapshot({ ...source, sequence: String(sequence), at: '6000000' });
  await settle();
  const finish = () => {
    const first = fight('synthetic-first', { target_type: 300, target_kind: 'boss', started: '6100000', ended: null });
    const boss = fight('synthetic-last', { target_type: 301, target_kind: 'boss', started: '6300000', ended: null });
    const killed = { ...boss, target_hp: 0, kill: { at: '6400000', evidence: 'hp_zero_removed' as const, provisional: false } };
    const events = [
      envelope({ fight_started: first }, first.started),
      envelope({ fight_ended: { ...first, ended: '6200000' } }, '6200000'),
      envelope({ fight_started: boss }, boss.started),
      envelope({ fight_updated: killed }, '6400000'),
      envelope({ fight_ended: { ...killed, ended: '6500000' } }, '6500000'),
      envelope({ bag_dropped: drop }, drop.at),
      envelope({ run_ended: { ...source.run!, ended: '6700000', outcome: 'cleared' } }, '6700000'),
      envelope({ map_entered: { at: '6800000', kind: 'nexus', name: 'Nexus' } }, '6800000'),
    ];
    // A native batch can contain the final boss and departure. The final window
    // displayed for the batch is Nexus; the recap must retain the run before it.
    transport.publishEngineEvent({ stream_id: source.stream_id,
      first_sequence: events[0].sequence, last_sequence: events.at(-1)!.sequence, events });
    transport.publishSnapshot({ ...source, sequence: String(sequence), at: '6900000', area_kind: 'nexus', map: 'Nexus',
      run: null, run_totals: null, fights: [], drops: [] });
  };
  const publish = (snapshot: LiveSnapshot) => transport.publishSnapshot({ ...snapshot, sequence: String(sequence) });
  const batch = (events: [EngineEvent, string][]) => {
    const rows = events.map(([event, at]) => envelope(event, at));
    transport.publishEngineEvent({ stream_id: source.stream_id,
      first_sequence: rows[0].sequence, last_sequence: rows.at(-1)!.sequence, events: rows });
  };
  return { page, finish, source, send, batch, publish, nav: within(page.getByRole('navigation', { name: 'Navigation' })) };
}

it.each(['inactive', 'hidden', 'visible'] as const)('retains the final batch of fights, kill, loot and outcome while %s', async mode => {
  const { page, nav, finish } = await mount();
  if (mode === 'inactive') { await fireEvent.click(nav.getByRole('link', { name: 'Runs' })); await settle(); }
  if (mode === 'hidden') setNativePresentationVisible(false);
  finish(); await settle();
  if (mode === 'hidden') { setNativePresentationVisible(true); await settle(); }
  if (mode === 'inactive') { await fireEvent.click(nav.getByRole('link', { name: 'Home' })); await settle(); }
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByText('Cleared')).toBeTruthy();
  expect(recap.getByRole('link', { name: 'Synthetic first encounter' })).toBeTruthy();
  expect(recap.getByRole('link', { name: 'Synthetic last boss' })).toBeTruthy();
  expect(recap.getByText('Killed')).toBeTruthy();
  expect(recap.getByText('Synthetic prize')).toBeTruthy();
  expect(recap.getByText('Your DPS · Whole run').parentElement?.querySelector('dd')?.textContent)
    .toBe(mode === 'hidden' ? '—' : '4,321');
  expect(recap.getByText('Rank').parentElement?.querySelector('dd')?.textContent).toBe(mode === 'hidden' ? '—' : '#1');
  expect(recap.getByText('Observed players').parentElement?.querySelector('dd')?.textContent).toBe(mode === 'hidden' ? '—' : '1');
  expect(page.container.textContent).not.toContain('Maxed was hidden');
});

it('does no recap observation work with homeRedesign disabled', async () => {
  const { page, finish } = await mount(false);
  setNativePresentationVisible(false); finish(); await settle();
  setNativePresentationVisible(true); await settle();
  const tracker = vi.mocked(createRecapTracker).mock.results.at(-1)!.value;
  expect(tracker.observe).not.toHaveBeenCalled();
  expect(tracker.onEvent).not.toHaveBeenCalled();
  expect(tracker.setVisible).not.toHaveBeenCalled();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
});

it('flag_off_projects_and_publishes_a_multi_event_batch_once', async () => {
  const { batch } = await mount(false);
  projections.observed.mockClear(); projections.published.mockClear();
  batch(Array.from({ length: 8 }, (_, i) => [{ fight_updated: fight(`synthetic-batch-${i}`) }, '6500000']));
  await settle();
  expect(projections.observed).not.toHaveBeenCalled();
  expect(projections.published).toHaveBeenCalledOnce();
});

it('flag_on_builds_observes_and_publishes_eight_fight_updates_once', async () => {
  const { batch } = await mount();
  const tracker = vi.mocked(createRecapTracker).mock.results.at(-1)!.value;
  tracker.onEvent.mockClear();
  projections.observed.mockClear(); projections.published.mockClear();
  batch(Array.from({ length: 8 }, (_, i) => [{ fight_updated: fight(`synthetic-batch-${i}`) }, '6500000']));
  await settle();
  // Every built projection is delivered to the observer; publication reuses it.
  expect(projections.observed).toHaveBeenCalledOnce();
  expect(new Set(projections.observed.mock.calls.map(([window]) => window)).size).toBe(1);
  expect(projections.published).toHaveBeenCalledOnce();
  expect(projections.published.mock.lastCall![0]).toBe(projections.observed.mock.lastCall![0]);
  expect(tracker.onEvent.mock.calls.map(([envelope]: [EventEnvelope]) =>
    'fight_updated' in envelope.event && envelope.event.fight_updated.id))
    .toEqual(Array.from({ length: 8 }, (_, i) => `synthetic-batch-${i}`));
});

const nexus = (source: LiveSnapshot, at = '1860000000'): LiveSnapshot => ({ ...source, at,
  area_kind: 'nexus', map: 'Nexus', run: null, run_totals: null, fights: [], drops: [], instance: null });
const stat = (page: Awaited<ReturnType<typeof mount>>['page'], name: string) => within(page.getByRole('region', { name: 'Recap' }))
  .getByText(name).parentElement?.querySelector('dd')?.textContent;
const run = (id: string, dungeon: string, started: string): RunView => ({ ...homeSnapshot().run!, id, dungeon, started });
const enter = (run: RunView): [EngineEvent, string][] => [
  [{ map_entered: { at: run.started, name: run.dungeon, kind: 'dungeon' } }, run.started],
  [{ run_started: run }, run.started],
];
const complete = (run: RunView): [EngineEvent, string][] => [
  [{ fight_ended: fight(`${run.id}-boss`, { run_id: run.id, target_type: 301,
    started: run.started, ended: '59000000', kill: { at: '59000000', evidence: 'hp_zero_removed', provisional: false } }) }, '59000000'],
  [{ bag_dropped: { ...homeSnapshot().drops[0], id: `${run.id}-bag`, run_id: run.id, at: '59000000' } }, '59000000'],
  [{ run_ended: { ...run, ended: '60000000', outcome: 'cleared' } }, '60000000'],
];
const leave: [EngineEvent, string] = [{ map_entered: { at: '61000000', name: 'Nexus', kind: 'nexus' } }, '61000000'];

it.each(['hidden', 'visible'] as const)('%s realm departure excludes thirty minutes in Nexus', async mode => {
  const { page, source, send, publish } = await mount(true, realmSnapshot());
  if (mode === 'hidden') setNativePresentationVisible(false);
  send({ map_entered: { at: '60000000', name: 'Nexus', kind: 'nexus' } }, '60000000');
  await settle();
  if (mode === 'hidden') setNativePresentationVisible(true);
  publish(nexus(source)); await settle();
  await openRecap(page);
  expect(stat(page, 'Duration')).toBe('0:54');
  const tracker = vi.mocked(createRecapTracker).mock.results.at(-1)!.value;
  let endedAt: string | null | undefined;
  tracker.subscribe((value: { summary: { endedAt: string | null } } | null) => { endedAt = value?.summary.endedAt; })();
  expect(endedAt).toBe('60000000');
  expect(within(page.getByRole('region', { name: 'Recap' })).getByRole('heading', { name: 'Synthetic Realm' })).toBeTruthy();
});

it.each(['hidden', 'visible'] as const)('%s realm to Castle to Nexus recaps Castle only', async mode => {
  const { page, source, batch, publish } = await mount(true, realmSnapshot());
  const castle = run('synthetic-castle', "Oryx's Castle", '10000000');
  if (mode === 'hidden') setNativePresentationVisible(false);
  batch(enter(castle)); await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  if (mode === 'visible') { publish({ ...source, at: castle.started, area_kind: 'dungeon', map: castle.dungeon, run: castle }); await settle(); }
  batch([...complete(castle), leave]); await settle();
  if (mode === 'hidden') setNativePresentationVisible(true);
  publish(nexus(source)); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByRole('heading', { name: castle.dungeon })).toBeTruthy();
  expect(recap.getByText('Cleared')).toBeTruthy();
  expect(recap.getByRole('link', { name: 'Synthetic last boss' }).getAttribute('href')).toContain(`${castle.id}-boss`);
  expect(recap.getByText('Synthetic prize')).toBeTruthy();
  expect(stat(page, 'Duration')).toBe('0:50');
  expect(stat(page, 'Your DPS · Whole run')).toBe(mode === 'hidden' ? '—' : '4,321');
  expect(stat(page, 'Rank')).toBe(mode === 'hidden' ? '—' : '#1');
  expect(stat(page, 'Observed players')).toBe(mode === 'hidden' ? '—' : '1');
});

it.each(['hidden', 'visible'] as const)('%s run A to run B to Nexus retains run B only', async mode => {
  const { page, source, batch, publish } = await mount();
  const second = run('synthetic-run-b', 'Synthetic second dungeon', '20000000');
  if (mode === 'hidden') setNativePresentationVisible(false);
  batch([[{ run_ended: { ...source.run!, ended: '15000000', outcome: 'left' } }, '15000000'], ...enter(second)]);
  await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  if (mode === 'visible') { publish({ ...source, at: second.started, map: second.dungeon, run: second }); await settle(); }
  batch([...complete(second), leave]); await settle();
  if (mode === 'hidden') setNativePresentationVisible(true);
  publish(nexus(source)); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByRole('heading', { name: second.dungeon })).toBeTruthy();
  expect(recap.getByRole('link', { name: 'Open run story ›' }).getAttribute('href')).toContain(second.id);
  expect(recap.getByRole('link', { name: 'Synthetic last boss' }).getAttribute('href')).toContain(`${second.id}-boss`);
  expect(recap.queryByText(source.run!.dungeon)).toBeNull();
});

it('a_fresh_terminal_snapshot_refines_a_run_played_entirely_while_hidden', async () => {
  const { page, source, batch, publish } = await mount(true, realmSnapshot());
  const castle = run('synthetic-castle', "Oryx's Castle", '10000000');
  setNativePresentationVisible(false);
  batch([...enter(castle), ...complete(castle), leave]); await settle();
  setNativePresentationVisible(true);
  publish({ ...nexus(source), run: { ...castle, ended: '60000000', outcome: 'cleared' }, run_totals: source.run_totals });
  await settle();
  await openRecap(page);
  expect(stat(page, 'Your DPS · Whole run')).toBe('4,321');
  expect(stat(page, 'Rank')).toBe('#1');
  expect(stat(page, 'Observed players')).toBe('1');
  expect(within(page.getByRole('region', { name: 'Recap' })).getByRole('link', { name: 'Synthetic last boss' })).toBeTruthy();
});

it('a_hidden_new_realm_does_not_inherit_the_previous_realms_title', async () => {
  const { page, source, batch, publish } = await mount(true, realmSnapshot());
  setNativePresentationVisible(false);
  batch([[{ map_entered: { at: '10000000', name: 'Realm', kind: 'realm', realm_instance_key: 'synthetic-next-realm' } }, '10000000'], leave]);
  await settle(); setNativePresentationVisible(true); publish(nexus(source)); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByRole('heading', { level: 2 }).textContent).toBe('—');
  expect(stat(page, 'Duration')).toBe('0:51');
});

const realmEvidence = (id: string, at: string): [EngineEvent, string][] => [
  [{ fight_ended: fight(id, { run_id: null, target_type: 301, started: at, ended: at,
    kill: { at, evidence: 'hp_zero_removed', provisional: false } }) }, at],
  [{ bag_dropped: { ...homeSnapshot().drops[0], id: `${id}-bag`, run_id: null, bag_rank: 255, at } }, at],
];

it('snapshot_first_realm_departure_keeps_the_complete_fights_and_best_bag', async () => {
  const { page, source, batch, publish, send } = await mount(true, realmSnapshot());
  batch(realmEvidence('synthetic-snapshot-first-boss', '10000000'));
  publish(nexus(source));
  send(...leave); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByRole('link', { name: 'Synthetic last boss' }).getAttribute('href')).toContain('synthetic-snapshot-first-boss');
  expect(recap.getByText('Killed')).toBeTruthy();
  expect(recap.getByText('Synthetic prize')).toBeTruthy();
});

it('snapshot_first_session_change_observes_only_the_new_scope', async () => {
  const { source, batch, publish } = await mount(true, realmSnapshot());
  batch(realmEvidence('synthetic-previous-session-boss', '10000000'));
  projections.observed.mockClear();
  publish({ ...source, at: '20000000', session: { ...source.session!, id: 'synthetic-next-session' } });
  await settle();
  expect(projections.observed).toHaveBeenCalledOnce();
  expect(projections.observed.mock.lastCall![0]).toMatchObject({ visitKey: null, fights: [], drops: [] });
});

it('checkpoints_realm_fights_and_best_bag_before_map_entry_without_run_end', async () => {
  const { page, source, batch, publish } = await mount(true, realmSnapshot());
  batch([...realmEvidence('synthetic-realm-boss', '10000000'), leave]);
  publish(nexus(source)); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByRole('link', { name: 'Synthetic last boss' }).getAttribute('href')).toContain('synthetic-realm-boss');
  expect(recap.getByText('Killed')).toBeTruthy();
  expect(recap.getByText('Synthetic prize')).toBeTruthy();
  expect(stat(page, 'Duration')).toBe('0:55');
});

it('same_batch_castle_to_chamber_never_displays_a_castle_recap', async () => {
  const castle = run('synthetic-castle', "Oryx's Castle", '6000000');
  const chamber = run('synthetic-chamber', "Oryx's Chamber", '62000000');
  const { page, source, batch, publish } = await mount(true, { ...homeSnapshot(), map: castle.dungeon, run: castle });
  batch([...complete(castle), ...enter(chamber)]); await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  publish({ ...source, at: chamber.started, map: chamber.dungeon, run: chamber }); await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  batch([[{ run_ended: { ...chamber, ended: '70000000', outcome: 'left' } }, '70000000'],
    [{ map_entered: { at: '71000000', name: 'Nexus', kind: 'nexus' } }, '71000000']]);
  publish(nexus(source)); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByRole('heading', { name: chamber.dungeon })).toBeTruthy();
  expect(recap.queryByText(castle.dungeon)).toBeNull();
});

it('same_batch_realm_departure_and_reentry_retains_one_combined_visit', async () => {
  const { page, source, batch, publish } = await mount(true, realmSnapshot());
  batch([...realmEvidence('synthetic-first-segment', '10000000'), leave,
    [{ map_entered: { at: '70000000', name: source.map!, kind: 'realm', realm_instance_key: 'synthetic-realm' } }, '70000000'],
    [{ character_updated: source.character! }, '70000000']]);
  publish({ ...source, at: '70000000' }); await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  batch([...realmEvidence('synthetic-second-segment', '80000000'),
    [{ map_entered: { at: '90000000', name: 'Nexus', kind: 'nexus' } }, '90000000']]);
  publish(nexus(source)); await settle();
  await openRecap(page);
  expect(page.getAllByRole('region', { name: 'Recap' })).toHaveLength(1);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getAllByRole('link', { name: 'Synthetic last boss' }).map(link => link.getAttribute('href')))
    .toEqual(['#/fights/synthetic-first-segment', '#/fights/synthetic-second-segment']);
  expect(recap.getByText('Synthetic prize')).toBeTruthy();
});

it('terminal_run_updated_checkpoints_the_final_fight_and_loot', async () => {
  const { page, source, batch, publish } = await mount();
  const events = complete(source.run!);
  const terminal = (events.pop()![0] as { run_ended: RunView }).run_ended;
  batch([...events, [{ run_updated: terminal }, terminal.ended!], leave]);
  publish(nexus(source)); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.getByText('Cleared')).toBeTruthy();
  expect(recap.getByRole('link', { name: 'Synthetic last boss' })).toBeTruthy();
  expect(recap.getByText('Synthetic prize')).toBeTruthy();
});

it('a_truncated_checkpoint_keeps_fights_and_best_bag_unknown', async () => {
  const { page, source, batch, publish } = await mount(true, realmSnapshot());
  batch([...Array.from({ length: 65 }, (_, i): [EngineEvent, string] => [{ fight_ended: fight(`synthetic-trim-${i}`,
    { run_id: null, started: String(10000000 + i), ended: String(10000001 + i) }) }, String(10000001 + i)]),
    ...realmEvidence('synthetic-final-trim', '20000000'), leave]);
  publish(nexus(source)); await settle();
  await openRecap(page);
  const recap = within(page.getByRole('region', { name: 'Recap' }));
  expect(recap.queryByRole('link', { name: 'Synthetic last boss' })).toBeNull();
  expect(recap.queryByText('Synthetic prize')).toBeNull();
  expect(recap.getAllByText('—').length).toBeGreaterThanOrEqual(2);
  expect(recap.getByRole('heading', { name: 'Observed fights' }).nextElementSibling?.textContent).toBe('—');
  expect(recap.getByRole('heading', { name: 'Best bag' }).nextElementSibling?.textContent).toBe('—');
});

it('keeps Nexus visible and counts only mounted, visible Home time', async () => {
  const { page, nav, finish } = await mount();
  finish(); await settle();
  expect(page.queryByRole('region', { name: 'Recap' })).toBeNull();
  expect(page.container.querySelector('.nexus-home')).toBeTruthy();
  expect(page.getByRole('region', { name: 'Recap ready' })).toBeTruthy();
  await vi.advanceTimersByTimeAsync(5_000);
  await fireEvent.click(nav.getByRole('link', { name: 'Runs' })); await settle();
  await vi.advanceTimersByTimeAsync(60_000);
  await fireEvent.click(nav.getByRole('link', { name: 'Home' })); await settle();
  setNativePresentationVisible(false);
  await vi.advanceTimersByTimeAsync(60_000);
  setNativePresentationVisible(true); await settle();
  await vi.advanceTimersByTimeAsync(9_999);
  expect(page.getByRole('region', { name: 'Recap ready' })).toBeTruthy();
  await vi.advanceTimersByTimeAsync(1);
  expect(page.queryByRole('region', { name: 'Recap ready' })).toBeNull();
  await openRecap(page);
  expect(page.getByRole('region', { name: 'Recap' })).toBeTruthy();
});

it.each(['hover', 'focus'] as const)('banner pauses on %s; dismiss retains access and restores owned focus', async interaction => {
  const { page, finish } = await mount();
  finish(); await settle();
  const banner = page.getByRole('region', { name: 'Recap ready' });
  const dismiss = within(banner).getByRole('button', { name: 'Dismiss recap banner' });
  await vi.advanceTimersByTimeAsync(5_000);
  if (interaction === 'hover') await fireEvent.mouseEnter(banner);
  else { dismiss.focus(); await settle(); }
  await vi.advanceTimersByTimeAsync(60_000);
  expect(page.getByRole('region', { name: 'Recap ready' })).toBeTruthy();
  dismiss.focus(); await fireEvent.click(dismiss); await settle();
  const last = page.getByRole('button', { name: /^Last recap:/ });
  expect(document.activeElement).toBe(last);
  expect(page.queryByRole('region', { name: 'Recap ready' })).toBeNull();
  await fireEvent.click(last); await settle();
  expect(page.getByRole('region', { name: 'Recap' })).toBeTruthy();
});

it.each(['dungeon', 'realm'] as const)('entry into %s hides banner, keeps previous recap and Back restores live Home', async area => {
  const { page, finish, source, batch, publish } = await mount();
  finish(); await settle();
  const button = page.getByRole('button', { name: 'View recap' });
  button.focus(); await settle();
  const next = run('synthetic-next-run', 'Synthetic next dungeon', '10000000');
  if (area === 'dungeon') batch(enter(next));
  else batch([[{ map_entered: { at: next.started, name: 'Synthetic next realm', kind: 'realm' } }, next.started]]);
  publish({ ...source, area_kind: area, at: next.started, run: area === 'dungeon' ? next : null }); await settle();
  expect(page.queryByRole('region', { name: 'Recap ready' })).toBeNull();
  const last = page.getByRole('button', { name: `Last recap: ${source.run!.dungeon}` });
  expect(document.activeElement).toBe(last);
  await fireEvent.click(last); await settle();
  expect(within(page.getByRole('region', { name: 'Recap' })).getByRole('heading', { name: source.run!.dungeon })).toBeTruthy();
  await fireEvent.click(within(page.getByRole('region', { name: 'Recap' })).getByRole('button', { name: 'Back' })); await settle();
  expect(page.container.querySelector(`.${area}-screen`)).toBeTruthy();
  expect(page.getByRole('button', { name: /^Last recap:/ })).toBeTruthy();
});

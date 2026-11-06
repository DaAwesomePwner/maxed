import { describe, expect, it, vi } from 'vitest';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { PartyView } from '../../src/lib/bindings/PartyView';
import type { PartySettings } from '../../src/lib/bindings/PartySettings';
import type { Route } from '../../src/lib/navigation/routes';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport, type PartyFixtureQueries } from '../../src/lib/bridge/fixture';
import { invalidatesParty, partyNoticeInvalidates } from '../../src/lib/party/invalidation';
import { homeSnapshot } from '../home/fixtures';
import { fight, ready, run } from '../runs/fixtures';

const envelope = (event: EventEnvelope['event']): EventEnvelope => ({ stream_id: 'synth-live', sequence: '2', at: '1', event });
const routes: Array<Route | undefined> = [
  { kind: 'party-live' }, { kind: 'party-run', runId: 'run-1' }, { kind: 'party-players' },
  { kind: 'party-player', key: 'synthalpha' }, { kind: 'party-presets' }, { kind: 'home' }, undefined,
];
const none = [false, false, false, false, false, false, false];
const cases: Array<[string, EventEnvelope['event'], boolean[]]> = [
  ['matching run ended', { run_ended: run() }, [false, true, true, true, false, false, false]],
  ['other run ended', { run_ended: run({ id: 'other' }) }, [false, false, true, true, false, false, false]],
  ['run started', { run_started: run() }, none],
  ['run updated', { run_updated: run() }, none],
  ['fight started', { fight_started: fight() }, [false, true, false, false, false, false, false]],
  ['fight updated', { fight_updated: fight() }, [false, true, false, false, false, false, false]],
  ['fight ended', { fight_ended: fight() }, [false, true, false, false, false, false, false]],
  ['other fight started', { fight_started: fight({ run_id: 'other' }) }, none],
  ['other fight updated', { fight_updated: fight({ run_id: 'other' }) }, none],
  ['other fight ended', { fight_ended: fight({ run_id: 'other' }) }, none],
  ['realm fight ended', { fight_ended: fight({ run_id: null }) }, none],
  ['map entered', { map_entered: { at: '1', name: 'Nexus', kind: 'nexus' } }, none],
  ['profile', { player_observed: { scope_id: 'synth-scope', run_id: 'run-1', player_id: 'synth-player', object_id: 1, is_local: false,
    name: 'SynthAlpha', stats: [], maxed_stats: [], equipment: [], first_seen: '1', last_seen: '1' } }, none],
  ['instance event', { instance_event: { id: 'synth-event', session_id: 'synth-session', run_id: 'run-1', at: '1',
    kind: { type: 'player_left', object_id: 1, player_name: 'SynthAlpha' }, provisional: false } }, none],
  ['fame changed', { fame_changed: { at: '1', character_id: 'synth-character', fame: 1 } }, none],
];

describe('Party invalidation', () => {
  it.each(cases)('invalidates only the relevant routes for %s', (_name, event, expected) => {
    expect(routes.map(route => invalidatesParty(route, envelope(event)))).toEqual(expected);
  });

  it.each(['synth-scope', null])('refreshes only Live on a notice, including scope changes: %s', scope_id => {
    expect(routes.map(route => partyNoticeInvalidates(route, { scope_id }))).toEqual([true, false, false, false, false, false, false]);
  });
});

const live: PartyView = { scope_id: 'synth-scope', live: true, area_kind: 'nexus',
  counts: { in_view: 0, pass: 0, fail: 0, check: 0, not_seen: 0, no_damage_seen: 0, left_early: 0 },
  classes: [], members: [], assets_available: false, gear_recorded: false };
const settings: PartySettings = { presets: [], default_preset_id: null };
const fixture = (party?: PartyFixtureQueries) => createFixtureTransport(homeSnapshot(), undefined, undefined,
  undefined, undefined, undefined, undefined, {}, undefined, undefined, undefined, party);

describe('Party client and fixture transport', () => {
  it('sends all nine commands with native argument names and preserves replies', async () => {
    const transport = fixture({
      getPartyLive: async () => ready(live), getPartyRun: async () => ready(null),
      getPartyPlayers: async () => ready({ rows: [], next: null }), getPartyPlayer: async () => ready(null),
      setPartyMark: async () => {}, clearPartyHistory: async () => {}, setPartyPresetPick: async () => {},
      openRealmeye: async () => {}, updatePartySettings: async () => {},
    });
    const invoke = vi.spyOn(transport, 'invoke');
    const client = createClient(transport);
    await expect(client.getPartyLive()).resolves.toEqual(ready(live));
    await expect(client.getPartyRun('synth:run%/ 1')).resolves.toEqual(ready(null));
    const cursor = { sort_value: '4607182418800017408', key: 'synthalpha' };
    await expect(client.getPartyPlayers('Synth', 'watched', 'avg_share', cursor)).resolves.toEqual(ready({ rows: [], next: null }));
    await client.getPartyPlayers('', 'all', 'last_seen', null);
    const runsCursor = { started: '9007199254740993', run_id: 'synth-run' };
    await expect(client.getPartyPlayer('synthalpha', runsCursor)).resolves.toEqual(ready(null));
    await client.getPartyPlayer('synthalpha', null);
    await client.setPartyMark('synthalpha', 'SynthAlpha', 'watched', 'Synthetic note');
    await client.setPartyMark('synthalpha', 'SynthAlpha', null, null);
    await client.clearPartyHistory({ marks_and_notes: false });
    await client.clearPartyHistory({ marks_and_notes: true });
    await client.setPartyPresetPick('synth-preset');
    await client.setPartyPresetPick(null);
    await client.openRealmeye('SynthAlpha');
    await client.updatePartySettings(settings);
    expect(invoke.mock.calls).toEqual([
      ['get_party_live', undefined], ['get_party_run', { runId: 'synth:run%/ 1' }],
      ['get_party_players', { query: 'Synth', filter: 'watched', sort: 'avg_share', cursor }],
      ['get_party_players', { query: '', filter: 'all', sort: 'last_seen', cursor: null }],
      ['get_party_player', { key: 'synthalpha', cursor: runsCursor }], ['get_party_player', { key: 'synthalpha', cursor: null }],
      ['set_party_mark', { key: 'synthalpha', name: 'SynthAlpha', mark: 'watched', note: 'Synthetic note' }],
      ['set_party_mark', { key: 'synthalpha', name: 'SynthAlpha', mark: null, note: null }],
      ['clear_party_history', { options: { marks_and_notes: false } }], ['clear_party_history', { options: { marks_and_notes: true } }],
      ['set_party_preset_pick', { presetId: 'synth-preset' }], ['set_party_preset_pick', { presetId: null }],
      ['open_realmeye', { name: 'SynthAlpha' }], ['update_party_settings', { settings }],
    ]);
  });

  it('passes arguments to overrides, clones replies and preserves rejected reads', async () => {
    const getPartyPlayers = vi.fn(async () => ready({ rows: [], next: null }));
    const setPartyMark = vi.fn(async () => {});
    const client = createClient(fixture({ getPartyLive: async () => ready(live), getPartyPlayers, setPartyMark,
      getPartyRun: async () => { throw new Error('Synthetic read failure'); } }));
    const reply = await client.getPartyLive();
    expect(reply).toEqual(ready(live));
    if (reply.status === 'ready') expect(reply.value).not.toBe(live);
    await client.getPartyPlayers('Synth', 'banned', 'deaths', null);
    expect(getPartyPlayers).toHaveBeenCalledExactlyOnceWith('Synth', 'banned', 'deaths', null);
    await client.setPartyMark('synthalpha', 'SynthAlpha', 'banned', null);
    expect(setPartyMark).toHaveBeenCalledExactlyOnceWith('synthalpha', 'SynthAlpha', 'banned', null);
    await expect(client.getPartyRun('synth-run')).rejects.toThrow('Synthetic read failure');
  });

  it('requires explicit Party fixtures instead of fabricating history or performing mutations', async () => {
    const client = createClient(fixture());
    for (const action of [() => client.getPartyLive(), () => client.getPartyRun('synth-run'),
      () => client.getPartyPlayers('', 'all', 'last_seen', null), () => client.getPartyPlayer('synthalpha', null),
      () => client.setPartyMark('synthalpha', 'SynthAlpha', null, null), () => client.clearPartyHistory({ marks_and_notes: false }),
      () => client.setPartyPresetPick(null), () => client.openRealmeye('SynthAlpha'), () => client.updatePartySettings(settings)]) {
      await expect(action()).rejects.toThrow('Preview Only');
    }
  });

  it('subscribes to party-changed independently of engine events and releases the listener', async () => {
    const transport = fixture();
    const client = createClient(transport);
    const receive = vi.fn();
    const stop = await client.onPartyChanged(receive);
    transport.publishEngineEvent(envelope({ run_ended: run() }));
    expect(receive).not.toHaveBeenCalled();
    transport.publishPartyChanged({ scope_id: 'synth-scope' });
    transport.publishPartyChanged({ scope_id: null });
    expect(receive.mock.calls).toEqual([[{ scope_id: 'synth-scope' }], [{ scope_id: null }]]);
    stop();
    transport.publishPartyChanged({ scope_id: 'synth-next' });
    expect(receive).toHaveBeenCalledTimes(2);
  });
});

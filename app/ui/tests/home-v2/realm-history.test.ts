import { render, within } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import RealmKills from '../../src/lib/home/v3/RealmKills.svelte';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import { spriteUrl } from '../../src/lib/bridge/visuals';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createHomeVisuals, emptyHomeVisuals, homeVisualTypes } from '../../src/lib/home/visuals';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { FightView } from '../../src/lib/bindings/FightView';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { realmView } from '../../src/lib/home/realm';
import { fight, flush, homeSnapshot, realmSnapshot } from './fixtures';

function harness() {
  let window = emptyHomeLiveWindow(), sequence = 12;
  const source = realmSnapshot();
  const live = createHomeLiveWindow(next => { window = next; });
  live.snapshot(source);
  const send = (event: EngineEvent, at = '9000000') => live.event({
    stream_id: source.stream_id, sequence: String(++sequence), at, event,
  });
  const enter = (at: string, kind: 'realm' | 'dungeon', realm_instance_key = 'realm-a') => {
    send({ map_entered: { at, name: kind, kind, realm_instance_key } }, at);
    send({ character_updated: source.character! }, at);
  };
  enter('6000000', 'realm');
  const kill = (id: string, overrides: Partial<FightView> = {}) => fight(id, {
    session_id: source.session!.id, run_id: null, started: '6100000', ended: '8100000',
    kill: { at: '8100000', evidence: 'announced_death_removed', provisional: false }, ...overrides,
  });
  const bag = (id: string) => ({ ...homeSnapshot().drops[0], id, session_id: source.session!.id, run_id: null, at: '8200000' });
  const announce = (id: string, object_type = 999) => send({ instance_event: {
    id, session_id: source.session!.id, run_id: null, at: '8100000', provisional: true,
    kind: { type: 'realm_event_killed', boss_key: 'Synthetic_Boss', object_type, killer: 'Synthetic_Killer' },
  } });
  const traffic = () => {
    for (let i = 0; i < 1536; i++) send({ fight_updated: kill(`ordinary-${i}`, {
      target_kind: 'ordinary', started: String(8500000 + i), ended: null, kill: null,
    }) });
    for (let i = 0; i < 320; i++) send({ instance_event: {
      id: `event-${i}`, session_id: source.session!.id, run_id: null, at: String(8600000 + i),
      provisional: true, kind: { type: 'player_left', object_id: i },
    } });
  };
  return { live, send, enter, kill, bag, announce, traffic, source,
    get window() { return window; }, view: () => realmView(source, window) };
}

it('keeps early kill evidence and merged announcements through busy traffic and a dungeon return', () => {
  const h = harness();
  h.send({ fight_ended: h.kill('early-a') });
  h.send({ fight_ended: h.kill('early-b', { target_type: 301, target_kind: 'quest',
    kill: { at: '8100001', evidence: 'hp_zero_removed', provisional: false } }) });
  h.announce('merged', 300); h.announce('oryx');
  const expected = h.view().kills;
  expect(expected).toHaveLength(3);
  h.traffic();
  expect(h.view().kills).toEqual(expected);
  expect(h.view().kills.find(row => row.fightId === 'early-a')).toMatchObject({
    at: '8100000', started: '6100000', ended: '8100000', evidence: 'announced_death_removed',
    provisional: false, bossKey: 'Synthetic_Boss', killer: 'Synthetic_Killer',
  });
  h.enter('10000000', 'dungeon'); h.enter('15000000', 'realm');
  expect(h.view().kills).toEqual(expected);
  h.enter('16000000', 'realm', 'realm-b');
  expect(h.view().kills).toEqual([]);
  h.live.dispose();
});

it('retains all 320 bags in the perf-live shaped visit and revisions to an early bag', () => {
  const h = harness();
  for (let i = 0; i < 320; i++) h.send({ bag_dropped: h.bag(`bag-${i}`) });
  h.traffic();
  expect(h.view().drops).toHaveLength(320);
  const revised = { ...h.bag('bag-0'), items: [1234] };
  h.send({ drop_updated: revised });
  expect(h.view().drops.find(row => row.id === 'bag-0')).toEqual(revised);
  h.enter('10000000', 'dungeon'); h.enter('15000000', 'realm');
  expect(h.view().drops).toHaveLength(320);
  h.enter('16000000', 'realm', 'realm-b');
  expect(h.view().drops).toEqual([]);
  h.live.dispose();
});

it.each(['kills', 'announcements', 'loot'] as const)('reports truncation of bounded visit %s', kind => {
  const h = harness(), cap = kind === 'loot' ? 512 : 500;
  for (let i = 0; i <= cap; i++) {
    if (kind === 'kills') h.send({ fight_ended: h.kill(`kill-${i}`) });
    if (kind === 'announcements') h.announce(`announcement-${i}`);
    if (kind === 'loot') h.send({ bag_dropped: h.bag(`bag-${i}`) });
  }
  const view = h.view();
  expect(kind === 'loot' ? view.drops.length : view.kills.length).toBe(cap);
  expect(view.partial).toBe(true);
  expect(view.reasons.join(' ')).toMatch(kind === 'loot' ? /loot history.*512/i
    : kind === 'kills' ? /kill history.*500/i : /announcement history.*500/i);
  h.enter('10000000', 'dungeon'); h.enter('15000000', 'realm');
  expect(h.view().reasons).toEqual(view.reasons);
  h.live.dispose();
});

it('bounds the perf-live shaped window without retaining kill charts or full player lists', () => {
  const h = harness();
  const template = h.kill('template');
  const players = Array.from({ length: 24 }, (_, i) => ({ ...template.players[0],
    player_id: `synthetic-${i}`, object_id: i, is_local: false }));
  for (let i = 0; i < 2; i++) h.send({ fight_ended: h.kill(`early-${i}`, {
    players, buckets: Array.from({ length: 600 }, () => template.buckets[0]),
  }) });
  for (let i = 0; i < 320; i++) h.send({ bag_dropped: h.bag(`bag-${i}`) });
  h.traffic();
  // Bound serialized size, not renderer private bytes; Windows owns that measurement.
  const bytes = new TextEncoder().encode(JSON.stringify(h.window)).length;
  expect(h.window.realmKills).toHaveLength(2);
  for (const row of h.window.realmKills!) {
    expect(row).not.toHaveProperty('buckets');
    expect(row).not.toHaveProperty('player_series');
    expect(row).not.toHaveProperty('players');
  }
  expect(h.window.fights.length).toBeLessThanOrEqual(64);
  expect(h.window.events.length).toBeLessThanOrEqual(256);
  expect(h.view().kills).toHaveLength(2);
  expect(h.window.drops).toHaveLength(320);
  expect(bytes).toBeLessThan(400_000);
  h.live.dispose();
});

it.each(['boss', 'event_boss', 'miniboss', 'quest'] as const)('retains %s local metrics and late revisions without accepting stale updates', target_kind => {
  const h = harness();
  const original = h.kill('revised', { target_kind });
  const local = { ...original.players[0], is_local: true, player_id: h.source.character!.id,
    damage: 123, dps: null, rank: 2, share: 0.25 };
  h.send({ fight_ended: { ...original, players: [local] } });
  h.traffic();
  expect(h.view().kills[0].self).toMatchObject({ damage: 123, dps: null, rank: 2, share: 0.25 });
  const revised = { ...original, players: [{ ...local, damage: 234, dps: 42 }],
    kill: { ...original.kill!, provisional: true } };
  h.send({ fight_updated: revised });
  h.live.event({ stream_id: h.source.stream_id, sequence: '15', at: '9000000', event: { fight_updated: original } });
  expect(h.view().kills).toHaveLength(1);
  expect(h.view().kills[0]).toMatchObject({ provisional: true, self: { damage: 234, dps: 42 } });
  h.live.snapshot({ ...h.source, sequence: '99999', at: '9000001', fights: [{ ...revised,
    kill: { ...revised.kill, provisional: false } }] });
  expect(h.view().kills[0]).toMatchObject({ provisional: false, self: { damage: 234, dps: 42 } });
  h.live.dispose();
});

it('keeps names and sprites for kills whose fights and announcements were evicted', async () => {
  const h = harness();
  h.send({ fight_ended: h.kill('focused') });
  h.send({ fight_ended: h.kill('early', { target_type: 301 }) });
  h.announce('early-announcement', 999);
  const sprite = { key: 'synthetic', sheet: 'characters.png', x: 0, y: 0, width: 8, height: 8 };
  const client = createClient(createFixtureTransport(h.source, {
    client_fingerprint: 'a'.repeat(64), objects: [301, 999].map(object_type => ({
      object_type, name: `Synthetic boss ${object_type}`, sprite, bag: null,
    })),
  }));
  let visuals = emptyHomeVisuals();
  const loader = createHomeVisuals(client, next => { visuals = next; });
  try {
    loader.update(homeVisualTypes(h.source, null, h.window)); await flush();
    expect(visuals.objects.map(row => row.object_type)).toEqual(expect.arrayContaining([301, 999]));
    h.traffic();
    // Force a new visual key/page, so the previous page cannot mask missing types.
    h.send({ fight_updated: h.kill('new-ordinary', { target_kind: 'ordinary',
      target_type: 302, started: '8900000', ended: null, kill: null }) });
    expect(h.window.fights.some(row => row.target_type === 301)).toBe(false);
    expect(h.window.events.some(row => row.kind.type === 'realm_event_killed')).toBe(false);
    const types = homeVisualTypes(h.source, null, h.window);
    expect(types).toEqual(expect.arrayContaining([301, 999]));
    loader.update(types); await flush();
    expect(visuals.phase).toBe('ready');
    const rows = h.view().kills.filter(row => row.targetType === 301 || row.targetType === 999);
    expect(rows).toHaveLength(2);
    const page = render(RealmKills, { rows, window: h.window, visuals,
      resolveSpriteUrl: (value: SpriteRef) => spriteUrl(visuals.client_fingerprint, value) });
    for (const row of rows) {
      const node = page.container.querySelector<HTMLElement>(`[data-kill-id="${row.id}"]`)!;
      expect(within(node).getByText(`Synthetic boss ${row.targetType}`)).toBeTruthy();
      expect(within(node).getByRole('img', { name: `Synthetic boss ${row.targetType}` })
        .getAttribute('data-fallback')).toBe('false');
      expect(node.querySelector('img')?.getAttribute('src'))
        .toBe(`http://maxed-sprite.localhost/${'a'.repeat(64)}/characters.png/0-0-8-8.png`);
    }
  } finally {
    loader.dispose(); h.live.dispose();
  }
});

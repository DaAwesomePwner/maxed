import type { ScopeLootItem } from '../../src/lib/bindings/ScopeLootItem';
import type { ScopeLootPage } from '../../src/lib/bindings/ScopeLootPage';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { ObjectVisual } from '../../src/lib/bindings/ObjectVisual';
import type { RunFightSummary } from '../../src/lib/bindings/RunFightSummary';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { VisitRecap } from '../../src/lib/bindings/VisitRecap';
import { createClient, type Transport } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot, homeSummary } from '../home/fixtures';

export type LegacyPreviewMode = 'dungeon' | 'realm' | 'nexus';
export type PreviewMode = LegacyPreviewMode | 'home-v4-dungeon' | 'home-v4-realm' | 'home-v4-nexus'
  | 'home-v4-recap' | 'home-v4-recap-died' | 'home-v4-recap-realm';
export function previewMode(value: string | null): PreviewMode {
  if (value === 'home-v4-dungeon' || value === 'home-v4-realm' || value === 'home-v4-nexus' || value === 'home-v4-recap') return value;
  if (value === 'home-v4-recap-died' || value === 'home-v4-recap-realm') return value;
  return value === 'realm' || value === 'nexus' ? value : 'dungeon';
}

// Unique build-audit marker: HOME_V2_SYNTHETIC_PREVIEW_ONLY.
const stream = 'HOME_V2_SYNTHETIC_PREVIEW_ONLY';
// Fixed synthetic evening in Chicago; preserve all relative encounter timings.
const evening = BigInt(Date.parse('2026-10-02T19:30:00-05:00')) * 1000n;
const at = (seconds: number) => String(evening + BigInt(Math.round(seconds * 1_000_000)));
const names = ['TestMage', 'TestKnight', 'TestArcher', 'DemoHero', 'DemoAlly', 'TestPriest', 'TestRogue'];

/** Invented DTOs only; no recording, native bridge, network or persistence. */
export function createHomePreview(scene: PreviewMode) {
  const redesign = scene.startsWith('home-v4-');
  const recapScene = scene.startsWith('home-v4-recap');
  const died = scene === 'home-v4-recap-died';
  const mode: LegacyPreviewMode = scene === 'home-v4-realm' || scene === 'home-v4-recap-realm' ? 'realm' : scene === 'home-v4-nexus' ? 'nexus'
    : redesign ? 'dungeon' : scene as LegacyPreviewMode;
  const parameters = new URLSearchParams(globalThis.location?.search ?? '');
  const variant = parameters.get('variant');
  const stressVariant = !redesign && variant === 'stress';
  const acceptanceVariant = !redesign && (stressVariant || variant === 'busy');
  const legacyFight = redesign && mode === 'realm' && variant === 'legacy';
  const busy = mode !== 'nexus' && acceptanceVariant;
  const churn = parameters.get('churn') === '1';

  const snapshot = homeSnapshot({ stream_id: stream, sequence: '0', at: at(540) });
  const character = snapshot.character!;
  character.name = names[0];
  character.fame = 1680;
  character.maxed = 4;
  character.observed_at = snapshot.at;
  character.equipment = [200, 201, 202, 203].map((item_type, slot) => ({
    ...character.equipment[0], slot, item_type,
  }));
  character.stat_panel = {
    breakdown_provisional: false,
    lines: [
      { stat_id: 0, current: 725, total: 800, boost: 80, at_cap: true },
      { stat_id: 3, current: 210, total: 385, boost: 35, at_cap: false },
      { stat_id: 20, total: 82, boost: 7, at_cap: true },
      { stat_id: 21, total: 38, boost: 13, at_cap: true },
      { stat_id: 22, total: 52, boost: 2, at_cap: true },
      { stat_id: 28, total: 48, boost: 3, at_cap: false },
      { stat_id: 26, total: 55, boost: 5, at_cap: false },
      { stat_id: 27, total: 42, boost: 2, at_cap: false },
    ],
    conditions: [{ name: 'Armored', bit: 25, secondary: false, confirmed: true }],
  };
  if (stressVariant) {
    character.name = 'TestLongHeroXX';
    character.fame = 125340;
    character.maxed = 4;
    character.equipment = character.equipment.map(item => ({ ...item,
      enchant: { state: 'recorded', slots: 4, applied: 4, ids: [1, 2, 3, 4] } }));
    character.stats = character.stats.filter(row => row.stat_id !== 58);
    character.stat_panel = {
      breakdown_provisional: false,
      conditions: [],
      lines: [
        { stat_id: 0, current: 1250, total: 1385, base: 1075, cap: 1210,
          boost: 310, at_cap: false, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
        { stat_id: 3, current: 52, total: 1105, base: 1105, cap: 1120,
          boost: 0, at_cap: false, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
        { stat_id: 20, total: 150, base: 45, cap: 70, boost: 105, at_cap: false, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
        { stat_id: 21, total: 125, base: 25, cap: 25, boost: 100, at_cap: true, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
        { stat_id: 22, total: 99, base: 123, cap: 123, boost: -24, at_cap: true, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
        { stat_id: 28, total: 110, base: 75, cap: 75, boost: 35, at_cap: true, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
        { stat_id: 26, total: 140, base: 40, cap: 80, boost: 100, at_cap: false, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
        { stat_id: 27, total: 135, base: 60, cap: 60, boost: 75, at_cap: true, exalt: 0,
          split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] } },
      ],
    };
  }
  const run = snapshot.run!;
  const seedFight = snapshot.fights[0];
  run.dungeon = 'Synthetic Moonlit Sanctum';
  run.started = at(300);
  run.party_size = names.length;
  run.party_size_estimated = true;
  snapshot.map = run.dungeon;
  snapshot.session!.started = at(0);
  snapshot.session!.last_traffic = snapshot.at;
  snapshot.instance = {
    context: { realm_name: 'Synthetic Willow Realm', instance_cap: 50, entered_from_realm: true, modifiers: [] },
    population: 5, population_peak: 7, population_provisional: true,
    realm_score: null, events_dropped: 0,
  };

  function fight(index: number, start: number, end: number | null, realm = false): FightView {
    if (legacyFight && end === null) end = 1390;
    const finish = end ?? (mode === 'realm' ? (busy ? 4660 : 1400) : 540);
    const players = names.map((name, i) => ({
      ...seedFight.players[0], object_id: i + 1,
      player_id: i === 0 ? character.id : `synthetic-player-${i}`, name,
      is_local: i === 0, damage: (7 - i) * 12_000, hits: (7 - i) * 24,
      biggest_hit: 780, deaths: i === 6 && index > 0 ? 1 : 0,
      share: (7 - i) / 28, rank: i + 1, dps: (7 - i) * 12_000 / (finish - start),
      sources: [{ ...seedFight.players[0].sources[0], damage: (7 - i) * 12_000,
        hits: (7 - i) * 24, biggest_hit: 780 }],
    }));
    if (redesign) {
      const count = recapScene ? 8 : realm ? 37 : 9, selfIndex = recapScene ? 1 : realm ? 8 : 1;
      const seed = players[0];
      players.splice(0, players.length, ...Array.from({ length: count }, (_, i) => {
        const damage = (count - i) * 12_000;
        return { ...seed, object_id: i + 1, player_id: i === selfIndex ? character.id : `synthetic-v4-player-${i}`,
          name: i === selfIndex ? names[0] : `TestPlayer${String(i + 1).padStart(2, '0')}`,
          is_local: i === selfIndex, rank: i + 1, damage, dps: damage / (finish - start),
          share: damage / (count * (count + 1) / 2 * 12_000 + 24_000), deaths: 0,
          sources: seed.sources.map(source => ({ ...source, damage })) };
      }));
    }
    const weights = [1, 2, 3, 2, 4, 5, 3, 4, 2, 2];
    const buckets = weights.map((weight, i) => ({
      from: at(start + (finish - start) * i / 10), until: at(start + (finish - start) * (i + 1) / 10),
      total: 12_000 * weight, local: 3_000 * weight,
    }));
    const total = players.reduce((sum, player) => sum + player.damage, 24_000);
    const local = players.find(player => player.is_local)!;
    const v4Buckets = buckets.map((bucket, i) => ({ ...bucket, total: total * weights[i] / 28,
      local: local.damage * weights[i] / 28,
      // Older saved fights deliberately omit both fields; every live bucket has them.
      ...(index === 0 || legacyFight ? {} : {
        others_damage: (total - 24_000 - local.damage) * weights[i] / 28,
        others_presence: (players.length - 1) * (finish - start) / 10,
      }),
    }));
    return { ...seedFight, id: `synthetic-fight-${realm ? 'realm-' : ''}${index}`,
      run_id: realm ? null : run.id, target_id: 30 + index, target_type: 300 + index, boss_type: 300 + index,
      started: at(start), ended: end === null ? null : at(end), first_hit: at(start), last_hit: at(finish),
      end_reason: end === null ? null : 'killed', players, total_damage: 586_000, unattributed_damage: 250_000,
      max_hp: end === null ? 500_000 : 336_000, target_hp: end === null ? 164_000 : 0,
      target_kind: realm ? 'event_boss' : 'boss', complete_capture: true, issues: [], buckets,
      kill: end === null ? null : { at: at(end), evidence: 'hp_zero_removed', provisional: false },
      ...(redesign ? { total_damage: total, unattributed_damage: 24_000, buckets: v4Buckets,
        local_last_hit: at(finish) } : {}),
      player_series: players.map(player => ({ player_id: player.player_id, object_id: player.object_id,
        is_local: player.is_local, points: weights.map(weight => player.damage * weight / 28) })),
    };
  }
  const fights = [fight(0, 310, 355), fight(1, 375, 430), fight(2, 455, null)];
  const drops = [6, 4, 2, 1, 0].map((rank, i) => ({
    ...snapshot.drops[0], id: `synthetic-bag-${i}`, at: at(360 + i * 35), bag_type: 400 + i,
    bag_rank: rank, source_enemy: 300 + (i % 2), source_estimated: false,
    items: [200 + (i % 4), 210 + i],
    contents: [200 + (i % 4), 210 + i].map((item_type, slot) => ({
      ...snapshot.drops[0].contents[0], slot, item_type, quantity: slot === 0 ? 1 : 2,
      taken_observed_at: i === 0 && slot === 0 ? at(365) : null,
    })),
  }));
  snapshot.run_totals = {
    fights: 3, kills: 2, first_hit: at(310), last_hit: snapshot.at,
    total_damage: 1_008_000, unattributed_damage: 0, fame_delta: 185, partial: false,
    players: fights[0].players.map(player => ({ ...player, damage: player.damage * 3, hits: player.hits * 3,
      dps: player.damage * 3 / 230, deaths: player.object_id === 7 ? 1 : 0 })),
  };
  snapshot.run_totals.local = snapshot.run_totals.players[0];
  if (mode === 'dungeon' && busy) {
    fights.splice(0, fights.length, ...Array.from({ length: 64 }, (_, i) =>
      fight(i % 3, 301 + i * 3, i === 63 ? null : 303 + i * 3))
      .map((row, i) => ({ ...row, id: `stress-fight-${i}` })));
    const seed = fights[63].players[0];
    fights[63].players = Array.from({ length: 85 }, (_, i) => ({ ...seed,
      player_id: i === 14 ? character.id : `stress-player-${i}`, object_id: 100 + i,
      name: i === 14 ? character.name : `LongPlayer${String(i).padStart(4, '0')}`,
      is_local: i === 14, rank: i + 1, share: (85 - i) / 3655,
      damage: (85 - i) * 10000, dps: (85 - i) * 200,
    }));
    fights[63].player_series = null;
    const seedDrops = [...drops];
    drops.splice(0, drops.length, ...Array.from({ length: 128 }, (_, i) => ({ ...seedDrops[i % seedDrops.length],
      id: `stress-bag-${i}`, at: at(360 + i),
    })));
    snapshot.instance.population = 85; snapshot.instance.population_peak = 85;
    snapshot.instance.context.instance_cap = 85;
    snapshot.run_totals = { ...snapshot.run_totals!, fights: 64, kills: 63,
      players: fights[63].players, local: fights[63].players[14] };
  }
  // Exercise existing pips; includes unknown and unenchanted items.
  if (mode !== 'nexus') drops.forEach((drop, i) => drop.contents.forEach((item, slot) => {
    const count = (i + slot) % 5;
    item.enchant = i === 2 ? { state: 'missing', slots: null, applied: null, ids: [] }
      : { state: count === 0 ? 'recorded_empty' : 'recorded', slots: count, applied: count, ids: Array.from({ length: count }, (_, index) => index + 1) };
  }));
  if (mode === 'dungeon' && busy) {
    fights.forEach(row => { row.total_damage = row.players.reduce((sum, player) => sum + player.damage, row.unattributed_damage); });
    snapshot.run_totals!.total_damage = snapshot.run_totals!.players.reduce((sum, row) => sum + row.damage,
      snapshot.run_totals!.unattributed_damage);
  }
  const events: EngineEvent[] = [];
  if (mode === 'dungeon') {
    events.push({ map_entered: { at: run.started, name: run.dungeon, kind: 'dungeon' } },
      { character_updated: character }, { run_started: run });
    for (const row of fights) events.push(row.ended ? { fight_ended: row } : { fight_updated: row });
    for (const row of drops) events.push({ bag_dropped: row });
    events.push({ instance_event: { id: 'synthetic-departure', session_id: run.session_id, run_id: run.id,
      at: at(490), kind: { type: 'player_left', object_id: 6, player_name: names[5], class_id: 100 }, provisional: true } },
    { instance_event: { id: 'synthetic-death', session_id: run.session_id, run_id: run.id, at: at(510),
      kind: { type: 'player_death', player_name: names[6], level: 20, killed_by: 'Moonlit Warden',
        object_id: 7, class_id: 100, is_local: false }, provisional: false } });
    snapshot.fights = fights;
    snapshot.drops = drops;
  } else {
    snapshot.area_kind = mode;
    snapshot.map = mode === 'realm' ? 'Synthetic Willow Realm' : 'Nexus';
    snapshot.run = null;
    snapshot.run_totals = null;
    snapshot.fights = [];
    snapshot.drops = [];
    if (mode === 'realm') {
      snapshot.at = at(busy ? 4660 : 1400);
      character.observed_at = snapshot.at;
      snapshot.session!.last_traffic = snapshot.at;
      snapshot.instance.context.instance_cap = 85;
      snapshot.instance.population = busy ? 85 : 64;
      snapshot.instance.population_peak = busy ? 85 : 71;
      snapshot.instance.realm_score = { current: busy ? 300000 : 186000, max: 300000 };
      snapshot.instance.visit_fame_delta = busy ? 1140 : 412;
      const killed = fight(3, 440, 660, true);
      const live = fight(4, busy ? 4218 : 1346, busy ? 4460 : null, true);
      live.target_hp = busy ? 0 : 2880000;
      live.max_hp = 6000000;
      if (!redesign) live.total_damage = busy ? 32400000 : 3120000;
      if (busy) {
        const seed = live.players[0];
        live.players = Array.from({ length: 85 }, (_, i) => ({ ...seed, object_id: 100 + i,
          player_id: i === 13 ? character.id : `realm-player-${i}`, name: i === 13 ? character.name : `LongPlayer${String(i).padStart(4, '0')}`,
          is_local: i === 13, rank: i + 1, share: (85 - i) / 3655,
          damage: Math.round((85 - i) / 3655 * live.total_damage),
          dps: Math.round((85 - i) / 3655 * live.total_damage / 242),
        }));
        live.unattributed_damage = 0;
        live.player_series = null;
      }
      const retained = busy ? Array.from({ length: 62 }, (_, i) => ({
        ...fight(3, 310 + i * 55, 330 + i * 55, true), id: `realm-retained-${i}`,
        kill: i < 4 ? { at: at(330 + i * 55), evidence: 'hp_zero_removed' as const, provisional: false } : null,
        end_reason: i < 4 ? 'killed' as const : 'map_changed' as const,
      })) : [];
      const visitFights = [killed, ...retained, live];
      const visitLoot = Array.from({ length: busy ? 128 : 5 }, (_, i) => ({
        ...drops[i % drops.length], id: `realm-bag-${i}`, run_id: null, at: at(660 + i * (busy ? 29 : 180)),
        source_enemy: 303,
      }));
      events.push({ map_entered: { at: at(300), name: snapshot.map, kind: 'realm' } }, { character_updated: character });
      for (const row of visitFights) events.push(row.ended ? { fight_ended: row } : { fight_updated: row });
      // The same-type notice deliberately exercises Task 7's merge. Typical also has a distinct announced-only kill.
      events.push({ instance_event: { id: 'synthetic-merged-announcement', session_id: run.session_id, run_id: null,
        at: killed.kill!.at, kind: { type: 'realm_event_killed', boss_key: 'Willow Behemoth', object_type: 303, killer: 'Torvik' }, provisional: true } });
      if (!busy) events.push({ instance_event: { id: 'synthetic-announcement', session_id: run.session_id, run_id: null,
        at: at(1140), kind: { type: 'realm_event_killed', boss_key: 'Lantern Colossus', object_type: 305, killer: 'Torvik' }, provisional: true } });
      for (const row of visitLoot) events.push({ bag_dropped: row });
      snapshot.fights = visitFights;
      snapshot.drops = visitLoot;
    } else snapshot.instance = null;
  }
  if (redesign && snapshot.instance) {
    const count = recapScene ? 8 : mode === 'realm' ? 37 : 9;
    snapshot.instance.population = count; snapshot.instance.population_peak = count;
    run.party_size = count;
    if (snapshot.run_totals) {
      snapshot.run_totals.local = snapshot.run_totals.players.find(player => player.is_local)!;
      snapshot.run_totals.unattributed_damage = 72_000;
      snapshot.run_totals.total_damage = snapshot.run_totals.players.reduce((sum, player) => sum + player.damage, 72_000);
    }
  }
  // Normalize stress only; reviewed busy fixtures keep their existing counts.
  if (stressVariant && mode !== 'nexus') {
    const realm = mode === 'realm';
    if (snapshot.fights.length !== 64 || snapshot.drops.length !== 128) {
      throw new Error('Task 6/9 busy builder must supply 64 fights and 128 bags');
    }
    const rows = structuredClone(snapshot.fights);
    let selected = rows.at(-1)!;
    if (realm) {
      selected = { ...fight(4, 4218, null, true), id: selected.id };
      selected.max_hp = 6000000;
      selected.target_hp = 2880000;
      rows[rows.length - 1] = selected;
      // Reopening the last fight removes its kill; retain six fought kills.
      if (!selected.kill) {
        const extra = rows.find(row => row.ended !== null && !row.kill)!;
        extra.kill = { at: extra.ended!, evidence: 'hp_zero_removed', provisional: false };
        extra.end_reason = 'killed';
      }
    }
    selected.target_type = realm ? 304 : 302;
    selected.boss_type = selected.target_type;
    selected.local_last_hit = selected.last_hit;
    selected.player_series = null;
    const seed = selected.players[0];
    if (!seed) throw new Error('Stress fight needs the existing player seed');
    const duration = Number(BigInt(selected.last_hit!) - BigInt(selected.started)) / 1_000_000;
    selected.players = Array.from({ length: 47 }, (_, index) => {
      const damage = (47 - index) * 19000;
      const source = structuredClone(seed.sources[0]);
      if (!source) throw new Error('Stress player needs the existing source seed');
      return { ...structuredClone(seed), object_id: 100 + index,
        player_id: index === 13 ? character.id : 'acceptance-player-' + index,
        name: index === 13 ? character.name : 'Player' + String(index + 1).padStart(2, '0'),
        is_local: index === 13, rank: index + 1, damage,
        dps: damage / duration, share: 0, hits: 1912, biggest_hit: 2480,
        sources: [{ ...source, damage, hits: 1912, biggest_hit: 2480 }],
      };
    });
    selected.total_damage = selected.players.reduce((sum, player) => sum + player.damage,
      selected.unattributed_damage);
    selected.players.forEach(player => { player.share = player.damage / selected.total_damage; });
    const weights = [1, 2, 3, 2, 4, 5, 3, 4, 2, 2];
    const start = Number(BigInt(selected.started) - evening) / 1_000_000;
    selected.buckets = weights.map((weight, index) => ({
      from: at(start + duration * index / 10), until: at(start + duration * (index + 1) / 10),
      total: selected.total_damage * weight / 28, local: selected.players[13].damage * weight / 28,
    }));
    snapshot.fights = rows;
    snapshot.instance!.population = 85;
    snapshot.instance!.population_peak = 85;
    snapshot.instance!.context.instance_cap = 85;
    snapshot.instance!.population_provisional = true;
    if (realm) {
      snapshot.instance!.realm_score = { current: 300000, max: 300000 };
      snapshot.instance!.visit_fame_delta = 1140;
    } else {
      run.party_size = 85;
      const totals = snapshot.run_totals!;
      // Whole-run values are independent supplied aggregates with the same rank window.
      totals.players = structuredClone(selected.players);
      totals.local = totals.players[13];
      totals.fights = 64; totals.kills = 63;
      totals.first_hit = rows[0].first_hit; totals.last_hit = at(540);
      totals.total_damage = totals.players.reduce((sum, player) => sum + player.damage, totals.unattributed_damage);
      totals.players.forEach(player => {
        player.share = player.damage / totals.total_damage;
        player.dps = player.damage / 239;
      });
    }
    snapshot.drops = structuredClone(snapshot.drops);
    // Preserve Task 9's merge example; add an explicitly unlinked announcement.
    const retained = events.filter(event =>
      !('fight_updated' in event) && !('fight_ended' in event) && !('bag_dropped' in event));
    events.splice(0, events.length, ...retained);
    if (realm) events.push({ instance_event: {
      id: 'acceptance-announcement', session_id: run.session_id, run_id: null, at: at(4500),
      kind: { type: 'realm_event_killed', boss_key: 'Lantern Colossus', object_type: 305,
        killer: 'Player47' }, provisional: true,
    } });
    for (const row of rows) events.push(row.ended ? { fight_ended: row } : { fight_updated: row });
    for (const row of snapshot.drops) events.push({ bag_dropped: row });
  }
  // Events precede the final snapshot watermark, in timestamp/sequence order.
  const eventAt = (event: EngineEvent): string => {
    if ('character_updated' in event) return at(300);
    if ('run_started' in event) return event.run_started.started;
    if ('fight_ended' in event) return event.fight_ended.ended!;
    if ('fight_updated' in event) return event.fight_updated.last_hit!;
    if ('map_entered' in event) return event.map_entered.at;
    if ('bag_dropped' in event) return event.bag_dropped.at;
    if ('instance_event' in event) return event.instance_event.at;
    throw new Error('Unsupported synthetic preview event');
  };
  events.sort((a, b) => Number(BigInt(eventAt(a)) - BigInt(eventAt(b))));
  const recentRuns = ['Moonlit Sanctum', 'Crystal Orchard', 'Velvet Citadel', 'Crystal Orchard'].map((name, i) => ({
    ...run, id: `synthetic-recent-${i}`, dungeon: `Synthetic ${name}`, started: at(230 - i * 65),
    ended: at(285 - i * 65), completed_at: at(280 - i * 65), outcome: 'cleared' as const,
    evidence: [{ at: at(280 - i * 65), evidence: 'victory' as const }],
  }));
  const recentDetails = recentRuns.map(row => ({ run_id: row.id, local_share: 0.25,
    share_partial: false, observed_drop_count: 2,
    loot: drops.slice(0, 2).map((drop, index) => ({ ...drop, id: `${row.id}-bag-${index}`, run_id: row.id,
      at: row.completed_at, contents: drop.contents.map(item => ({ ...item, taken_observed_at: null })),
    })), loot_preview_limited: false, issues: [],
  }));
  const summary = homeSummary({
    runs: 4, clears: 4, deaths: 0, fame_delta: 430, fame_missing_baselines: 0, fame_sampled: true,
    fame: [1250, 1320, 1475, 1520, 1680].map((fame, i) => ({ at: at(1 + i * 130), character_id: character.id, fame })),
    fame_characters: [{ character_id: character.id, first_fame: 1250, last_fame: 1680, last_at: at(521) }],
    best_drop: recentDetails[0].loot[0], top_fight: { ...fight(0, 240, 280), run_id: recentRuns[0].id },
    recent_runs: recentRuns, recent_details: recentDetails, unranked_drop_count: 0,
    dungeon_stats: ['Moonlit Sanctum', 'Crystal Orchard', 'Velvet Citadel'].map((name, i) => ({
      dungeon: `Synthetic ${name}`, runs: i === 1 ? 2 : 1, clears: i === 1 ? 2 : 1, deaths: 0,
      observed_clears: i === 1 ? 2 : 1, mean_clear_micros: 50_000_000, best_clear_micros: 50_000_000,
      last_started: at(230 - i * 65),
    })),
  });
  const objects: ObjectVisual[] = [
    { object_type: 1796, name: 'Realm Portal', sprite: null, bag: null, item_tier: null },
    { object_type: 100, name: 'Synthetic Knight', bag: null,
      sprite: { key: 'home-preview-character', sheet: 'home-preview.svg', x: 0, y: 0, width: 8, height: 8 } },
    ...['Moonbeam Blade', 'Petal Shield', 'Twilight Armor', 'Dewdrop Ring'].map((name, i) => ({
      object_type: 200 + i, name, sprite: null, bag: null, item_tier: i === 0 ? 'UT' : i === 1 ? 'ST' : `T${12 - i}`,
    })),
    ...['Moss Sentinel', 'Crystal Herald', 'Moonlit Warden', 'Willow Behemoth', 'Starfall Guardian', 'Lantern Colossus'].map((name, i) => ({
      object_type: 300 + i, name, sprite: null, bag: null,
    })),
    ...['White', 'Orange', 'Cyan', 'Blue', 'Purple'].map((color, i) => ({
      object_type: 400 + i, name: `${color} bag`, sprite: null,
      bag: { object_type: 400 + i, name: `${color} bag`, base_type: 400 + i, is_white: i === 0, rank: drops[i].bag_rank, capture: true },
    })),
    ...Array.from({ length: 5 }, (_, i) => ({ object_type: 210 + i, name: `Synthetic potion ${i + 1}`, sprite: null, bag: null })),
  ];
  const stress = mode === 'nexus' && acceptanceVariant;
  if (mode === 'nexus') {
    snapshot.map = 'Nexus';
    summary.runs = stress ? 9999 : 14; summary.clears = stress ? 8888 : 12; summary.deaths = stress ? 999 : 1;
    summary.fame_delta = stress ? 125340 : 3420;
    summary.fame = [1250, 1500, 2100, 2350, 3400, 4670].map((fame, i) => ({ at: at(i * 100), character_id: character.id, fame }));
    summary.fame_sampled = false; summary.fame_missing_baselines = 0;
    summary.top_fight!.players[0].dps = stress ? 999999 : 7900;
    summary.top_fight!.target_type = 302;
    const target = objects.find(object => object.object_type === 302)!;
    target.name = 'Forgotten King';
    const names = redesign ? ['Synthetic Moonlit Sanctum', 'Synthetic Crystal Orchard', 'Synthetic Moonlit Sanctum',
      'Synthetic Petal Grove', 'Synthetic Velvet Citadel', 'Synthetic Willow Den']
      : ['The Shatters', 'Lost Halls', 'The Shatters', 'Sprite World', 'Abyss of Demons', 'Snake Pit'];
    summary.recent_runs = names.map((dungeon, i) => ({ ...recentRuns[0], id: `nexus-run-${i}`, dungeon,
      started: at(60 + i * 90), ended: at(140 + i * 90), completed_at: i === 2 || i === 3 ? null : at(130 + i * 90),
      outcome: i === 2 ? 'died' : i === 3 ? 'left' : 'cleared', died_at: i === 2 ? at(140 + i * 90) : null,
      evidence: [],
    }));
    summary.recent_details = summary.recent_runs.map((run, i) => ({ ...recentDetails[0], run_id: run.id,
      local_share: i === 2 ? null : [0.26, 0.19, 0, 0.12, 0.31, 0.44][i],
      loot: [{ ...drops[i % drops.length], id: `nexus-drop-${i}`, run_id: run.id }],
    }));
    summary.dungeon_stats = ['The Shatters', 'Lost Halls', 'Abyss of Demons', 'Sprite World', "Oryx's Sanctuary", 'Snake Pit'].map((dungeon, i) => ({
      dungeon, runs: [5, 2, 3, 2, 1, 1][i], clears: i === 4 ? 0 : 1, deaths: i === 0 ? 1 : 0,
      observed_clears: i === 4 ? 0 : 1, mean_clear_micros: i === 4 ? null : [768, 1085, 400, 210, 0, 250][i] * 1_000_000,
      best_clear_micros: i === 4 ? null : [719, 1040, 362, 192, 0, 250][i] * 1_000_000,
      last_started: at(540), all_time_best_clear_micros: i === 4 ? null : [719, 1000, 362, 180, 0, 240][i] * 1_000_000,
    }));
    summary.best_drop = summary.recent_details[0].loot[0];
    summary.best_drop.contents[0].enchant = { state: 'recorded', slots: 4, applied: 4, ids: [1, 2, 3, 4] };
    const featured = objects.find(object => object.object_type === summary.best_drop!.contents[0].item_type)!;
    featured.name = stress ? 'A remarkably long name for the featured untiered crown' : 'The Forgotten Crown';
    featured.item_tier = 'UT';
    if (redesign) {
      target.name = 'Moonlit Warden'; featured.name = 'Synthetic Moonbeam Crown';
      summary.dungeon_stats.forEach((row, i) => { row.dungeon = names[i]; });
    }
  }
  const previewLoot: ScopeLootItem[] = [
    ...[200, 201, 202, 203].map((item_type, i) => ({ item_type, tier: i === 3 ? 'ST' : 'UT', count: 1,
      best_enchant: { state: 'recorded' as const, slots: [4, 1, 3, 2][i], applied: [4, 1, 3, 2][i], ids: Array.from({ length: [4, 1, 3, 2][i] }, (_, index) => index + 1) }, last_at: at(540) })),
    ...Array.from({ length: stress ? 253 : 8 }, (_, i) => ({ item_type: 210 + i, tier: null,
      count: stressVariant ? 1 + i % 5 : stress ? 1234 : [2, 1, 3, 4, 1, 2, 1, 5][i], best_enchant: null, last_at: at(540) })),
  ];
  if (mode === 'nexus') {
    for (const item of previewLoot) {
      const visual = objects.find(object => object.object_type === item.item_type);
      if (visual) visual.item_tier = item.tier;
    }
  }
  const queryLoot = async (_scope: import('../../src/lib/bindings/HomeScope').HomeScope, cursor: string | null, limit: number) => {
    if (mode !== 'nexus') return { status: 'ready' as const, value: { items: [], next: null, total_bags: 0 } };
    const offset = cursor === null ? 0 : Number(cursor);
    if (!Number.isSafeInteger(offset) || offset < 0 || limit < 1 || limit > 100) throw new Error('Invalid preview page');
    const page: ScopeLootPage = { items: previewLoot.slice(offset, offset + limit),
      next: offset + limit < previewLoot.length ? String(offset + limit) : null, total_bags: stressVariant ? 128 : stress ? 5000 : 14 };
    return { status: 'ready' as const, value: page };
  };

  if (stressVariant) {
    for (const type of [302, 304]) {
      const visual = objects.find(row => row.object_type === type);
      if (!visual) throw new Error('Missing existing synthetic boss visual ' + type);
      visual.name = 'Lord of the Lost Lands';
    }
    if (mode === 'nexus') {
      summary.fame_delta = 125340;
      // Retain Task 8's null/empty-table, partial, enchant and paging fixtures.
      // Six digits end the plotted series too; the rail has no class-quest goal.
      summary.fame = [121920, 122500, 123100, 124000, 124700, 125340].map((fame, i) => ({
        at: at(i * 100), character_id: character.id, fame,
      }));
      summary.fame_missing_baselines = 0;
      const local = summary.top_fight!.players.find(row => row.is_local);
      if (!local) throw new Error('Nexus top fight lost its local player');
      local.name = character.name;
      local.dps = 999999;
      summary.top_fight!.target_type = 302;
    }
  }

  // Saved replies use the same invented fights, identities and clock as the live scene.
  const savedFights = [fight(0, 310, 355), fight(1, 375, 430), fight(2, 455, 549)];
  const summarize = (row: FightView, ordinal: number): RunFightSummary => {
    const local = row.players.find(player => player.is_local)!;
    const others = row.players.filter(player => !player.is_local && player.dps !== null);
    return { id: row.id, target_kind: row.target_kind, target_type: row.target_type,
      target_name: objects.find(object => object.object_type === row.target_type)?.name ?? null,
      boss_type: row.boss_type, ordinal, started: row.started, ended: row.ended,
      first_hit: row.first_hit, last_hit: row.last_hit, total_damage: row.total_damage,
      local_damage: local.damage, local_dps: local.dps, local_rank: local.rank,
      party: { avg_dps: others.reduce((sum, player) => sum + player.dps!, 0) / others.length, players: others.length },
      player_count: row.players.length, partial: false, estimated: false };
  };
  const endedRun = { ...run, ended: at(550), completed_at: died ? null : at(549),
    died_at: died ? at(550) : null, outcome: died ? 'died' as const : 'cleared' as const,
    death: died ? { killed_by: 'synthetic-warden', killed_by_type: 302, fame: 1680,
      hp_trace: [545, 545.25, 545.5, 545.75, 546, 546.25, 548, 548.25, 548.5, 548.75, 549, 549.25]
        .map((seconds, i) => ({ at: at(seconds), hp: 725 - i * 60, max_hp: 800 })) } : null };
  const savedDrops = drops.map((drop, i) => ({ ...drop, bag_rank: i === 0 ? 255 : drop.bag_rank }));
  const story: RunStory = { run: endedRun, character, fights: savedFights.map(summarize),
    boss_fights: savedFights, boss_fights_limited: false, drops: savedDrops, timeline: [], comparison: null,
    totals: snapshot.run_totals ? { ...snapshot.run_totals, kills: 3 } : null, events: [], events_limited: false };
  const realmFights = Array.from({ length: 5 }, (_, i) => fight(i, 400 + i * 170, 460 + i * 170, true));
  const visit: VisitRecap = { visit_id: 'synthetic-visit', session_id: run.session_id, character_id: character.id,
    title: 'Synthetic Willow Realm', periods: [{ visit_id: 'synthetic-visit', started: at(300), ended: at(1401) }],
    fights: realmFights.map(summarize), boss_fights: realmFights, boss_fights_limited: false,
    announcements: [305, 306].map((object_type, i) => ({ id: `synthetic-announced-${i}`, session_id: run.session_id,
      run_id: null, at: at(1300 + i * 30), provisional: true,
      kind: { type: 'realm_event_killed', object_type, boss_key: `Synthetic event ${i + 1}`, killer: 'Synthetic Ally' } })),
    announcements_limited: false, drops: savedDrops.map(drop => ({ ...drop, run_id: null })), drops_limited: false,
    fame_delta: 412, best_dps: [], chain: { steps: [
      { visit_id: 'synthetic-visit', kind: 'realm', name: 'Synthetic Willow Realm', run_id: null,
        started: at(300), ended: at(1401), local_dps: 1400, party_dps: 1000, bags: 5, whites: 1, item_types: [200] },
      { visit_id: 'synthetic-castle-visit', kind: 'castle', name: 'Synthetic Castle', run_id: 'synthetic-castle-run',
        started: at(1401), ended: at(1500), local_dps: 1600, party_dps: 1100, bags: 0, whites: 0, item_types: [] },
    ] } };
  // The initial query establishes the sequence baseline before any event is sent.
  const baseline = { ...snapshot, at: at(299), fights: [], drops: [], run: null, run_totals: null };
  const fixture = createFixtureTransport(baseline, { client_fingerprint: 'a'.repeat(64), objects }, summary,
    recapScene ? {
      getRun: async id => ({ status: 'ready', value: id === run.id ? story : null }),
      listRuns: async () => ({ status: 'ready', value: { rows: [endedRun], next: null } }),
      getFight: async id => ({ status: 'ready', value: savedFights.find(row => row.id === id) ?? null }),
      getPlayerBreakdown: async (id, objectId) => ({ status: 'ready',
        value: savedFights.find(row => row.id === id)?.players.find(player => player.object_id === objectId) ?? null }),
    } : undefined, undefined, queryLoot,
    async () => ({ status: 'ready', value: (summary.dungeon_stats ?? []).flatMap(row =>
      row.all_time_best_clear_micros == null ? [] : [{ dungeon: row.dungeon, best_clear_micros: row.all_time_best_clear_micros }]) }),
    { requireHistoryFixtures: true, enchants: [
      { id: 1, name: 'Synthetic Edge I', description: 'Synthetic preview effect: adds imaginary damage to an invented target. This deliberately long description exercises wrapping and placement near window edges, in narrow rails, and over scrolling lists without describing any real game mechanic.' },
      { id: 2, name: 'Synthetic Guard II', description: 'Synthetic preview effect: grants imaginary protection.' },
      { id: 3, name: 'Synthetic Spark III', description: 'Synthetic preview effect: creates an invented spark.' },
      { id: 4, name: 'Synthetic Echo IV', description: 'Synthetic preview effect: repeats an imaginary pulse.' },
    ] },
    // The preview's Loot → Stats tab uses this; Home v4 reads homeDungeonStreak below.
    redesign ? { lootStats: () => ({ dungeons: [{ dungeon: run.dungeon, runs_ended: 46, clears: 40,
      bags: [], whites: 2, runs_since_last_white: 23, white_runs: 2, avg_runs_per_white: 23,
      longest_dry: 25, best: null }], sources: [], totals: [], whites: 2 }) } : undefined,
    undefined,
    redesign ? {
      runRecap: request => ({ run_id: request.run_id, clear_number: died ? null : 38, in_a_row: 3,
        chain: scene === 'home-v4-recap' ? { steps: [
          { visit_id: 'synthetic-realm-step', kind: 'realm', name: 'Synthetic Willow Realm', run_id: null,
            started: at(0), ended: at(150), local_dps: 1400, party_dps: 1000, bags: 2, whites: 0, item_types: [] },
          { visit_id: 'synthetic-castle-step', kind: 'castle', name: 'Synthetic Castle', run_id: 'synthetic-castle-run',
            started: at(150), ended: run.started, local_dps: 1600, party_dps: 1100, bags: 0, whites: 0, item_types: [] },
          { visit_id: 'synthetic-run-step', kind: 'sanctuary', name: run.dungeon, run_id: run.id,
            started: run.started, ended: endedRun.ended, local_dps: 1800, party_dps: 1200,
            bags: savedDrops.length, whites: 1, item_types: [200, 201, 202, 203] },
        ] } : null, pb_clear: died ? null : { clear_micros: 249_000_000, previous_best_micros: 260_000_000 },
        dry_streak_end: { runs: 23, had_previous_white: true },
        best_dps: [{ fight_id: savedFights[0].id, boss_type: 300,
          dps: savedFights[0].players.find(player => player.is_local)!.dps!, previous_best_dps: 1500 }], milestones: [] }),
      visitRecap: request => ({ ...visit, visit_id: request.visit_id,
        periods: [{ ...visit.periods[0], visit_id: request.visit_id }] }),
      homeDungeonStreak: request => ({ dungeon: request.dungeon, runs_ended: 46,
        runs_since_last_white: 23, avg_runs_per_white: 23, last_white_at: at(0) }),
      homeHud: () => ({ fame_per_hour: 1234, combat: { fights: 5, weighted_dps: 1380,
        weighted_party_dps: 1000, vs_party: 0.38, avg_rank: 2.6 }, whites: 2,
        ut_st: [200, 201, 202, 203], ut_st_count: 4,
        potions: [{ stat: 'life', count: 7 }, { stat: 'mana', count: 5 }, { stat: 'attack', count: 3 },
          { stat: 'speed', count: 2 }, { stat: 'wisdom', count: 1 }] }),
      dungeonProfile: request => ({ dungeon: request.dungeon, clears: 10, usual_run_dps: 1000,
        run_dps_samples: 5, usual_clear_micros: 260_000_000, clear_samples: 5,
        bosses: [300, 301, 302].map((target_type, i) => ({ target_type, target_name: null, seen_in: 8,
          median_offset_micros: (50 + i * 70) * 1_000_000, median_kill_micros: 60_000_000,
          kill_samples: 5, best_dps: 2400 })) }),
      homeBossBaselines: request => ({ rows: (request.target_types ?? [300, 301, 302, 303, 304]).map(target_type => ({
        target_type, fights: 5, avg_dps: 1000, best_dps: 2400,
        kill_samples: 5, median_kill_micros: 60_000_000 })), truncated: false }),
    } : undefined
  );
  let timer: ReturnType<typeof setTimeout> | undefined;
  let churnTimer: ReturnType<typeof setInterval> | undefined;
  let recapTimer: ReturnType<typeof setTimeout> | undefined;

  let started = false, disposed = false, sequence = 0;
  // Recap scenes publish one final run-ended or realm-exit event after the live batch.
  const finalSequence = events.length + 1;
  const transport: Transport = {
    ...fixture,
    async invoke(command, args) {
      if (command === 'get_dungeon_bests') return { status: 'ready', value: [
        { dungeon: run.dungeon, best_clear_micros: 230_000_000 },
      ] };

      if (command === 'get_store_status') return {
        state: 'ready', queued: 0, durable: String(recapScene ? finalSequence : sequence), stream_id: stream,
        message: 'Synthetic in-memory preview history',
      };
      const result = await fixture.invoke(command, args);
      if (command === 'get_live_snapshot' && !started && !disposed) {
        started = true;
        // Next task: subscribeLive has accepted the query's baseline and all listeners.
        timer = setTimeout(() => {
          if (disposed) return;
          for (const event of events) fixture.publishEngineEvent({ stream_id: stream, sequence: String(++sequence), at: eventAt(event), event });
          // Nexus also advances the watermark so the final numeric snapshot is accepted.
          if (events.length === 0) fixture.publishEngineEvent({ stream_id: stream, sequence: String(++sequence), at: snapshot.at,
            event: { character_updated: character } });
          fixture.publishSnapshot({ ...snapshot, sequence: String(sequence) });
          if (recapScene) {
            // Let Shell observe the live run before ending it; no injected recap state.
            recapTimer = setTimeout(() => {
              if (disposed) return;
              if (mode === 'realm') {
                fixture.publishEngineEvent({ stream_id: stream, sequence: String(++sequence), at: at(1401),
                  event: { map_entered: { at: at(1401), name: 'Nexus', kind: 'nexus' } } });
                fixture.publishSnapshot({ ...snapshot, sequence: String(sequence), at: at(1401),
                  area_kind: 'nexus', map: 'Nexus', run: null, fights: [], drops: [], instance: null });
              } else {
                fixture.publishEngineEvent({ stream_id: stream, sequence: String(++sequence), at: endedRun.ended,
                  event: { run_ended: endedRun } });
                fixture.publishSnapshot({ ...snapshot, sequence: String(sequence), at: endedRun.ended, run: endedRun });
              }
            }, 50);
          }
          if (churn && mode !== 'nexus') {
            let high = false, seconds = mode === 'realm' ? 10000 : 540;
            churnTimer = setInterval(() => {
              if (disposed) return;
              high = !high;
              const value = high ? 9999 : 99;
              const next = structuredClone(snapshot);
              next.at = at(++seconds);
              if (next.instance) next.instance.population = high ? 85 : 9;
              const live = next.fights.find(row => row.ended === null) ?? next.fights.at(-1);
              if (live) {
                live.target_hp = value; live.max_hp = 9999;
                if (live.ended === null) live.last_hit = next.at;
                live.players.forEach(row => { row.dps = value; row.damage = value; });
              }
              // Synthetic preview only: values alternate, but accepted timestamps keep increasing.
              fixture.publishSnapshot({ ...next, sequence: String(++sequence) });
            }, 250);
          }

        }, 0);
      }
      return result;
    },
  };
  return { client: createClient(transport), dispose() {
    disposed = true; clearTimeout(timer); clearInterval(churnTimer); clearTimeout(recapTimer);
  } };
}

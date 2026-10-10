import type { Client } from '../../src/lib/bridge/client';
import type { EnchantView } from '../../src/lib/bindings/EnchantView';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { PlayerDamage } from '../../src/lib/bindings/PlayerDamage';
import type { RunFightSummary } from '../../src/lib/bindings/RunFightSummary';
import type { RunRowDetail } from '../../src/lib/bindings/RunRowDetail';
import type { RunListSummary } from '../../src/lib/bindings/RunListSummary';
import type { RunPage } from '../../src/lib/bindings/RunPage';
import type { RunPlayerTotal } from '../../src/lib/bindings/RunPlayerTotal';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { RunView } from '../../src/lib/bindings/RunView';
import type { TimelineEntry } from '../../src/lib/bindings/TimelineEntry';
import { homeSnapshot } from '../home/fixtures';
import { createHomePreview } from './scenario';

// Runtime marker: production builds must eliminate this module and its fixtures.
const marker = 'RUNS_SYNTHETIC_PREVIEW_ONLY';
const names = ['TestMage', 'TestKnight', 'TestArcher', 'DemoHero', 'DemoAlly', 'TestPriest', 'TestRogue'];
const dungeons = ['The Shatters', 'Lost Halls', 'Abyss of Demons', 'Sprite World', 'The Shatters',
  'Snake Pit', 'Moonlight Village', 'Cultist Hideout', 'Tomb of the Ancients'];
// Default-variant edge cases appended after the nine standard runs (preview-run-9 onward).
const edgeDungeons = ['Synthetic Ordinary-Only Dungeon', 'Synthetic Unknown-Target Dungeon', 'Synthetic Empty Dungeon',
  'Synthetic Missing-Identity Dungeon', 'Synthetic Unknown-Bag Dungeon', 'Synthetic Clear-and-Death Dungeon'];
const edgeShapes = ['ordinary-only', 'unknown-targets', 'no-fights', 'no-identity', 'unknown-bag', 'clear-and-death'] as const;
type Shape = 'standard' | (typeof edgeShapes)[number];

function sourceEnchant(count: number | null): EnchantView {
  if (count === null) return { state: 'missing', slots: null, applied: null, ids: [] };
  return { state: count === 0 ? 'recorded_empty' : 'recorded', slots: count, applied: count,
    ids: Array.from({ length: count }, (_, index) => index + 1) };
}

/** Synthetic saved history only. The inherited Home transport owns the live rail and lifecycle. */
export function createRunsPreview(variant: 'default' | 'stress', now = new Date()): { client: Client; dispose(): void } {
  const preview = createHomePreview('nexus');
  const seed = homeSnapshot();
  const stress = variant === 'stress';
  const playerCount = stress ? 85 : 7;
  const bossCount = stress ? 64 : 4;
  const stories = new Map<string, RunStory>();
  const fights = new Map<string, FightView>();
  const shapeOf = (index: number): Shape => (stress ? undefined : edgeShapes[index - dungeons.length]) ?? 'standard';
  const clearSeconds = (index: number) => [552, 1040, 430, 210, 580, 280, 362, 768, 495][index % 9] + Math.floor(index / 9) * 17;
  const classId = (index: number) => 100 + index % 3;
  const characterName = (index: number) => names[index % 3] + (stress ? 'WithAnExceptionallyLongSyntheticName' : '');
  const playerWeights = (index: number) => Array.from({ length: playerCount }, (_, player) =>
    player === 0 ? playerCount * (.55 + index % 5 * .3) : playerCount - player);
  const dropCount = (index: number) => shapeOf(index) === 'no-fights' ? 0 : stress ? (index + 5) % 6
    : [5, 2, 1, 1, 4, 0, 3, 2, 1, 0, 3, 0, 2, 4, 5][index];
  const fameDelta = (index: number) => [412, 620, 0, 96, 405, 38, 288, 512, 364][index % 9] + Math.floor(index / 9) * 7;
  const runDungeons = stress ? Array.from({ length: 60 }, (_, index) => dungeons[index % dungeons.length]) : [...dungeons, ...edgeDungeons];
  const runs: RunView[] = runDungeons.map((dungeon, index) => {
    const group = Math.floor(index / 6), daysAgo = group < 2 ? group : group + 1;
    const started = BigInt(new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo, 19, 30 - index % 6 * 25).getTime()) * 1000n;
    const at = (seconds: number) => String(started + BigInt(Math.round(seconds * clearSeconds(index) / 580 * 1_000_000)));
    const shape = shapeOf(index);
    const outcome = shape !== 'standard' ? 'cleared' : index % 9 === 2 ? 'died' : index % 9 === 5 ? 'left' : index % 9 === 7 ? null : 'cleared';
    const completed = outcome === 'cleared' ? at(580) : null;
    const died = outcome === 'died' || shape === 'clear-and-death' ? at(590) : null;
    const ended = outcome === null ? null : at(600);
    return { ...seed.run!, id: `preview-run-${index}`, session_id: marker,
      character_id: shape === 'no-identity' ? null : index % 3 ? `${seed.run!.character_id}-${index % 3}` : seed.run!.character_id,
      dungeon: stress ? `${dungeon} · Synthetic exceptionally long dungeon name ${index % dungeons.length + 1}` : dungeon,
      started: String(started), ended, completed_at: completed, died_at: died, outcome,
      evidence: completed ? [{ at: completed, evidence: 'victory' as const },
        ...(died ? [{ at: died, evidence: 'death_packet' as const }] : [])]
        : died ? [{ at: died, evidence: 'death_packet' }]
        : ended ? [{ at: ended, evidence: 'observed_exit' }] : [],
      party_size: playerCount, party_size_estimated: true, complete_capture: index !== 8 && shape !== 'no-identity',
      issues: index === 8 ? ['stream_gap'] : shape === 'no-identity' ? ['missing_identity'] : [],
      context: { realm_name: 'Synthetic Medusa Realm', entered_from_realm: true, modifiers: [], instance_cap: 85 },
    };
  });

  // Keep the list lightweight even when every stress story contains 64 bosses and 85 players.
  // The active detail route owns one materialized story and its fights.
  function loadStory(runIndex: number): RunStory {
    const run = runs[runIndex];
    const cached = stories.get(run.id);
    if (cached) return cached;
    stories.clear(); fights.clear();
    const time = (seconds: number) => String(BigInt(run.started) + BigInt(Math.round(seconds * clearSeconds(runIndex) / 580 * 1_000_000)));
    const shape = shapeOf(runIndex);
    const partial = !run.complete_capture;
    const noIdentity = shape === 'no-identity';
    const playerName = (i: number) => i === 0 ? character.name
      : stress ? `DemoAllyWithAnExceptionallyLongSyntheticName${i}` : names[i];
    const character = { ...seed.character!, id: run.character_id ?? seed.character!.id, name: characterName(runIndex),
      class_id: classId(runIndex), observed_at: time(600), died_at: run.died_at };
    function fight(index: number, type: number | null, start: number, end: number, ordinary = false): FightView {
      const shares = playerWeights(runIndex), weightSum = shares.reduce((sum, weight) => sum + weight, 0);
      const players: PlayerDamage[] = Array.from({ length: playerCount }, (_, i) => {
        const weight = shares[i], damage = weight * (ordinary ? 300 : 12_000);
        const hits = Math.round(weight * 24);
        return { ...seed.fights[0].players[0], object_id: i + 1,
          player_id: i === 0 ? character.id : `preview-player-${i}`,
          name: playerName(i),
          class_id: i === 0 ? character.class_id : classId(i), is_local: i === 0 && !noIdentity, damage, hits, biggest_hit: ordinary ? 25 : 1200,
          deaths: 0, share: weight / weightSum, rank: 1 + shares.filter(other => other > weight).length, dps: damage / ((end - start) * clearSeconds(runIndex) / 580),
          sources: [
            { ...seed.fights[0].players[0].sources[0], item_type: 200, kind: 'weapon', ability: false,
              enchant: sourceEnchant(i === 0 ? 4 : i % 6 === 5 ? null : i % 6),
              damage: damage * .75, hits: Math.round(weight * 20), biggest_hit: ordinary ? 20 : 780 },
            { ...seed.fights[0].players[0].sources[0], item_type: 201, kind: 'ability', ability: true,
              enchant: sourceEnchant(i === 0 ? 0 : (i + 2) % 6 === 5 ? null : (i + 2) % 6),
              damage: damage * .25, hits: Math.round(weight * 4), biggest_hit: ordinary ? 25 : 1200 },
          ],
        };
      });
      const total = players.reduce((sum, player) => sum + player.damage, 0);
      const weights = [1, 2, 3, 2, 4, 5, 3, 4, 2, 2];
      const row: FightView = { ...seed.fights[0], id: `${run.id}-${ordinary ? 'other' : 'fight'}-${index}`,
        run_id: run.id, session_id: run.session_id, target_id: 1000 + index, target_type: type,
        boss_type: ordinary ? null : type, started: time(start), ended: time(end),
        first_hit: time(start), last_hit: time(end), local_last_hit: time(end), end_reason: 'killed', players,
        total_damage: total, unattributed_damage: 0, max_hp: total, target_hp: 0,
        target_kind: type === null ? null : ordinary ? 'ordinary' : 'boss', complete_capture: !partial, issues: [...run.issues],
        kill: { at: time(end), evidence: 'hp_zero_removed', provisional: partial },
        buckets: weights.map((weight, i) => ({ from: time(start + (end - start) * i / 10),
          until: time(start + (end - start) * (i + 1) / 10), total: total * weight / 28, local: noIdentity ? 0 : players[0].damage * weight / 28 })),
        player_series: players.map(player => ({ player_id: player.player_id, object_id: player.object_id,
          is_local: player.is_local, points: weights.map(weight => player.damage * weight / 28) })),
      };
      fights.set(row.id, row);
      return row;
    }
    const intervals = [[40, 95], [140, 210], [300, 420], [460, 560]];
    const standard = shape === 'standard';
    const bosses = Array.from({ length: standard ? bossCount : 0 }, (_, i) => fight(i, [300, 301, 302, 304][i % 4],
      stress ? 40 + i * 8 : intervals[i][0], stress ? 46 + i * 8 : intervals[i][1]));
    const otherCount = standard ? 9 : shape === 'ordinary-only' || shape === 'unknown-targets' ? 3 : shape === 'no-fights' ? 0 : 4;
    const others = Array.from({ length: otherCount }, (_, i) =>
      fight(i, shape === 'unknown-targets' ? null : 303, 100 + i * 30, 110 + i * 30, true));
    const all = [...bosses, ...others].sort((a, b) => Number(BigInt(a.started) - BigInt(b.started)));
    const summaries: RunFightSummary[] = all.map((row, i) => ({ id: row.id, target_kind: row.target_kind,
      target_type: row.target_type, target_name: null, boss_type: row.boss_type, ordinal: i + 1,
      started: row.started, ended: row.ended, first_hit: row.first_hit, last_hit: row.last_hit,
      total_damage: row.total_damage, local_damage: noIdentity ? null : row.players[0].damage,
      local_dps: noIdentity ? null : row.players[0].dps, local_rank: noIdentity ? null : row.players[0].rank, player_count: row.players.length, partial, estimated: false }));
    const ranks = [6, 4, 2, 1, 0, 3];
    const drops = Array.from({ length: dropCount(runIndex) }, (_, i) => ({ ...seed.drops[0],
      id: `${run.id}-bag-${i}`, session_id: run.session_id, run_id: run.id, at: time(stress ? 100 + i * 3 : 100 + i * 80),
      bag_type: 400 + (i % 5), bag_rank: ranks[i % 6], source_enemy: [300, 301, 302, 304][i % 4],
      source_estimated: false, contents_known: !(shape === 'unknown-bag' && i % 2 === 1),
      items: shape === 'unknown-bag' && i % 2 === 1 ? [] : [200 + (i % 4), 210 + (i % 6)],
      contents: (shape === 'unknown-bag' && i % 2 === 1 ? [] : [200 + (i % 4), 210 + (i % 6)]).map((item_type, slot) => ({ slot, item_type, quantity: 1,
        enchant: { state: 'recorded' as const, slots: 2, applied: 2, ids: [] }, taken_observed_at: null })),
    }));
    const total = all.reduce((sum, row) => sum + row.total_damage, 0);
    const players: RunPlayerTotal[] = (all[0]?.players ?? []).map((player, i) => {
      const damage = all.reduce((sum, row) => sum + row.players[i].damage, 0);
      return { player_id: player.player_id, name: player.name, class_id: player.class_id, is_local: player.is_local,
        damage, hits: all.reduce((sum, row) => sum + row.players[i].hits, 0), deaths: i === 0 && run.died_at ? 1 : 0,
        share: player.share, rank: player.rank, dps: damage / Number((BigInt(all.at(-1)!.last_hit!) - BigInt(all[0].first_hit!)) / 1_000_000n) };
    });
    const timeline: TimelineEntry[] = [
      { id: `${run.id}-entry`, at: run.started, kind: 'entered', related_id: run.id, explanation: '' },
      ...summaries.flatMap(row => [
        { id: `${row.id}-start`, at: row.started, kind: 'fight' as const, related_id: row.id, explanation: '' },
        { id: `${row.id}-kill`, at: row.ended!, kind: 'kill' as const, related_id: row.id, explanation: '' },
      ]),
      ...drops.map(drop => ({ id: `${drop.id}-drop`, at: drop.at, kind: 'drop' as const, related_id: drop.id, explanation: '' })),
      ...(all.length ? [{ id: `${run.id}-departure`, at: time(250), kind: 'player_left' as const, related_id: null,
        explanation: `${playerName(4)} left` }] : []),
    ];
    if (run.completed_at) timeline.push({ id: `${run.id}-clear`, at: run.completed_at, kind: 'completion', related_id: run.id, explanation: '' });
    if (run.died_at) timeline.push({ id: `${run.id}-death`, at: run.died_at, kind: 'death', related_id: run.id, explanation: `${character.name} died` });
    if (run.ended) timeline.push({ id: `${run.id}-exit`, at: run.ended, kind: 'exit', related_id: run.id, explanation: '' });
    timeline.sort((a, b) => Number(BigInt(a.at) - BigInt(b.at)));
    stories.set(run.id, { run, boss_fights: bosses, boss_fights_limited: false, events_limited: false,
      older_run_id: runs[runIndex + 1]?.id ?? null, newer_run_id: runs[runIndex - 1]?.id ?? null,
      events: [...(all.length ? [{ id: `${run.id}-departure`, session_id: run.session_id, run_id: run.id, at: time(250), provisional: false,
        kind: { type: 'player_left' as const, object_id: 5, player_name: playerName(4) } }] : []),
        ...(run.died_at ? [{ id: `${run.id}-death`, session_id: run.session_id, run_id: run.id, at: run.died_at, provisional: false,
          kind: { type: 'player_death' as const, player_name: character.name, is_local: true } }] : [])],
      character: noIdentity ? null : character, fights: summaries, drops, timeline,
      comparison: run.completed_at && !partial ? { previous_clears: 6, mean_micros: 640_000_000, change_fraction: (clearSeconds(runIndex) - 640) / 640 } : null,
      // The store reports no totals at all for a run without fights.
      totals: shape === 'no-fights' ? null : { fights: all.length, kills: all.length, first_hit: all[0]?.first_hit ?? null, last_hit: all.at(-1)?.last_hit ?? null,
        total_damage: total, unattributed_damage: 0, partial, fame_delta: partial ? null : fameDelta(runIndex), players, local: noIdentity ? null : players[0] ?? null },
    });
    return stories.get(run.id)!;
  }

  const details = new Map<string, RunRowDetail>();
  const clearMicros = (run: RunView) => run.completed_at === null ? null : Number(BigInt(run.completed_at) - BigInt(run.started));
  const chronological = (a: RunView, b: RunView) => Number(BigInt(a.started) - BigInt(b.started)) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const qualifyingClear = (run: RunView) => run.area_kind === 'dungeon' && run.outcome === 'cleared'
    && run.completed_at !== null && BigInt(run.completed_at) > BigInt(run.started)
    && run.evidence.some(({ evidence }) => evidence === 'victory' || typeof evidence === 'object' && 'completion_dialogue' in evidence);
  for (const [index, run] of runs.entries()) {
    const hasTotals = shapeOf(index) !== 'no-fights', hasIdentity = run.character_id !== null;
    const weights = playerWeights(index);
    const clears = runs.filter(candidate => candidate.dungeon === run.dungeon && qualifyingClear(candidate))
      .sort((a, b) => clearMicros(a)! - clearMicros(b)! || chronological(a, b));
    const best = clears[0];
    const previousBest = clears.find(candidate => chronological(candidate, run) < 0);
    const ranks = [6, 4, 2, 1, 0, 3];
    const loot = Array.from({ length: dropCount(index) }, (_, i) => ({ drop_id: `${run.id}-bag-${i}`, bag_type: 400 + i % 5, bag_rank: ranks[i % 6] }))
      .sort((a, b) => b.bag_rank - a.bag_rank);
    details.set(run.id, { run_id: run.id, character_name: hasIdentity ? characterName(index) : null, class_id: hasIdentity ? classId(index) : null,
      local_share: hasTotals && hasIdentity ? weights[0] / weights.reduce((sum, weight) => sum + weight, 0) : null, share_partial: !run.complete_capture || run.issues.length > 0,
      fame_delta: hasTotals && run.complete_capture ? fameDelta(index) : null, observed_drop_count: loot.length,
      loot_preview: loot.slice(0, 3),
      clear_micros: qualifyingClear(run) ? clearMicros(run) : null,
      best_clear_micros: best ? clearMicros(best) : null,
      previous_best_clear_micros: previousBest ? clearMicros(previousBest) : null, is_personal_best: best?.id === run.id });
  }
  // Preview replies are JSON DTOs, just like the native bridge. A JSON round-trip
  // keeps responses independent without structuredClone's cost on large boss histories.
  const ready = <T>(value: T): HistoryReply<T> => ({ status: 'ready', value: JSON.parse(JSON.stringify(value)) as T });
  const client = preview.client;
  const getVisuals = client.getObjectVisuals;
  // Home's Nexus loot fixture retags item 201 UT; Runs keeps the mockup's ST ability.
  client.getObjectVisuals = async types => {
    const page = await getVisuals([...new Set(types.map(type => type === 101 || type === 102 ? 100 : type))]);
    const character = page.objects.find(object => object.object_type === 100);
    return { ...page, objects: [
      ...page.objects.filter(object => types.includes(object.object_type))
        .map(row => row.object_type === 201 ? { ...row, item_tier: 'ST' } : row),
      ...types.filter(type => type === 101 || type === 102).flatMap(type => character
        ? [{ ...character, object_type: type, name: type === 101 ? 'Synthetic Knight' : 'Synthetic Archer' }] : []),
    ] };
  };
  client.listRuns = async filter => {
    const sortValue = (run: RunView) => filter.sort.startsWith('duration')
      ? (run.ended === null ? -1n : BigInt(run.ended) - BigInt(run.started)) : BigInt(run.started);
    const descending = filter.sort === 'newest' || filter.sort === 'duration_descending';
    const common = runs.filter(run => (!filter.dungeon || run.dungeon.toLowerCase().includes(filter.dungeon.toLowerCase()))
      && (!filter.range || (BigInt(run.started) >= BigInt(filter.range.start) && BigInt(run.started) < BigInt(filter.range.end))));
    const counted = common.filter(run => !filter.character_id || run.character_id === filter.character_id);
    const rows = counted.filter(run => !filter.outcome || run.outcome === filter.outcome)
      .sort((a, b) => (Number(sortValue(a) - sortValue(b)) || a.id.localeCompare(b.id)) * (descending ? -1 : 1));
    const characterCounts = new Map<string, RunListSummary['characters'][number]>();
    for (const run of common.filter(run => !filter.outcome || run.outcome === filter.outcome)) {
      if (!run.character_id) continue;
      const character = characterCounts.get(run.character_id) ?? { id: run.character_id,
        name: details.get(run.id)!.character_name, class_id: details.get(run.id)!.class_id, runs: 0 };
      character.runs++; characterCounts.set(run.character_id, character);
    }
    const fame = rows.map(run => details.get(run.id)!.fame_delta).filter((value): value is number => value !== null);
    const lastDeath = counted.filter(run => run.outcome === 'died' && run.died_at !== null)
      .sort((a, b) => Number(BigInt(b.died_at!) - BigInt(a.died_at!)))[0];
    const summary: RunListSummary | null = filter.cursor ? null : {
      runs: counted.length, cleared: counted.filter(run => run.outcome === 'cleared').length,
      died: counted.filter(run => run.outcome === 'died').length, left: counted.filter(run => run.outcome === 'left').length,
      unknown: counted.filter(run => run.outcome === 'unknown').length,
      time_in_runs_micros: rows.reduce((sum, run) => sum + (run.ended ?? run.completed_at ? Number(BigInt((run.ended ?? run.completed_at)!) - BigInt(run.started)) : 0), 0),
      personal_bests: rows.filter(run => details.get(run.id)!.is_personal_best).length,
      fame_delta: fame.length ? fame.reduce((sum, value) => sum + value, 0) : null, fame_observed_runs: fame.length,
      first_started: runs.at(-1)?.started ?? null, total_runs_all_time: runs.length,
      last_death: lastDeath ? { run_id: lastDeath.id, dungeon: lastDeath.dungeon, at: lastDeath.died_at! } : null,
      characters: [...characterCounts.values()].slice(0, 50),
    };
    const start = filter.cursor ? rows.findIndex(row => row.id === filter.cursor!.id) + 1 : 0;
    const pageRows = rows.slice(start, start + Math.max(1, Math.min(filter.limit, 100)));
    const last = pageRows.at(-1);
    const page: RunPage = { rows: pageRows, details: pageRows.map(run => details.get(run.id)!), summary, next: start + pageRows.length < rows.length && last
      ? { id: last.id, sort_value: String(sortValue(last)) } : null };
    return ready(page);
  };
  const getRun = (id: string) => {
    const index = runs.findIndex(run => run.id === id);
    return index < 0 ? null : loadStory(index);
  };
  const getFight = (id: string) => {
    const parent = /^(preview-run-\d+)-(?:fight|other)-\d+$/.exec(id)?.[1];
    if (parent) getRun(parent);
    return fights.get(id) ?? null;
  };
  client.getRun = async id => ready(getRun(id));
  client.getFight = async id => ready(getFight(id));
  client.getRunFightStrip = async (runId, playerId) => {
    getRun(runId);
    return ready([...fights.values()]
      .filter(row => row.run_id === runId && row.boss_type !== null)
      .sort((a, b) => Number(BigInt(a.started) - BigInt(b.started)))
      .map(row => {
        const matches = row.players.filter(player => playerId === null ? player.is_local : player.player_id === playerId);
        const self = matches.length === 1 ? matches[0] : null;
        return { fight_id: row.id, target_type: row.target_type, target_name: null, boss_type: row.boss_type,
          started: row.started, ended: row.ended, killed: !!row.kill, player_count: row.players.length,
          damage: self?.damage ?? null, rank: self?.rank ?? null, share: self?.share ?? null };
      }));
  };
  client.getPlayerBreakdown = async (id, objectId) => ready(getFight(id)?.players.find(player => player.object_id === objectId) ?? null);
  return preview;
}

import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport, type PartyFixtureQueries } from '../../src/lib/bridge/fixture';
import type { PartyMember } from '../../src/lib/bindings/PartyMember';
import type { PartyView } from '../../src/lib/bindings/PartyView';
import type { PartyPlayerView } from '../../src/lib/bindings/PartyPlayerView';
import type { PartySettings } from '../../src/lib/bindings/PartySettings';
import type { RequirementPreset } from '../../src/lib/bindings/RequirementPreset';
import type { ParseOutcome } from '../../src/lib/bindings/ParseOutcome';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { VisualPage } from '../../src/lib/bindings/VisualPage';
import { homeSnapshot } from '../home/fixtures';

// Runtime marker: this entire module must be absent from the production bundle.
const marker = 'PARTY_SYNTHETIC_PREVIEW_ONLY';
const map = 'Synthetic Sanctum';
const names = ['SynthAlpha', 'SynthBravo', 'SynthCheck', 'SynthUnseen', 'SynthAway', 'SynthLeft',
  'SynthQuiet', 'SynthTrash', 'SynthWatch', 'SynthBan', null, 'SynthLocal'];
const at = (minutes: number) => String(BigInt(Date.parse('2026-10-11T12:00:00-05:00')) * 1000n + BigInt(minutes) * 60_000_000n);
const ready = <T>(value: T): HistoryReply<T> => ({ status: 'ready', value: structuredClone(value) });

/** Synthetic DTOs only. Writes stay in this instance; no native bridge or browser handoff. */
export function createPartyPreview(variant: 'default' | 'stress') {
  let settings: PartySettings = { default_preset_id: 'synth-standard', presets: [
    { id: 'synth-standard', name: 'Synthetic standard', min_maxed: 6, min_tier: { weapon: 11, ability: 5, armor: 12, ring: 4 }, ut_st_meets: true, auto_apply: [map] },
    { id: 'synth-practice', name: 'Synthetic practice', min_maxed: 4, min_tier: { weapon: 9 }, ut_st_meets: true, auto_apply: ['Synthetic Garden'] },
  ] };
  let picked: string | null = null;
  const members: PartyMember[] = Array.from({ length: variant === 'stress' ? 85 : names.length }, (_, index) => {
    const name = index < names.length ? names[index] : `SynthGuest${String.fromCharCode(65 + Math.floor((index - 12) / 26), 65 + (index - 12) % 26)}`;
    const unseen = index === 3 || index === 10;
    return { player_id: `synth-player-${index}`, name, name_key: name?.toLowerCase() ?? null,
      class_id: 100 + index % 3, level: unseen ? null : 20, stars: unseen ? null : 45,
      guild: name ? 'Synthetic Guild' : null, maxed: unseen ? null : index === 1 ? 5 : 8,
      equipment: unseen ? [] : [index === 1 ? 204 : 200, 201, 202, index === 2 ? 205 : 203].map((item_type, slot) => ({
        slot, item_type, enchant: { state: 'recorded_empty', slots: 0, applied: 0, ids: [] },
      })),
      is_local: index === 11, present: index !== 4 && index !== 5,
      out_of_view_since: index === 4 ? at(8) : null, departed_at: index === 5 ? at(9) : null,
      damage: index >= 64 ? null : index === 6 ? 0 : [1, 7].includes(index) ? 30000 : 10000 + index * 250,
      share: index >= 64 ? null : index === 6 ? 0 : .02, boss_share: index >= 64 ? null : [1, 6, 7].includes(index) ? 0 : .025,
      flags: index === 6 ? ['no_damage_seen'] : [1, 7].includes(index) ? ['mostly_trash'] : [],
      mark: index === 8 ? 'watched' : index === 9 ? 'banned' : null, runs_together: unseen || index === 11 ? 0 : 3,
    };
  });
  // Normalize represented shares while retaining explicit unknown totals beyond 64 players.
  const damageTotal = members.reduce((sum, member) => sum + (member.damage ?? 0), 0);
  const bossTotal = members.reduce((sum, member) => sum + (member.boss_share ? member.damage ?? 0 : 0), 0);
  for (const member of members) {
    if (member.damage != null) {
      member.share = member.damage / damageTotal;
      member.boss_share = member.boss_share ? member.damage / bossTotal : 0;
    }
  }
  const visuals: VisualPage = { client_fingerprint: marker, objects: [
    ...['Synthetic Knight', 'Synthetic Wizard', 'Synthetic Priest'].map((name, index) => ({ object_type: 100 + index, name, sprite: null, bag: null })),
    ...['12', '6', '13', '5', '9', null].map((item_tier, index) => ({ object_type: 200 + index,
      name: ['Synthetic sword', 'Synthetic ability', 'Synthetic armor', 'Synthetic ring', 'Synthetic starter sword', 'Synthetic unknown ring'][index],
      item_tier, sprite: null, bag: null })),
  ] };
  const snapshot = homeSnapshot({ stream_id: marker, sequence: '0', at: at(10), map, fights: [], drops: [], run_totals: null, instance: null });
  snapshot.character = { ...snapshot.character!, id: 'synth-local', name: 'SynthLocal', class_id: members[11].class_id!,
    maxed: 8, level: 20, equipment: structuredClone(members[11].equipment), observed_at: at(10) };
  snapshot.run = { ...snapshot.run!, id: 'synth-party-live', dungeon: map, session_id: marker, character_id: 'synth-local', started: at(0), ended: null };
  const runs = Array.from({ length: 3 }, (_, index) => ({ ...snapshot.run!, id: index ? `synth-party-run-${index}` : 'synth-party-run',
    started: at(-60 * (index + 1)), ended: at(-60 * (index + 1) + 12), completed_at: at(-60 * (index + 1) + 10),
    outcome: 'cleared' as const, party_size: members.length }));
  const history = new Map<string, PartyPlayerView>();
  for (const member of members) {
    if (!member.name_key || !member.name || member.is_local) continue;
    history.set(member.name_key, {
      row: { key: member.name_key, name: member.name, last_class_id: member.class_id, last_maxed: member.maxed,
        runs_together: 3, deaths: 1, left_early: member.departed_at ? 1 : 0, avg_share: member.share,
        last_seen: at(-48), last_map: map, mark: member.mark, note: member.mark ? 'Synthetic private note' : null },
      first_seen: at(-180), guild: member.guild,
      last_profile: { scope_id: runs[0].id, run_id: runs[0].id, player_id: member.player_id, object_id: members.indexOf(member),
        is_local: false, name: member.name, class_id: member.class_id, level: member.level, stars: member.stars,
        guild: member.guild, maxed: member.maxed, maxed_stats: [], stats: [], equipment: structuredClone(member.equipment),
        first_seen: runs[0].started, last_seen: runs[0].ended! },
      runs: runs.map((run, index) => ({ run_id: run.id, dungeon: run.dungeon, started: run.started, outcome: run.outcome,
        share: member.share, died: index === 2, left_early: !!member.departed_at && index === 0 })), next: null,
    });
  }
  function resolved(saved: boolean) {
    const explicit = !saved && settings.presets.find(preset => preset.id === picked);
    const auto = settings.presets.find(preset => preset.auto_apply.includes(map));
    const preset = explicit || auto || settings.presets.find(preset => preset.id === settings.default_preset_id);
    return { preset, source: explicit ? 'picked' as const : auto ? 'auto' as const : 'default' as const };
  }
  // Evaluate only the invented catalog's numeric tiers; native rule tests own game semantics.
  function parse(member: PartyMember, preset: RequirementPreset): ParseOutcome {
    if (member.level == null && member.maxed == null) return { result: 'not_seen', reasons: [] };
    const reasons: ParseOutcome['reasons'] = [];
    if (preset.min_maxed != null && member.maxed != null && member.maxed < preset.min_maxed)
      reasons.push({ kind: 'below_maxed', text: `${member.maxed}/8 < ${preset.min_maxed}/8` });
    for (const [index, slot] of (['weapon', 'ability', 'armor', 'ring'] as const).entries()) {
      const min = preset.min_tier[slot];
      if (min == null) continue;
      const item = member.equipment.find(item => item.slot === index);
      const tier = visuals.objects.find(object => object.object_type === item?.item_type)?.item_tier;
      if (item?.item_type == null) reasons.push({ slot, kind: 'slot_empty', text: `no ${slot}` });
      else if (tier == null) reasons.push({ slot, kind: 'no_tier_data', text: `${slot}: no tier data` });
      else if (Number(tier) < min) reasons.push({ slot, kind: 'below_tier', text: `${slot} T${tier} < T${min}` });
    }
    return { result: reasons.some(reason => reason.kind !== 'no_tier_data') ? 'fail' : reasons.length ? 'check' : 'pass', reasons };
  }
  function view(runId?: string): PartyView {
    const saved = runId !== undefined, { preset, source } = resolved(saved);
    const rows = members.map(member => ({ ...structuredClone(member),
      present: saved ? null : member.present, out_of_view_since: saved ? null : member.out_of_view_since,
      departed_at: saved && member.departed_at ? at(-51) : member.departed_at,
      flags: saved && member.departed_at ? [...member.flags, 'left_early' as const] : [...member.flags],
      mark: member.name_key ? history.get(member.name_key)?.row.mark ?? null : null,
      runs_together: member.name_key ? history.get(member.name_key)?.row.runs_together ?? 0 : 0,
      parse: preset ? parse(member, preset) : null,
    }));
    return { scope_id: runId ?? snapshot.run!.id, run_id: runId ?? snapshot.run!.id, map, area_kind: 'dungeon', live: !saved,
      preset: preset ? { id: preset.id, name: preset.name, source } : null, assets_available: true, gear_recorded: true,
      members: rows, counts: { in_view: rows.filter(row => row.present).length,
        pass: rows.filter(row => row.parse?.result === 'pass').length, fail: rows.filter(row => row.parse?.result === 'fail').length,
        check: rows.filter(row => row.parse?.result === 'check').length, not_seen: rows.filter(row => row.parse?.result === 'not_seen').length,
        no_damage_seen: rows.filter(row => row.flags.includes('no_damage_seen')).length,
        left_early: rows.filter(row => row.flags.includes('left_early')).length },
      classes: [100, 101, 102].map(class_id => ({ class_id, count: rows.filter(row => row.class_id === class_id).length })),
    };
  }
  const party: PartyFixtureQueries = {
    getPartyLive: async () => ready(view()),
    getPartyRun: async id => ready(runs.some(run => run.id === id) ? view(id) : null),
    getPartyPlayers: async (query, filter, sort, cursor) => {
      const rows = [...history.values()].map(detail => detail.row).filter(row => row.name.toLowerCase().includes(query.toLowerCase())
        && (filter === 'all' || (filter === 'with_notes' ? !!row.note : row.mark === filter)));
      rows.sort((a, b) => Number(b[sort] ?? -1) - Number(a[sort] ?? -1) || a.key.localeCompare(b.key));
      const start = cursor ? rows.findIndex(row => row.key === cursor.key) + 1 : 0;
      const page = rows.slice(start, start + 100);
      return ready({ rows: page, next: start + page.length < rows.length ? { key: page.at(-1)!.key, sort_value: '0' } : null });
    },
    getPartyPlayer: async key => ready(history.get(key) ?? null),
    setPartyMark: async (key, name, mark, note) => {
      let detail = history.get(key);
      if (!detail) {
        detail = { row: { key, name, runs_together: 0, deaths: 0, left_early: 0, last_seen: at(10) }, first_seen: at(10), runs: [] };
        history.set(key, detail);
      }
      detail.row.mark = mark; detail.row.note = note;
    },
    clearPartyHistory: async ({ marks_and_notes }) => {
      for (const [key, detail] of history) {
        if (marks_and_notes || (!detail.row.mark && !detail.row.note)) history.delete(key);
        else { detail.runs = []; detail.last_profile = null; detail.row.runs_together = 0; detail.row.deaths = 0;
          detail.row.left_early = 0; detail.row.avg_share = null; }
      }
    },
    setPartyPresetPick: async id => { picked = id; },
    updatePartySettings: async next => { settings = structuredClone(next); },
    openRealmeye: async () => { throw new Error('Preview only: no browser handoff.'); },
  };
  const transport = createFixtureTransport(snapshot, visuals, undefined, {
    listRuns: async () => ready({ rows: runs, next: null }), getRun: async () => ready(null),
    getFight: async () => ready(null), getPlayerBreakdown: async () => ready(null),
  }, undefined, undefined, undefined, { catalogDungeons: [map, 'Synthetic Garden'] }, undefined, undefined, undefined, party);
  const client = createClient(transport), querySettings = client.querySettings;
  client.querySettings = async () => ({ ...await querySettings(), party: structuredClone(settings) });
  client.queryStore = async () => ({ state: 'ready', queued: 0, durable: '0', stream_id: marker, message: '' });
  return { client };
}

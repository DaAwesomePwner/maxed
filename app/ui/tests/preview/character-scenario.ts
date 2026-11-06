import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { CharacterCalculation } from '../../src/lib/bindings/CharacterCalculation';
import type { CharacterCalculationRequest } from '../../src/lib/bindings/CharacterCalculationRequest';
import type { CharacterValue } from '../../src/lib/bindings/CharacterValue';
import type { CharacterRange } from '../../src/lib/bindings/CharacterRange';
import type { CharacterWeaponCalculation } from '../../src/lib/bindings/CharacterWeaponCalculation';
import { homeSnapshot } from '../home/fixtures';

export const characterVariants = ['normal', 'empty', 'partial', 'stale', 'unknown-weapon', 'missing-catalog'] as const;
export type CharacterVariant = typeof characterVariants[number];
export function characterVariant(value: string | null): CharacterVariant {
  return characterVariants.find(variant => variant === value) ?? 'normal';
}
const marker = 'CHARACTER_SYNTHETIC_PREVIEW_ONLY';
const at = '1792231200000000';
const value = (n: number | null, reason = 'fixture result unavailable'): CharacterValue => ({ value: n,
  evidence: 'modeled', completeness: n === null ? 'unavailable' : 'complete',
  reasons: n === null ? [reason] : [], exclusions: [] });
const range = (n: number | null, reason: string): CharacterRange => ({ min: n, max: n,
  evidence: 'catalog', completeness: n === null ? 'unavailable' : 'complete',
  reasons: n === null ? [reason] : [], exclusions: [] });
// Authored Task 5 vectors: ATT/DEX 75, fixed damage 100, mask 0, one projectile, rate 1.
// No arithmetic engine: unlisted defenses explicitly have no authored result.
const vectors: Record<number, { hit: number; dps: number }> = {
  0: { hit: 200, dps: 1600 }, 50: { hit: 150, dps: 1200 },
};
function weapon(item: number, defense: number, variant: CharacterVariant): CharacterWeaponCalculation {
  const reason = variant === 'missing-catalog' ? 'catalog unavailable' : item === 444 ? 'item metadata missing'
    : 'no authored preview result for this defense; use 0 or 50';
  const vector = variant !== 'missing-catalog' && item !== 444 ? vectors[defense] : undefined;
  const group = { group_id: 'normal', projectile_index: 0, armor_piercing: false, count: 1,
    rate_factor: value(1), catalog_range: range(vector ? 100 : null, reason),
    hit_range: { ...range(vector?.hit ?? null, reason), evidence: 'modeled' as const },
    hit_mean: value(vector?.hit ?? null, reason), attacks_per_sec: value(vector ? 8 : null, reason),
    projectiles_per_sec: value(vector ? 8 : null, reason), dps: value(vector?.dps ?? null, reason) };
  const partialReason = 'unsupported schedule: synthetic conditional volley remains unavailable until its '
    + 'timing and projectile coverage are established; supported normal fire is only a subtotal';
  const groups = item === 444 || variant === 'missing-catalog' ? [] : [group];
  if (variant === 'partial') groups.push({ ...group, group_id: 'unsupported conditional volley',
    attacks_per_sec: value(null, partialReason), projectiles_per_sec: value(null, partialReason),
    dps: value(null, partialReason) });
  return { item_type: item, applied_att: 75, applied_dex: 75, applied_primary_mask: 0,
    groups, rate_factor: group.rate_factor, catalog_range: group.catalog_range, hit_range: group.hit_range,
    hit_mean: group.hit_mean, attacks_per_sec: group.attacks_per_sec, projectiles_per_sec: group.projectiles_per_sec,
    dps_subtotal: value(vector?.dps ?? null, reason),
    dps_total: variant === 'partial' ? value(null, partialReason) : value(vector?.dps ?? null, reason) };
}

/** All identities, observations and results are invented; no native transport or persisted controls. */
export function createCharacterPreview(variant: CharacterVariant = 'normal') {
  const snapshot = homeSnapshot({ stream_id: marker, at, sequence: '12', map: 'Synthetic Sanctuary',
    area_kind: 'nexus', fights: [], drops: [], run: null, run_totals: null, instance: null });
  snapshot.character = { ...snapshot.character!, id: 'synthetic-character', name: 'SynthAster',
    observed_at: at, class_id: 100, maxed: null, identity_verified: variant !== 'partial',
    issues: variant === 'partial' ? ['missing_identity', 'missing_stats'] : [],
    calculation_revision: { stream_id: marker, character_id: 'synthetic-character',
      input_hash: 'synthetic-input', catalog_fingerprint: variant === 'missing-catalog' ? null : marker },
    equipment: [0, 1, 2, 3].map(slot => ({ slot, item_type: slot === 0 ? variant === 'unknown-weapon' ? 444 : 200 : null,
      observed_empty: slot !== 0, enchant: slot === 0 ? { state: 'recorded', slots: 1, applied: 1, ids: [900] }
        : { state: 'recorded_empty', slots: 0, applied: 0, ids: [] } })),
    stats: [{ stat_id: 20, value: 75 }, { stat_id: 28, value: 75 }, { stat_id: 27, value: 50 },
      { stat_id: 29, value: 0 }, { stat_id: 3, value: 300 },
      ...(variant === 'partial' ? [] : [{ stat_id: 0, value: 800 }])],
    stat_panel: { breakdown_provisional: true, conditions: [], lines: [0, 3, 20, 21, 22, 28, 26, 27].map(stat_id => ({
      stat_id, total: stat_id === 0 ? variant === 'partial' ? null : 800 : stat_id === 3 ? 300 : stat_id === 27 ? 50 : 75,
      current: stat_id === 0 ? 600 : stat_id === 3 ? 250 : null,
      base: variant === 'partial' ? null : stat_id === 0 ? 795 : stat_id === 3 ? 295 : stat_id === 27 ? 45 : 70,
      boost: 5, gear: variant === 'partial' ? 10 : 5,
      exalt: 0, other: variant === 'partial' ? -5 : 0,
      cap: stat_id === 0 ? 800 : stat_id === 3 ? 300 : stat_id === 27 ? 50 : 75, at_cap: variant === 'partial' ? null : false,
      split_evidence: { placement: 'provisional_inside_boost', cap_eligible: variant !== 'partial',
        reasons: variant === 'partial' ? ['negative_other'] : [] },
    })) } };
  if (variant === 'empty') snapshot.character = null;
  let stale = variant === 'stale';
  function reply(request: CharacterCalculationRequest): CharacterCalculation {
    const missing = variant === 'missing-catalog';
    const partial = variant === 'partial';
    const hp = missing ? value(null, 'enchant metadata missing') : partial ? value(null, 'maximum not observed') : value(10);
    return { revision: structuredClone(request.expected_revision), observed_at: at,
      catalog_fingerprint: request.expected_revision.catalog_fingerprint, formula_version: 'synthetic-character-v1',
      freshness: stale ? 'last_seen' : 'live', issues: snapshot.character?.issues ?? [], applied_defense: request.defense,
      weapons: [variant === 'unknown-weapon' ? 444 : 200, ...request.comparison_item_types]
        .map(item => weapon(item, request.defense, variant)),
      regeneration: { applied_wis: 50, wis: value(6), effects: missing ? [] : [{ source_slot: 0, enchant_id: 900,
        resource: 'hp', applied_maximum: partial ? null : 800, flat_per_sec: value(2),
        fraction_of_max_per_sec: { ...value(.01), evidence: 'catalog' },
        source_units: ['units_per_second', 'fraction_of_maximum_per_second'], subtotal: partial ? value(2) : value(10), total: hp }],
      hp_subtotal: missing ? value(null, 'enchant metadata missing') : partial ? value(2) : value(10), hp_total: hp,
      mp_subtotal: value(6), mp_total: missing ? value(null, 'enchant metadata missing') : value(6),
      base_vit: value(null, 'base VIT unverified'), base_mp: value(null, 'base MP unverified') },
      exalt_damage: { status: 'deferred', value: null, reason: 'deferred pending verification' } };
  }
  const visuals = { client_fingerprint: marker, objects: [
    { object_type: 100, name: 'Synthetic Knight', sprite: null, bag: null },
    ...[200, 201, 202].map(object_type => ({ object_type, name: `Synthetic blade ${object_type}`, sprite: null, bag: null })),
  ] };
  const transport = createFixtureTransport(snapshot, variant === 'missing-catalog' ? undefined : visuals,
    undefined, undefined, undefined, undefined, undefined, { characterCalculation: reply });
  const client = createClient(transport);
  client.queryCatalogAvailable = async () => variant !== 'missing-catalog';
  client.getItemDetails = async type => {
    if (variant === 'missing-catalog' || type === 444) throw new Error('Synthetic metadata unavailable');
    return { item: { object_type: type, name: `Synthetic blade ${type}`, tier: null, slot_type: 1,
      slot_label: null, soulbound: null }, enchants: [] };
  };
  client.searchItems = async query => variant === 'missing-catalog' || !query.trim() ? [] : [201, 202]
    .map(object_type => ({ object_type, name: `Synthetic blade ${object_type}`, tier: null }));
  function setStale(next: boolean) {
    stale = next;
    snapshot.sequence = String(BigInt(snapshot.sequence) + 1n);
    snapshot.capture.state = next ? 'Stopped' : 'Live';
    transport.publishSnapshot(snapshot); transport.publishCapture(snapshot.capture);
  }
  return { client, snapshot, setStale };
}

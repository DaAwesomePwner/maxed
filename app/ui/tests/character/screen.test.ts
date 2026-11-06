import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import type { CharacterCalculation } from '../../src/lib/bindings/CharacterCalculation';
import type { CharacterCalculationRequest } from '../../src/lib/bindings/CharacterCalculationRequest';
import type { CharacterValue } from '../../src/lib/bindings/CharacterValue';
import type { CharacterRange } from '../../src/lib/bindings/CharacterRange';
import type { CharacterWeaponCalculation } from '../../src/lib/bindings/CharacterWeaponCalculation';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { productionFeatures } from '../../src/lib/navigation/routes';
import { homeSnapshot } from '../home/fixtures';
import { fakeHistory } from '../navigation/fake-history';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const value = (n: number | null, reason = 'missing metadata'): CharacterValue => ({ value: n,
  evidence: 'modeled', completeness: n === null ? 'unavailable' : 'complete', reasons: n === null ? [reason] : [], exclusions: [] });
const range = (min: number, max = min): CharacterRange => ({ min, max, evidence: 'catalog',
  completeness: 'complete', reasons: [], exclusions: [] });
function weapon(type = 200): CharacterWeaponCalculation {
  const group = { group_id: 'normal', projectile_index: 0, armor_piercing: false, count: 1,
    rate_factor: value(1), catalog_range: range(100), hit_range: range(150), hit_mean: value(150),
    attacks_per_sec: value(8), projectiles_per_sec: value(8), dps: value(1200) };
  return { item_type: type, applied_att: 75, applied_dex: 75, applied_primary_mask: 0, groups: [group],
    rate_factor: group.rate_factor, catalog_range: group.catalog_range,
    hit_range: group.hit_range, hit_mean: group.hit_mean, attacks_per_sec: group.attacks_per_sec,
    projectiles_per_sec: group.projectiles_per_sec, dps_subtotal: value(1200), dps_total: value(1200) };
}
function observation(): LiveSnapshot {
  const s = homeSnapshot();
  s.character = { ...s.character!, name: 'Synthetic Knight', calculation_revision: { stream_id: s.stream_id,
    character_id: s.character!.id, input_hash: 'synthetic-input', catalog_fingerprint: 'synthetic-catalog' },
    equipment: [0, 1, 2, 3].map(slot => ({ slot, item_type: slot === 0 ? 200 : null,
      observed_empty: slot !== 0, enchant: { state: 'recorded_empty', slots: 0, applied: 0, ids: [] } })),
    stat_panel: { breakdown_provisional: true, conditions: [], lines: [0, 3, 20, 21, 22, 26, 27, 28].map(stat_id => ({
      stat_id, total: 75, current: 70, base: 70, boost: 5, gear: 5, exalt: 0, other: 0, cap: 75,
      at_cap: false, split_evidence: { placement: 'provisional_inside_boost', cap_eligible: true, reasons: [] },
    })) } };
  return s;
}
function reply(request: CharacterCalculationRequest): CharacterCalculation {
  return { revision: request.expected_revision, catalog_fingerprint: request.expected_revision.catalog_fingerprint,
    observed_at: '1000000', formula_version: 'synthetic-character-v1', freshness: 'live', issues: [],
    applied_defense: request.defense, weapons: [weapon(), ...request.comparison_item_types.map(weapon)],
    regeneration: { applied_wis: 50, wis: value(6), effects: [], hp_subtotal: value(0), hp_total: value(0),
      mp_subtotal: value(6), mp_total: value(6), base_vit: value(null, 'base VIT unverified'),
      base_mp: value(null, 'base MP unverified') }, exalt_damage: { status: 'deferred', value: null, reason: 'deferred' } };
}
async function mount(options: { snapshot?: LiveSnapshot; catalog?: boolean; change?: (c: CharacterCalculation) => void } = {}) {
  const snapshot = options.snapshot ?? observation();
  const transport = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined, undefined, undefined,
    { characterCalculation: request => { const c = reply(request); options.change?.(c); return c; } });
  const client = createClient(transport);
  client.queryCatalogAvailable = async () => options.catalog ?? true;
  client.getObjectVisuals = async types => ({ client_fingerprint: 'synthetic-catalog', objects: types.map(object_type => ({
    object_type, name: object_type === 100 ? 'Knight' : `Synthetic weapon ${object_type}`, sprite: null, bag: null,
  })) });
  client.getItemDetails = async type => ({ item: { object_type: type, name: `Synthetic weapon ${type}`,
    tier: null, slot_type: type === 999 ? 9 : 1, slot_label: null, soulbound: null }, enchants: [] });
  client.searchItems = async () => [201, 202, 999].map(object_type => ({ object_type, name: `Synthetic weapon ${object_type}`, tier: null }));
  const calculate = vi.spyOn(client, 'getCharacterCalculation');
  const page = render(Shell, { client, features: { ...productionFeatures, character: true }, preview: true,
    historyAdapter: fakeHistory('#/character').adapter });
  await screen.findByRole('spinbutton', { name: 'Enemy defense' });
  if (snapshot.character && snapshot.capture.state === 'Live') await waitFor(() => expect(calculate).toHaveBeenCalled());
  return { ...page, client, transport, snapshot, calculate, main: within(screen.getByRole('main')) };
}

it('renders all five sections, observed identity, four slots, raw stats and eligible pots', async () => {
  const p = await mount();
  expect(p.main.getAllByRole('heading', { level: 2 }).map(h => h.textContent)).toEqual([
    'Character', 'Stats', 'Weapon', 'Regeneration', 'Exalt damage',
  ]);
  const identity = p.main.getByRole('region', { name: 'Character' });
  expect(identity.textContent).toContain('Synthetic Knight'); expect(identity.textContent).toContain('Knight');
  expect(identity.textContent).toContain('Level 20'); expect(identity.textContent).toContain('Live');
  expect(within(identity).getAllByText('Empty')).toHaveLength(3);
  const stats = p.main.getByRole('region', { name: 'Stats' });
  for (const name of ['HP', 'MP', 'ATT', 'DEF', 'SPD', 'DEX', 'VIT', 'WIS']) expect(within(stats).getByRole('row', { name: new RegExp(`^${name} `) })).toBeTruthy();
  expect(stats.textContent).toMatch(/provisional/i); expect(stats.textContent).toMatch(/unexplained remainder/i);
  expect(within(stats).getByRole('row', { name: /^HP / }).textContent).toContain('1 pot left');
  expect(within(stats).getByRole('row', { name: /^ATT / }).textContent).toContain('5 pots left');
  expect(within(stats).getByRole('columnheader', { name: 'Raw boost' })).toBeTruthy();
  expect(p.main.getByText('Unavailable — deferred pending verification')).toBeTruthy();
  expect(p.main.queryByText('1.0×')).toBeNull();
});
it('renders authored group results with precise rounding, applied defense and accessible formula details', async () => {
  const p = await mount({ change: c => { c.weapons[0].groups[0].attacks_per_sec = value(8.126); c.weapons[0].groups[0].dps = value(1200.126); } });
  const section = p.main.getByRole('region', { name: 'Weapon' });
  expect(section.textContent).toContain('Normal firing rate'); expect(section.textContent).toContain('Theoretical DPS');
  expect(section.textContent).toContain('100–100'); expect(section.textContent).toContain('150–150');
  expect(section.textContent).toContain('8.13'); expect(section.textContent).toContain('1200.13');
  expect(section.textContent).toContain('Applied defense: 0');
  expect(section.textContent).toMatch(/Excludes exalt damage/);
  const details = within(section).getAllByText('Formula details')[0];
  details.focus(); await userEvent.setup().keyboard('{Enter}');
  expect(details.closest('details')?.open).toBe(true);
  expect(section.textContent).toContain('synthetic-character-v1'); expect(section.textContent).toContain('ATT: 75');
  expect(section.textContent).toContain('DEX: 75'); expect(section.textContent).toContain('Evidence');
});
it('keeps the layout and exact empty-state instruction without fabricated observations', async () => {
  const s = observation(); s.character = null;
  const p = await mount({ snapshot: s });
  expect(p.main.getByText('No character observed. Start capture and enter the game.')).toBeTruthy();
  expect(p.main.getAllByRole('heading', { level: 2 })).toHaveLength(5);
  expect(p.main.queryByText('Empty')).toBeNull(); expect(p.calculate).not.toHaveBeenCalled();
});
it('shows partial identity and coverage while preserving available observations', async () => {
  const s = observation(); s.character!.identity_verified = false; s.character!.issues = ['missing_identity'];
  const p = await mount({ snapshot: s });
  expect(p.main.getByRole('region', { name: 'Character' }).textContent).toMatch(/Partial.*identity/is);
  expect(p.main.getByText('Synthetic Knight')).toBeTruthy();
});
it.each(['Stopped', 'Recovering', 'Failed', 'died'] as const)('never labels frozen %s observations Live', async state => {
  const p = await mount();
  const next = structuredClone(p.snapshot); next.sequence = '13';
  if (state === 'died') next.character!.died_at = '6000000'; else next.capture.state = state;
  p.transport.publishSnapshot(next);
  p.transport.publishCapture(next.capture);
  await waitFor(() => {
    const text = p.main.getByRole('region', { name: 'Character' }).textContent;
    expect(text).toContain(state === 'died' ? 'Died' : 'Last seen'); expect(text).not.toMatch(/\bLive\b/);
  });
});
it('keeps observed slots/stats and a Settings link when assets are missing', async () => {
  const p = await mount({ catalog: false });
  const section = p.main.getByRole('region', { name: 'Weapon' });
  expect(within(section).getByText('Weapon calculations unavailable — extract game assets')).toBeTruthy();
  expect(within(section).getByRole('link', { name: 'Settings' }).getAttribute('href')).toBe('#/settings?tab=assets');
  expect(p.main.getByText('Synthetic Knight')).toBeTruthy();
  expect(p.main.getByRole('region', { name: 'Stats' }).textContent).toContain('75');
});
it.each(['ATT', 'DEX', 'condition mask'])('retains independent values when %s is missing', async missing => {
  const p = await mount({ change: c => {
    const w = c.weapons[0], g = w.groups[0];
    if (missing === 'DEX') { g.attacks_per_sec = value(null, 'DEX missing'); g.projectiles_per_sec = value(null, 'DEX missing'); }
    else { g.hit_range = { ...range(0), min: null, max: null, completeness: 'unavailable', reasons: [`${missing} missing`] }; }
    g.dps = value(null, `${missing} missing`); w.dps_total = value(null, `${missing} missing`);
  } });
  const text = p.main.getByRole('region', { name: 'Weapon' }).textContent!;
  expect(text).toContain(`${missing} missing`); expect(text).toContain('—');
  expect(text).toContain(missing === 'DEX' ? '150–150' : '8.00');
});
it('distinguishes supported subtotal from unavailable full weapon total and unsupported groups', async () => {
  const p = await mount({ change: c => {
    c.weapons[0].dps_total = value(null, 'unsupported firing schedule');
    c.weapons[0].groups.push({ ...c.weapons[0].groups[0], group_id: 'charged', dps: value(null, 'charged schedule missing') });
  } });
  const section = p.main.getByRole('region', { name: 'Weapon' });
  expect(section.textContent).toMatch(/Supported subtotal.*1200.00/s);
  expect(section.textContent).toMatch(/Full total.*—.*unsupported firing schedule/s);
  expect(section.textContent).toContain('charged schedule missing');
});
it('uses Task 4 evidence to withhold cap/pots despite a provisional base and legacy at_cap', async () => {
  const s = observation(), line = s.character!.stat_panel!.lines.find(l => l.stat_id === 20)!;
  line.exalt = 2; line.at_cap = true; line.split_evidence = { placement: 'provisional_inside_boost', cap_eligible: false, reasons: ['unverified_placement'] };
  const p = await mount({ snapshot: s });
  const row = within(p.main.getByRole('region', { name: 'Stats' })).getByRole('row', { name: /^ATT / });
  expect(row.textContent).toContain('exalt placement unverified'); expect(row.textContent).not.toContain('At cap');
});
it('shows partial percentage regeneration, known flat subtotal and both excluded base rows', async () => {
  const p = await mount({ change: c => {
    c.regeneration.hp_total = value(null, 'maximum HP missing'); c.regeneration.hp_subtotal = value(2);
    c.regeneration.effects = [{ source_slot: 0, enchant_id: 10, resource: 'hp', applied_maximum: null,
      flat_per_sec: value(2), fraction_of_max_per_sec: value(0.01), source_units: ['per second'],
      subtotal: value(2), total: value(null, 'maximum HP missing') }];
  } });
  const section = p.main.getByRole('region', { name: 'Regeneration' });
  expect(section.textContent).toContain('maximum HP missing'); expect(section.textContent).toContain('2.00');
  expect(section.textContent).toContain('Base VIT'); expect(section.textContent).toContain('Base MP regeneration');
  expect(section.textContent).toContain('Excludes base VIT, pets, healing effects and combat-state modifiers.');
  expect(section.textContent).toContain('Excludes base MP regeneration, pets, consumables, ability costs and combat-state modifiers.');
  expect(section.textContent).toContain('Included'); expect(section.textContent).toContain('Excluded');
  expect(section.textContent).not.toMatch(/total recovery|net mana sustain/i);
});
it('distinguishes recorded empty zero regen from missing enchant metadata', async () => {
  const p = await mount({ change: c => { c.regeneration.mp_total = value(null, 'enchant metadata missing'); } });
  const section = p.main.getByRole('region', { name: 'Regeneration' });
  expect(section.textContent).toContain('0.00'); expect(section.textContent).toContain('enchant metadata missing');
});
it('validates defense without relabelling the last calculation and supports Reset by keyboard', async () => {
  const p = await mount(); const input = p.main.getByRole('spinbutton', { name: 'Enemy defense' });
  await fireEvent.input(input, { target: { value: '50' } });
  await waitFor(() => expect(p.main.getByText('Applied defense: 50', { selector: 'p' })).toBeTruthy());
  await fireEvent.input(input, { target: { value: '10001' } });
  expect(p.main.getByText('Enter an integer from 0 to 10,000.')).toBeTruthy();
  expect(p.main.getByText('Applied defense: 50', { selector: 'p' })).toBeTruthy();
  p.main.getByRole('button', { name: 'Reset' }).focus(); await userEvent.setup().keyboard('{Enter}');
  await waitFor(() => expect(p.main.getByText('Applied defense: 0', { selector: 'p' })).toBeTruthy());
});
it('filters non-weapons, allows two catalog picks, keeps equipped first and preserves picks on catalog churn', async () => {
  const p = await mount(); const user = userEvent.setup();
  await user.type(p.main.getByRole('searchbox', { name: 'Find catalog weapons' }), 'Synthetic');
  const select = p.main.getByRole('combobox', { name: 'Catalog weapon' });
  await waitFor(() => expect(within(select).getByRole('option', { name: /201/ })).toBeTruthy());
  expect(within(select).queryByRole('option', { name: /999/ })).toBeNull();
  await user.selectOptions(select, '201'); await user.click(p.main.getByRole('button', { name: 'Add comparison' }));
  await user.selectOptions(select, '202'); await user.click(p.main.getByRole('button', { name: 'Add comparison' }));
  await waitFor(() => expect(p.main.getAllByRole('article').map(a => a.getAttribute('aria-label'))).toEqual([
    'Equipped weapon', 'Comparison 201', 'Comparison 202',
  ]));
  expect((p.main.getByRole('button', { name: 'Add comparison' }) as HTMLButtonElement).disabled).toBe(true);
  expect(p.main.getByText('At current stats; equipment bonuses are not recalculated.')).toBeTruthy();
  expect(p.main.getByRole('region', { name: 'Weapon' }).textContent).toContain('Catalog-only');
  p.client.getItemDetails = async () => { throw new Error('missing item'); };
  const next = structuredClone(p.snapshot); next.sequence = '13'; next.character!.calculation_revision!.catalog_fingerprint = 'replacement';
  p.transport.publishSnapshot(next);
  await waitFor(() => expect(p.main.getByRole('article', { name: 'Comparison 201' }).textContent).toMatch(/201.*metadata unavailable/s));
  const remove = p.main.getByRole('button', { name: 'Remove comparison 201' }); remove.focus(); await user.keyboard('{Enter}');
  await waitFor(() => expect(p.main.queryByRole('article', { name: 'Comparison 201' })).toBeNull());
});

it.each([
  { scenario: 'sole comparison', picks: [201], removed: 201, remaining: null },
  { scenario: 'last of two comparisons', picks: [201, 202], removed: 202, remaining: 201 },
  { scenario: 'first of two comparisons', picks: [201, 202], removed: 201, remaining: 202 },
])('restores keyboard focus after removing the $scenario', async ({ picks, removed, remaining }) => {
  const p = await mount(); const user = userEvent.setup();
  await user.type(p.main.getByRole('searchbox', { name: 'Find catalog weapons' }), 'Synthetic');
  const select = p.main.getByRole('combobox', { name: 'Catalog weapon' });
  await waitFor(() => expect(within(select).getByRole('option', { name: /201/ })).toBeTruthy());
  for (const type of picks) {
    await user.selectOptions(select, String(type));
    await user.click(p.main.getByRole('button', { name: 'Add comparison' }));
  }
  p.main.getByRole('button', { name: `Remove comparison ${removed}` }).focus();
  await user.keyboard('{Enter}');
  await waitFor(() => {
    expect(p.main.queryByRole('article', { name: `Comparison ${removed}` })).toBeNull();
    expect(document.activeElement).toBe(remaining === null ? select
      : p.main.getByRole('button', { name: `Remove comparison ${remaining}` }));
  });
});

it('retains raw observations when the expanded stat panel is unavailable', async () => {
  const s = observation(); s.character!.stat_panel = null; s.character!.stats = [{ stat_id: 20, value: 81 }];
  const p = await mount({ snapshot: s });
  const stats = p.main.getByRole('region', { name: 'Stats' });
  expect(stats.textContent).toContain('Stat 20: 81');
  expect(stats.textContent).toContain('Split evidence missing');
});
it('keeps unknown item identifiers and formula evidence even without supported groups', async () => {
  const s = observation(); s.character!.equipment[0].item_type = 444;
  const p = await mount({ snapshot: s, change: c => {
    const w = c.weapons[0]; w.item_type = 444; w.groups = []; w.applied_att = null;
    w.dps_total = value(null, 'item metadata missing'); w.dps_subtotal = value(null, 'item metadata missing');
  } });
  const article = p.main.getByRole('article', { name: 'Equipped weapon' });
  expect(article.textContent).toContain('444'); expect(article.textContent).toContain('item metadata missing');
  expect(within(article).getByText('Formula details')).toBeTruthy();
  expect(article.textContent).toContain('synthetic-character-v1');
  expect(article.textContent).toMatch(/ATT: —.*Not observed/s);
});
it('does not label the previous weapon calculation with a newly equipped item', async () => {
  const p = await mount();
  p.client.getCharacterCalculation = () => new Promise(() => {});
  const next = structuredClone(p.snapshot); next.sequence = '13';
  next.character!.equipment[0].item_type = 333;
  next.character!.calculation_revision!.input_hash = 'replacement-equipment';
  p.transport.publishSnapshot(next);
  await waitFor(() => {
    const article = p.main.getByRole('article', { name: 'Equipped weapon' });
    expect(article.textContent).toContain('333');
    expect(article.textContent).not.toContain('1200.00');
  });
});
it('freezes Character during resync and clears previous results on identity change', async () => {
  const p = await mount();
  let resolve!: (s: LiveSnapshot) => void;
  p.client.querySnapshot = () => new Promise<LiveSnapshot>(done => { resolve = done; });
  p.transport.publishEngineEvent({ stream_id: p.snapshot.stream_id, sequence: '14', at: p.snapshot.at,
    event: { session_updated: p.snapshot.session! } });
  await waitFor(() => expect(p.main.getByRole('region', { name: 'Character' }).textContent).toContain('Last seen'));
  expect(p.main.getByRole('region', { name: 'Character' }).textContent).not.toMatch(/\bLive\b/);
  const next = structuredClone(p.snapshot); next.sequence = '14'; next.character!.id = 'synthetic-replacement';
  next.character!.name = 'Synthetic replacement'; next.character!.calculation_revision!.character_id = next.character!.id;
  p.client.getCharacterCalculation = () => new Promise(() => {});
  resolve(next);
  await waitFor(() => expect(p.main.getByText('Synthetic replacement')).toBeTruthy());
  expect(p.main.getByRole('region', { name: 'Weapon' }).textContent).not.toContain('1200.00');
});
it('labels a known-empty equipped weapon as Empty without claiming it was unobserved', async () => {
  const s = observation(); s.character!.equipment[0].item_type = null; s.character!.equipment[0].observed_empty = true;
  const p = await mount({ snapshot: s, change: c => {
    c.weapons[0].item_type = null; c.weapons[0].groups = [];
    c.weapons[0].dps_total = value(null, 'empty_weapon_slot');
    c.weapons[0].dps_subtotal = value(null, 'empty_weapon_slot');
  } });
  const article = p.main.getByRole('article', { name: 'Equipped weapon' });
  expect(within(article).getByRole('heading', { name: 'Equipped weapon · Empty' })).toBeTruthy();
  expect(article.textContent).not.toContain('Weapon not observed');
});
it('does not reread class metadata for unrelated live updates', async () => {
  const p = await mount();
  await waitFor(() => expect(p.main.getByRole('region', { name: 'Character' }).textContent).toContain('Knight'));
  const visuals = vi.spyOn(p.client, 'getObjectVisuals');
  const next = structuredClone(p.snapshot); next.sequence = '13'; next.at = '6000000';
  next.character!.observed_at = '6000000'; next.character!.level = 19;
  p.transport.publishSnapshot(next);
  await waitFor(() => expect(p.main.getByRole('region', { name: 'Character' }).textContent).toContain('Level 19'));
  expect(visuals).not.toHaveBeenCalled();
});

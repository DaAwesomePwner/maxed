import { render, screen } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import RosterTable from '../../src/lib/party/RosterTable.svelte';
import type { PartyMember } from '../../src/lib/bindings/PartyMember';
import type { PartyView } from '../../src/lib/bindings/PartyView';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';

function mount(saved: boolean, evidence: Partial<PartyMember>, withReason = true) {
  const member: PartyMember = { player_id: 'synthetic', name: 'Synthetic', name_key: 'synthetic',
    is_local: true, maxed: 5, equipment: [], flags: [], runs_together: 0, ...evidence,
    parse: { result: 'fail', reasons: withReason ? [{ kind: 'below_maxed', text: '5/8 < 6/8' }] : [] } };
  const view: PartyView = { area_kind: 'dungeon', live: !saved, members: [member], classes: [],
    assets_available: true, gear_recorded: true, preset: { id: 'synthetic', name: 'Synthetic', source: 'default' },
    counts: { in_view: 1, pass: 0, fail: 1, check: 0, not_seen: 0, no_damage_seen: 0, left_early: 0 } };
  return render(RosterTable, { view, members: view.members, saved, selected: null,
    visuals: emptyHomeVisuals(), resolveSpriteUrl: () => null, onSelect: () => {}, onRun: () => {} });
}

for (const saved of [false, true]) {
  it.each([{}, { maxed_gated: false }])(`hides legacy local maxed count and failure styling (saved: ${saved}, %j)`, evidence => {
    const { container } = mount(saved, evidence);
    const count = container.querySelector('tbody tr td:nth-child(3)')!;
    expect(count.textContent).toBe('—');
    expect(count.classList.contains('failing')).toBe(false);
    expect(screen.queryByText('5/8 < 6/8')).toBeNull();
  });
  it.each([{ maxed_gated: true }, { is_local: false }])(`keeps supported maxed counts and styling (saved: ${saved}, %j)`, evidence => {
    mount(saved, evidence);
    expect(screen.getByText('5/8').classList.contains('failing')).toBe(true);
    expect(screen.getByText('5/8 < 6/8')).toBeTruthy();
  });
  it(`uses the effective count for the observation note (saved: ${saved})`, () => {
    mount(saved, {}, false);
    expect(screen.getByText('Gear not observed yet')).toBeTruthy();
  });
}

import { describe, expect, it } from 'vitest';
import type { PartyMember } from '../../src/lib/bindings/PartyMember';
import type { RequirementPreset } from '../../src/lib/bindings/RequirementPreset';
import { failingNamesText, filterMembers, flagLabel, presetCopyText, presetSummary, reasonText, whisperText } from '../../src/lib/party/party-screen';

const preset: RequirementPreset = { id: 'synth', name: 'Synthetic', min_maxed: 6,
  min_tier: { weapon: 11, ability: 5 }, ut_st_meets: true, auto_apply: [] };
const member = (name: string, patch: Partial<PartyMember> = {}): PartyMember => ({ player_id: name,
  name, equipment: [], is_local: false, present: true, flags: [], runs_together: 0, ...patch });
const members = [member('SynthAlpha', { parse: { result: 'pass', reasons: [] } }),
  member('SynthBravo', { parse: { result: 'fail', reasons: [] }, mark: 'watched' }),
  member('SynthCheck', { parse: { result: 'check', reasons: [] }, flags: ['no_damage_seen'] }),
  member('SynthSeen', { parse: { result: 'not_seen', reasons: [] } }),
  member('SynthLeft', { parse: { result: 'fail', reasons: [] }, present: false }),
  member('SynthSaved', { parse: { result: 'fail', reasons: [] }, present: null }),
  member('SynthNone', { name: null, parse: { result: 'fail', reasons: [] } })];

describe('Party screen text', () => {
  it('summarizes rules in slot order and copies the announcement', () => {
    expect(presetSummary(preset)).toBe('6/8 · T11+ weapon · T5+ ability · UT/ST ok');
    expect(presetCopyText(preset)).toBe('Reqs: 6/8 · T11+ weapon · T5+ ability · UT/ST ok');
    expect(presetSummary({ ...preset, min_maxed: null, min_tier: {}, ut_st_meets: true })).toBe('No rules');
    expect(presetSummary({ ...preset, min_tier: {}, ut_st_meets: true })).toBe('6/8');
    expect(presetSummary({ ...preset, min_maxed: null, min_tier: { ring: 0, armor: 12 }, ut_st_meets: false })).toBe('T12+ armor · T0+ ring');
  });
  it('joins native reasons without rewriting unknown observations', () => {
    expect(reasonText([{ kind: 'below_maxed', text: '5/8 < 6/8' },
      { slot: 'weapon', kind: 'below_tier', text: 'weapon T9 < T11' }])).toBe('5/8 < 6/8 · weapon T9 < T11');
    expect(reasonText([])).toBe('');
  });
  it.each([
    ['all', members], ['failing', [members[1], members[4], members[5], members[6]]],
    ['check', [members[2]]], ['not-seen', [members[3]]], ['flagged', [members[2]]], ['marked', [members[1]]],
  ] as const)('filters %s', (filter, expected) => expect(filterMembers(members, filter)).toEqual(expected));
  it('copies only named, present failures, preserving the whisper trailing space', () => {
    expect(failingNamesText(members)).toBe('SynthBravo');
    expect(failingNamesText([])).toBe('');
    expect(whisperText('SynthAlpha')).toBe('/t SynthAlpha ');
  });
  it('labels every contribution flag', () => {
    expect(['no_damage_seen', 'left_early', 'mostly_trash'].map(flag => flagLabel(flag as PartyMember['flags'][number])))
      .toEqual(['No damage seen', 'Left early', 'Mostly trash']);
  });
});

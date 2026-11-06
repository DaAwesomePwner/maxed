import type { ContributionFlag } from '../bindings/ContributionFlag';
import type { PartyMember } from '../bindings/PartyMember';
import type { ParseReason } from '../bindings/ParseReason';
import type { RequirementPreset } from '../bindings/RequirementPreset';

export const equipSlots = ['weapon', 'ability', 'armor', 'ring'] as const;
export type MemberFilter = 'all' | 'failing' | 'check' | 'not-seen' | 'flagged' | 'marked';
export function presetSummary(preset: RequirementPreset): string {
  const rules: string[] = [];
  if (preset.min_maxed != null) rules.push(`${preset.min_maxed}/8`);
  const tiers = equipSlots.flatMap(slot => preset.min_tier[slot] == null ? [] : [`T${preset.min_tier[slot]}+ ${slot}`]);
  rules.push(...tiers);
  if (tiers.length && preset.ut_st_meets) rules.push('UT/ST ok');
  return rules.join(' · ') || 'No rules';
}
export const presetCopyText = (preset: RequirementPreset): string => `Reqs: ${presetSummary(preset)}`;
export const reasonText = (reasons: ParseReason[]): string => reasons.map(reason => reason.text).join(' · ');
export function filterMembers(members: PartyMember[], filter: MemberFilter): PartyMember[] {
  return members.filter(member => filter === 'all'
    || (filter === 'failing' && member.parse?.result === 'fail')
    || (filter === 'check' && member.parse?.result === 'check')
    || (filter === 'not-seen' && member.parse?.result === 'not_seen')
    || (filter === 'flagged' && member.flags.length > 0)
    || (filter === 'marked' && member.mark != null));
}
export const failingNamesText = (members: PartyMember[]): string => members
  .filter(member => member.present === true && member.parse?.result === 'fail' && member.name)
  .map(member => member.name).join('\n');
export const whisperText = (name: string): string => `/t ${name} `;
export function flagLabel(flag: ContributionFlag): string {
  return { no_damage_seen: 'No damage seen', left_early: 'Left early', mostly_trash: 'Mostly trash' }[flag];
}
export function sortMembers(members: PartyMember[]): PartyMember[] {
  const rank = { fail: 0, check: 1, not_seen: 2, pass: 3 };
  return [...members].sort((a, b) => rank[a.parse?.result ?? 'not_seen'] - rank[b.parse?.result ?? 'not_seen']
    || (b.damage ?? -1) - (a.damage ?? -1) || (a.name ?? '').localeCompare(b.name ?? ''));
}
export function partyDate(at: string | null | undefined): string {
  if (at == null || !/^\d+$/.test(at)) return '—';
  const date = new Date(Number(BigInt(at) / 1000n));
  return Number.isFinite(date.getTime()) ? date.toLocaleDateString() : '—';
}
export function presenceTime(at: string): string {
  const date = new Date(Number(BigInt(at) / 1000n));
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

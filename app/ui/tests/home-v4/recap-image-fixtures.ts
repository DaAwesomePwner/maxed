import type { RunRecapView, VisitRecapView } from '../../src/lib/home/v4/recap-view';
import { homeSnapshot } from '../home/fixtures';

export const options = { names: true, loot: true, fights: true };
export const sprite = 'http://maxed-sprite.localhost/' + 'a'.repeat(64) + '/characters.png/0-0-8-8.png';
export const context = { character: 'Synthetic Hero', className: 'Synthetic Knight', date: 'Oct 13, 2026',
  spriteUrl: () => sprite, playerClassName: (type: number | null) => type === 1 ? 'Synthetic Knight' : null,
  objectName: (type: number) => `Synthetic object ${type}` };
export function runView(): RunRecapView {
  const tile = (key: RunRecapView['tiles'][number]['key'], value: string, detail: string | null = null) =>
    ({ key, label: 'Deliberately unrelated label', value, detail, tone: null, partial: false, comparison: null });
  const drop = { ...homeSnapshot().drops[0], id: 'synthetic-drop', bag_type: 20, contents: [], items: [21], contents_known: true };
  return { kind: 'run', runId: 'synthetic-run', title: 'Synthetic Sanctum', outcome: 'cleared', outcomeLabel: 'Cleared',
    clearTime: '1:00', pb: true, meta: [], chain: null, best: { drop }, streakNote: null, streak: null, streakEnded: null,
    tiles: [tile('fame', '+12'), tile('party', '+99%'), tile('dps', '1,234'), tile('rank', '#2 of 8', '25% of damage')],
    fights: [{ fightId: 'synthetic-fight', targetType: 30, name: 'Synthetic Guardian', kill: '0:30', dps: 100,
      best: false, vsParty: .5, rank: 2, players: 8, buckets: null }], fightsPartial: false, chips: [],
    party: { top: [], more: 0, rows: Array.from({ length: 7 }, (_, i) => ({ rank: i + 1, name: `Synthetic player ${i + 1}`,
      classId: i === 2 ? null : 1, share: .25, isLocal: i === 1 })) }, drops: [drop], death: null, died: null,
    character: { name: 'Synthetic Hero', class_id: 1, maxed: 8 } };
}
export function visitView(): VisitRecapView {
  return { kind: 'visit', visitId: 'synthetic-visit', characterId: 'synthetic-character', title: 'Synthetic Realm', timeHere: '2:00',
    meta: [], chain: null, best: null, drops: [], dropsPartial: false, chips: [], tiles: [], events: ['killed', 'announced'].map((state, i) => ({
      key: `synthetic-event-${i}`, targetType: 30 + i, name: `Synthetic event ${i}`, at: '1000000',
      state: state as 'killed' | 'announced', dps: 100, rank: 2, players: 8, share: .25, bags: [],
      kill: '0:45', vsParty: -.25, buckets: null, best: false })) };
}

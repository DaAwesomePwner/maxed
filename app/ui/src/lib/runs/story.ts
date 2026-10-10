import type { RunStory } from '../bindings/RunStory';
import type { RunView } from '../bindings/RunView';
import type { RunFightSummary } from '../bindings/RunFightSummary';
import type { TimelineEntry } from '../bindings/TimelineEntry';
import type { EvidenceAt } from '../bindings/EvidenceAt';
import type { DropView } from '../bindings/DropView';
import { objectVisualTypes } from '../bridge/visuals';
import type { EnchantView } from '../bindings/EnchantView';
import { formatElapsed, formatStat } from '../styles/format';

export const WINDOW_SIZE = 50;
export const FIGHT_WINDOW_SIZE = 25;
export function windowIndex(index: number, length: number, size = WINDOW_SIZE): number {
  return Math.min(Math.max(0, (Number.isFinite(index) ? Math.floor(index) : 0)), Math.max(0, Math.ceil(length / size) - 1));
}
export function windowRows<T>(rows: T[], index: number, size = WINDOW_SIZE): T[] {
  const start = windowIndex(index, rows.length, size) * size;
  return rows.slice(start, start + size);
}
export function timestamp(at: string): string {
  try {
    const date = new Date(Number(BigInt(at) / 1000n));
    return Number.isFinite(date.getTime()) ? date.toLocaleString() : '—';
  } catch { return '—'; }
}
export function metric(value: number | null): string {
  return value !== null && Number.isFinite(value) && value >= 0 ? formatStat(value) : '—';
}
/** Debug-only evidence: counter completion can estimate clear time. */
export function evidenceText({ at, evidence }: EvidenceAt): string {
  let text: string;
  if (typeof evidence === 'string') {
    text = { victory: 'Victory observed', death_packet: 'Death observed', observed_exit: 'Exit observed',
      capture_gap: 'Capture gap', app_closed: 'App closed' }[evidence];
  } else if ('completion_dialogue' in evidence) text = `Completion dialogue: ${evidence.completion_dialogue.rule}`;
  else {
    const counter = evidence.completion_counter;
    text = `Completion counter ${counter.before} \u2192 ${counter.after}; delay ${metric(counter.delay_micros / 1_000_000)} seconds. Clear time estimated.`;
  }
  return `${timestamp(at)} \u2014 ${text}`;
}
export function estimatedClear(story: RunStory): boolean {
  return story.run.evidence.some(({ evidence }) => typeof evidence === 'object' && 'completion_counter' in evidence)
    && !story.run.evidence.some(({ evidence }) => evidence === 'victory' || (typeof evidence === 'object' && 'completion_dialogue' in evidence));
}
/** Outcomes can coexist; retain both timestamps and their saved evidence. */
export function clearAndDeath(run: RunView): boolean {
  const clear = run.completed_at !== null || run.evidence.some(({ evidence }) => evidence === 'victory'
    || (typeof evidence === 'object' && ('completion_dialogue' in evidence || 'completion_counter' in evidence)));
  const death = run.died_at !== null || run.evidence.some(({ evidence }) => evidence === 'death_packet');
  return clear && death;
}
export function bossTier(fight: RunFightSummary): boolean {
  return fight.boss_type !== null || ['boss', 'event_boss', 'miniboss', 'quest'].includes(fight.target_kind ?? '');
}
export function bossEncounters(fights: RunFightSummary[]): RunFightSummary[] {
  return fights.filter(bossTier).sort((a, b) => compareTime({ at: a.started, id: a.id }, { at: b.started, id: b.id }));
}
export function notableFightIds(fights: RunFightSummary[]): Set<string> {
  return new Set(fights.filter(bossTier).map(fight => fight.id));
}
/** Boss visuals stay available independently of either three-row window. */
export function storyVisualTypes(story: RunStory | null, otherFights: RunFightSummary[], drops: DropView[]): number[] {
  return objectVisualTypes([story?.character?.class_id,
    ...(story?.fights.filter(bossTier) ?? []).map(fight => fight.target_type ?? fight.boss_type),
    ...otherFights.map(fight => fight.target_type ?? fight.boss_type),
    ...drops.flatMap(drop => [drop.bag_type, drop.source_enemy, ...drop.contents.map(item => item.item_type), ...(drop.contents_known ? drop.items : [])])]);
}
export type StoryPoint = TimelineEntry & { group?: 'enemies' | 'departures'; count?: number; damage?: number; members?: TimelineEntry[]; incomplete?: boolean };
/** Display titles stay separate from retained explanations and their debug-only caveats. */
export function timelineTitle(point: StoryPoint): string {
  if (point.group === 'enemies') return `Enemies: ${point.count} · Damage: ${metric(point.damage ?? null)}`;
  if (point.group === 'departures') return `Departures: ${point.count}`;
  return { entered: 'Entry', fight: 'Fight', drop: 'Loot', completion: 'Clear', death: 'Death',
    exit: 'Exit', capture_issue: 'Capture', party_death: 'Party Death', player_left: 'Departure',
    portal_opened: 'Portal', kill: 'Kill' }[point.kind];
}
/** Only typed fight points with an actual summary can open a fight. */
export function fightForEntry(entry: TimelineEntry, fights: RunFightSummary[]): string | null {
  return entry.kind === 'fight' && entry.related_id !== null && fights.some(fight => fight.id === entry.related_id) ? entry.related_id : null;
}
const compareTime = (a: { at: string; id: string }, b: { at: string; id: string }) => {
  const left = BigInt(a.at), right = BigInt(b.at);
  return left < right ? -1 : left > right ? 1 : a.id.localeCompare(b.id);
};
export function storyTimeline(story: RunStory): StoryPoint[] {
  const fights = new Map(story.fights.map(fight => [fight.id, fight]));
  const points = story.timeline.filter(point => point.kind !== 'player_left'
    || story.run.completed_at === null || BigInt(point.at) < BigInt(story.run.completed_at));
  // Legacy summaries can lack timeline projections. Preserve each actual fight.
  const related = new Set(points.filter(point => point.kind === 'fight').map(point => point.related_id));
  for (const fight of story.fights) if (!related.has(fight.id)) points.push({ id: `summary:${fight.id}`, at: fight.started,
    kind: 'fight', related_id: fight.id, explanation: `Encounter ${fight.ordinal}: ${fight.target_name ?? 'Observed target'}` });
  story.run.evidence.forEach((evidence, index) => {
    const value = evidence.evidence;
    const kind = value === 'death_packet' ? 'death' : value === 'observed_exit' ? 'exit'
      : value === 'capture_gap' || value === 'app_closed' ? 'capture_issue' : 'completion';
    const matching = points.findIndex(point => point.kind === kind && point.at === evidence.at);
    if (matching >= 0) {
      points[matching] = { ...points[matching], explanation: `${points[matching].explanation}; ${evidenceText(evidence)}` };
    } else points.push({ id: `evidence:${story.run.id}:${index}`, at: evidence.at, kind, related_id: story.run.id, explanation: evidenceText(evidence) });
  });
  const grouped: StoryPoint[] = [];
  for (const point of points.sort(compareTime)) {
    const fight = point.kind === 'fight' ? fights.get(point.related_id ?? '') : undefined;
    const group = point.kind === 'fight' && (!fight || !bossTier(fight)) ? 'enemies'
      : point.kind === 'player_left' ? 'departures' : undefined;
    if (!group) { grouped.push(point); continue; }
    const previous = grouped[grouped.length - 1];
    const row: StoryPoint = previous?.group === group ? previous : { ...point, related_id: null, group, count: 0, damage: 0, members: [] };
    if (row !== previous) grouped.push(row);
    row.count!++;
    row.members!.push(point);
    if (fight && Number.isFinite(fight.total_damage)) row.damage! += fight.total_damage;
    else if (group === 'enemies') row.incomplete = true;
    row.explanation = group === 'enemies'
      ? `Fought ${row.count} ${row.count === 1 ? 'enemy' : 'enemies'} \u2014 ${metric(row.damage!)} damage${row.incomplete ? ' (incomplete summaries)' : ''}`
      : `${row.count} ${row.count === 1 ? 'player left' : 'players left'}`;
  }
  return grouped;
}
export function observedThrough(story: RunStory): string {
  const times = [story.run.started, story.run.completed_at, story.run.died_at,
    ...(story.events ?? []).map(event => event.at), ...(story.boss_fights ?? []).flatMap(fight => [fight.started, fight.ended, fight.last_hit]),
    ...story.timeline.map(point => point.at), ...story.run.evidence.map(point => point.at),
    ...story.drops.map(drop => drop.at), ...story.fights.flatMap(fight => [fight.started, fight.ended, fight.last_hit])];
  return times.reduce<string>((latest, at) => at !== null && BigInt(at) > BigInt(latest) ? at : latest, story.run.started);
}
export function elapsed(start: string, end: string | null): string {
  return end === null || BigInt(end) < BigInt(start) ? '—' : formatElapsed(start, end);
}
export function enchantText(enchant: EnchantView): string {
  if (enchant.state === 'recorded_empty') return `Enchantments: 0 · Slots: ${metric(enchant.slots)}`;
  if (enchant.state === 'recorded') return `Enchantments: ${enchant.ids.join(', ') || '—'} · Applied: ${metric(enchant.applied)} · Slots: ${metric(enchant.slots)}`;
  return '—';
}

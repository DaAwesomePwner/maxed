import type { PlayerTimeline } from '../bindings/PlayerTimeline';
import type { FightView } from '../bindings/FightView';
import type { GearChange } from '../bindings/GearChange';
import type { EquipmentView } from '../bindings/EquipmentView';
import type { RunPlayerView } from '../bindings/RunPlayerView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { PlayerProfileView } from '../bindings/PlayerProfileView';
import type { PlayerDeathAt } from '../bindings/PlayerDeathAt';
import type { SourceDamage } from '../bindings/SourceDamage';
import type { KpiCell } from '../home/v3/types';
import { formatDps, formatElapsed, formatStat, formatShare } from '../styles/format';
import { damageRank, nonnegative, sourceLabels } from '../runs/damage';
import { dpsMetric, type DpsScope } from '../runs/dps-metric';

/** Convert decimal microseconds without first rounding them to a JS integer. */
export function timestampMs(at: string | null | undefined): number | null {
  if (at == null || !/^\d+$/.test(at)) return null;
  const micros = BigInt(at);
  const value = Number(micros / 1000n) + Number(micros % 1000n) / 1000;
  return Number.isFinite(value) ? value : null;
}
/** A live timeline advances between content changes that republish a profile. */
export function presenceEnd(lastSeen: string | null | undefined, timeline: PlayerTimeline | null | undefined, live: boolean): number | null {
  const observed = timestampMs(lastSeen);
  if (observed === null) return null;
  return live ? Math.max(observed, timestampMs(timeline?.buckets.at(-1)?.until) ?? observed) : observed;
}
export function durationText(ms: number | null): string {
  return ms !== null && Number.isFinite(ms) && ms >= 0 ? formatElapsed('0', String(Math.round(ms * 1000))) : '—';
}
function width(from: string, until: string): number {
  return /^\d+$/.test(from) && /^\d+$/.test(until) ? Math.max(0, Number(BigInt(until) - BigInt(from)) / 1000) : 0;
}
export function activeMs(t: PlayerTimeline): number {
  return t.buckets.reduce((sum, bucket, i) => sum + (Number.isFinite(t.points[i]) && t.points[i] > 0 ? width(bucket.from, bucket.until) : 0), 0);
}
export function idleGaps(t: PlayerTimeline, present: { from: number; until: number }, minMs = 30_000): { from: number; until: number }[] {
  const gaps: { from: number; until: number }[] = [];
  let pending: { from: number; until: number } | null = null;
  const flush = () => {
    if (pending && pending.until - pending.from >= minMs) gaps.push(pending);
    pending = null;
  };
  if (t.disabled) return gaps;
  for (const [i, bucket] of t.buckets.entries()) {
    const start = timestampMs(bucket.from), end = timestampMs(bucket.until);
    const from = Math.max(start ?? Infinity, present.from), until = Math.min(end ?? -Infinity, present.until);
    if (t.points[i] !== 0 || !(bucket.party_total > 0) || until <= from) { flush(); continue; }
    if (pending && pending.until === from) pending.until = until;
    else { flush(); pending = { from, until }; }
  }
  flush();
  return gaps;
}
/** Half-open window; retain intersecting whole buckets, as stored context does.
 * Clipping bounds while retaining damage would invent an inflated edge DPS. */
export function sliceTimeline(t: PlayerTimeline, from: number, until: number): PlayerTimeline {
  const indices = t.buckets.flatMap((bucket, i) => {
    const start = timestampMs(bucket.from), end = timestampMs(bucket.until);
    return until > from && start !== null && end !== null && end > from && start < until ? [i] : [];
  });
  return { ...t, buckets: indices.map(i => ({ ...t.buckets[i] })), points: indices.map(i => t.points[i]) };
}
export function timelineFromFightSeries(fight: FightView, playerId: string, allowUnknownParty = false): PlayerTimeline | null {
  const rows = fight.player_series ?? [], matches = rows.filter(row => row.player_id === playerId);
  // Fight series keep only top five plus local; the bucket total includes everyone.
  const completeParty = fight.players.filter(player => player.damage > 0).length <= rows.length;
  if (!completeParty && !allowUnknownParty) return null;
  if (matches.length !== 1 || matches[0].points.length !== fight.buckets.length) return null;
  // With no complete active count, the chart treats the denominator as unknown
  // and renders gaps / a dash for party average, while retaining the player line.
  return {
    buckets: fight.buckets.map((bucket, i) => ({ from: bucket.from, until: bucket.until, party_total: bucket.total,
      party_active: completeParty ? rows.filter(row => row.points.length === fight.buckets.length && row.points[i] > 0).length : 0 })),
    points: [...matches[0].points], compacted: fight.buckets.some(bucket => width(bucket.from, bucket.until) > 1000), disabled: false,
  };
}
export function gearAt(log: GearChange[], at: number): EquipmentView[] | null {
  let latest: GearChange | null = null, latestAt = -Infinity;
  for (const row of log) {
    const time = timestampMs(row.at);
    if (time !== null && time <= at && time >= latestAt) { latest = row; latestAt = time; }
  }
  return latest?.equipment ?? null;
}
export function swapsIn(log: GearChange[], from: number, until: number): GearChange[] {
  return log.filter(row => {
    const at = timestampMs(row.at);
    return at !== null && at >= from && at < until;
  }).sort((a, b) => timestampMs(a.at)! - timestampMs(b.at)!);
}
export function vsAverage(dps: number | null, avg: number | null): number | null {
  const ratio = nonnegative(dps) !== null && nonnegative(avg) !== null && avg! > 0 ? dps! / avg! : null;
  return ratio !== null && Number.isFinite(ratio) ? ratio : null;
}
export function vsTop(dps: number | null, top: number | null): number | null { return vsAverage(dps, top); }
const percent = new Intl.NumberFormat('en-US', { style: 'percent', maximumFractionDigits: 1 });
const ratioText = (ratio: number | null) => ratio !== null && Number.isFinite(ratio) ? percent.format(ratio) : '—';
function bestSource(sources: SourceDamage[]): SourceDamage | null {
  return sources.filter(row => nonnegative(row.biggest_hit) !== null).sort((a, b) => b.biggest_hit! - a.biggest_hit!)[0] ?? null;
}
function sourceName(source: SourceDamage | null): string {
  return source ? source.item_type === null ? sourceLabels[source.kind] : `Item ${source.item_type}` : '—';
}
function activeCell(t: PlayerTimeline | null, presentMs: number | null): KpiCell {
  const active = t && !t.disabled ? activeMs(t) : null;
  const ratio = active !== null && presentMs !== null && presentMs > 0 ? Math.min(1, active / presentMs) : null;
  return { key: 'active', label: 'Active', value: `${ratio !== null && t?.compacted ? 'approx. ' : ''}${ratioText(ratio)}`,
    sub: `${active !== null && t?.compacted && ratio === null ? 'approx. ' : ''}${durationText(active)} of ${durationText(presentMs)} dealing dmg` };
}
function damageCells(input: { basis: DpsScope; local: boolean; damage: number | null; dps: number | null; topDps: number | null; topName: string | null;
  average: number | null; total: number; rank: number | null; count: number; share: number | null;
  hits: number | null; best: number | null; source: SourceDamage | null }): KpiCell[] {
  const avgRatio = vsAverage(input.dps, input.average), delta = avgRatio === null ? null : avgRatio - 1;
  return [
    { key: 'damage', label: 'Damage', value: formatStat(nonnegative(input.damage)), sub: `of ${formatStat(nonnegative(input.total))} party`, tone: input.local ? 'fame' : null },
    { key: 'dps', ...dpsMetric(input.basis), value: formatDps(input.dps), sub: delta === null ? '—' : `${delta >= 0 ? '+' : '−'}${ratioText(Math.abs(delta))} vs avg`, subTone: delta === null || delta === 0 ? null : delta > 0 ? 'good' : 'bad' },
    { key: 'vs_top', label: 'vs Top', value: ratioText(vsTop(input.dps, input.topDps)), sub: `${input.topName ?? '—'} ${formatDps(input.topDps)}` },
    { key: 'rank', label: 'Rank', value: damageRank(input.rank) === '—' ? '—' : `#${damageRank(input.rank)}`, of: String(input.count), sub: `${formatShare(input.share)} share` },
    { key: 'hits', label: 'Hits', value: formatStat(nonnegative(input.hits)), sub: `avg ${formatStat(nonnegative(input.hits) !== null && input.hits! > 0 && nonnegative(input.damage) !== null ? input.damage! / input.hits! : null)}` },
    { key: 'best_hit', label: 'Best hit', value: formatStat(nonnegative(input.best)), sub: sourceName(input.source) },
  ];
}
export function runPlayerKpis(v: RunPlayerView): KpiCell[] {
  const p = v.player, best = p ? bestSource(v.sources) : null;
  const deaths = v.deaths.length || (p ? nonnegative(p.deaths) : v.profile ? 0 : null);
  const from = timestampMs(v.profile?.first_seen), until = presenceEnd(v.profile?.last_seen, v.timeline, v.run.ended == null);
  const places = [...new Set(v.deaths.map(death => death.fight_id == null ? 'outside boss fights'
    : v.fights.find(f => f.fight_id === death.fight_id)?.target_name ?? '—'))];
  return [...damageCells({ basis: 'whole_run', local: p?.is_local ?? v.profile?.is_local ?? false, damage: p?.damage ?? null, dps: p?.dps ?? null, topDps: v.party.top_dps ?? null,
    topName: v.party.top_name ?? null, average: v.party.average_dps ?? null, total: v.party.total_damage,
    rank: p?.rank ?? null, count: v.party.players_with_damage, share: p?.share ?? null, hits: p?.hits ?? null,
    best: best?.biggest_hit ?? null, source: best }),
    { key: 'deaths', label: 'Deaths', value: formatStat(deaths), sub: places.join(', ') || '—', tone: deaths ? 'bad' : null },
    activeCell(v.timeline ?? null, from !== null && until !== null && until >= from ? until - from : null)];
}
export function fightPlayerKpis(f: FightView, p: PlayerDamage | null, deaths: PlayerDeathAt[], t: PlayerTimeline | null,
  presentMs: number | null = null, profile: PlayerProfileView | null = null): KpiCell[] {
  const deathCount = deaths.length || (p ? nonnegative(p.deaths) : profile ? 0 : null);
  const damaging = f.players.filter(row => row.damage > 0), rates = damaging.map(row => nonnegative(row.dps));
  const average = rates.length && rates.every((rate): rate is number => rate !== null) ? rates.reduce((sum, rate) => sum + rate, 0) / rates.length : null;
  const top = average !== null ? [...damaging].sort((a, b) => b.dps! - a.dps!)[0] : null;
  return [...damageCells({ basis: f.dps_basis, local: p?.is_local ?? profile?.is_local ?? false, damage: p?.damage ?? null, dps: p?.dps ?? null, topDps: top?.dps ?? null, topName: top?.name ?? null,
    average, total: f.total_damage, rank: p?.rank ?? null, count: damaging.length, share: p?.share ?? null, hits: p?.hits ?? null,
    best: p?.biggest_hit ?? null, source: p ? bestSource(p.sources) : null }),
    { key: 'deaths', label: 'Deaths', value: formatStat(deathCount), sub: deaths.length ? 'This fight' : '—', tone: deathCount ? 'bad' : null },
    activeCell(t, presentMs)];
}
export type ActivityItem = { at: number; kind: 'entered' | 'first-hit' | 'idle' | 'death' | 'departed' | 'last-hit'; label: string; until?: number };
export function activityItems(input: { firstSeen: number | null; firstHit: number | null; lastHit: number | null;
  deaths: PlayerDeathAt[]; departedAt: number | null; gaps: { from: number; until: number }[] }): ActivityItem[] {
  const items: ActivityItem[] = [];
  const add = (at: number | null, kind: ActivityItem['kind'], label: string) => {
    if (at !== null && Number.isFinite(at)) items.push({ at, kind, label });
  };
  add(input.firstSeen, 'entered', 'Entered run');
  add(input.firstHit, 'first-hit', 'First hit');
  for (const gap of input.gaps) if (Number.isFinite(gap.from) && gap.until > gap.from) items.push({ at: gap.from, until: gap.until, kind: 'idle', label: `Idle ${durationText(gap.until - gap.from)} (no damage)` });
  for (const death of input.deaths) add(timestampMs(death.at), 'death', 'Died');
  add(input.departedAt, 'departed', 'Departed run');
  add(input.lastHit, 'last-hit', 'Last hit');
  return items.sort((a, b) => a.at - b.at);
}

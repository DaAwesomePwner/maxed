import { characterMaxed } from '../../shell/vitals';
import type { ChainStep } from '../../bindings/ChainStep';
import type { CharacterView } from '../../bindings/CharacterView';
import type { DamageBucket } from '../../bindings/DamageBucket';
import type { DeathDetail } from '../../bindings/DeathDetail';
import type { DropView } from '../../bindings/DropView';
import type { FightView } from '../../bindings/FightView';
import type { HomeDungeonStreak } from '../../bindings/HomeDungeonStreak';
import type { Outcome } from '../../bindings/Outcome';
import type { RecapChain } from '../../bindings/RecapChain';
import type { RunFightSummary } from '../../bindings/RunFightSummary';
import { formatDps, formatElapsed, formatShare, formatStat } from '../../styles/format';
import { knownNumber } from '../evidence';
import { realmKillRows } from '../realm';
import { rankedDrops } from '../v3/loot-rank';
import { signedPercent } from './hud';
import type { RunRecapData, VisitRecapData } from './recap-reads';
import { eventRows, type EventRow } from './realm-events';
import { partySeries } from './you-vs-party';

export type Tone = 'up' | 'down' | 'neutral';
export type RecapTileKey = 'dps' | 'party' | 'rank' | 'best-hit' | 'fame' | 'events' | 'best-rank';
export interface RecapTile { key: RecapTileKey; label: string; value: string; detail: string | null; tone: Tone | null; partial: boolean; comparison: 'usual' | 'party' | null }
export interface FightRow {
  fightId: string; targetType: number | null; name: string | null; kill: string;
  dps: number | null; best: boolean; vsParty: number | null; rank: number | null; players: number;
  buckets: DamageBucket[] | null;
}
export type RecapChip = { kind: 'streak' | 'best-dps' | 'pb' | 'milestone' | 'new-items'; text: string; itemTypes?: number[] };
export interface ChainView { label: string; steps: ChainStep[]; totals: { time: string; bags: number; whites: number } }
export interface RunRecapView {
  kind: 'run'; runId: string; title: string; outcome: Outcome | null; outcomeLabel: string; clearTime: string | null;
  pb: boolean; meta: string[]; chain: ChainView | null; best: { drop: DropView } | null; streakNote: string | null;
  streak: HomeDungeonStreak | null; streakEnded: number | null; tiles: RecapTile[]; fights: FightRow[];
  fightsPartial: boolean; chips: RecapChip[];
  party: { top: Array<{ name: string; share: string }>; more: number;
    rows: Array<{ rank: number | null; name: string | null; classId: number | null; share: number | null; isLocal: boolean }> };
  drops: DropView[]; death: DeathDetail | null; died: { at: string | null; elapsed: string } | null;
  character: Pick<CharacterView, 'name' | 'class_id' | 'maxed'> | null;
}
export interface VisitRecapView {
  kind: 'visit'; visitId: string; characterId: string | null; title: string; timeHere: string; meta: string[]; chain: ChainView | null;
  best: { drop: DropView } | null; tiles: RecapTile[]; events: VisitEventRow[]; drops: DropView[];
  dropsPartial: boolean; chips: RecapChip[];
}
export type VisitEventRow = EventRow & { kill: string | null; vsParty: number | null; buckets: DamageBucket[] | null; best: boolean };

export const fightRank = (rank: number | null, players: number | null) => rank === null ? '—' : `#${rank} of ${players ?? '—'}`;

export const timestamp = (at: string | null | undefined) => at != null && /^\d+$/.test(at) ? BigInt(at) : null;
function duration(start: string | null | undefined, end: string | null | undefined): bigint | null {
  const from = timestamp(start), until = timestamp(end);
  return from !== null && until !== null && until >= from ? until - from : null;
}
const elapsed = (start: string, end: string | null) => end === null ? '—' : formatElapsed(start, end);
const microsText = (value: number) => formatElapsed('0', String(value));
function endedText(at: string | null, timeZone?: string): string {
  const time = timestamp(at), date = time === null ? null : new Date(Number(time / 1000n));
  return `ended ${date && Number.isFinite(date.getTime())
    ? new Intl.DateTimeFormat('en-GB', { timeZone, hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).format(date) : '—'}`;
}
function ordinal(n: number): string {
  const last = n % 100;
  return `${n}${last >= 11 && last <= 13 ? 'th' : ({ 1: 'st', 2: 'nd', 3: 'rd' }[n % 10] ?? 'th')}`;
}
export function indefiniteArticle(n: number): 'a' | 'an' {
  return /^(8|11$|18$|11\d{3}$|18\d{3}$)/.test(String(n)) ? 'an' : 'a';
}
function uniqueSelf<T extends { is_local: boolean }>(players: T[]): T | null {
  const locals = players.filter(row => row.is_local);
  return locals.length === 1 ? locals[0] : null;
}
function ratio(local: number | null | undefined, party: number | null | undefined): number | null {
  const you = knownNumber(local), avg = knownNumber(party);
  if (you === null || avg === null || avg === 0) return null;
  const value = you / avg - 1;
  return Number.isFinite(value) ? value : null;
}
export function tone(value: number | null): Tone | null {
  return value === null ? null : Math.round(Math.abs(value) * 100) === 0 ? 'neutral' : value > 0 ? 'up' : 'down';
}
const tile = (key: RecapTileKey, label: string, value: string, detail: string | null = null, tone: Tone | null = null, partial = false,
  comparison: RecapTile['comparison'] = null): RecapTile => ({ key, label, value, detail, tone, partial, comparison });
const bossTier = (fight: RunFightSummary) => fight.target_kind === 'boss' || fight.target_kind === 'event_boss';
function chronological(a: RunFightSummary, b: RunFightSummary): number {
  const left = timestamp(a.started), right = timestamp(b.started);
  return left === right ? a.id.localeCompare(b.id) : left === null ? 1 : right === null ? -1 : left < right ? -1 : 1;
}

/** Subtract integer timestamps before converting spans, retaining microsecond differences. */
function weighted(fights: RunFightSummary[], requireParty: boolean): { local: number | null; party: number | null } {
  let localSum = 0, partySum = 0, weight = 0;
  for (const fight of fights) {
    const local = knownNumber(fight.local_dps), party = knownNumber(fight.party?.avg_dps);
    const span = duration(fight.first_hit, fight.last_hit);
    if (local === null || span === null || span === 0n || (requireParty && party === null)) continue;
    const w = Number(span);
    if (!Number.isFinite(w)) continue;
    localSum += local * w;
    if (requireParty && party !== null) partySum += party * w;
    weight += w;
  }
  return { local: weight > 0 ? knownNumber(localSum / weight) : null,
    party: requireParty && weight > 0 ? knownNumber(partySum / weight) : null };
}
function partyTile(fights: RunFightSummary[], partial = false): RecapTile {
  const values = weighted(fights, true), delta = ratio(values.local, values.party);
  return tile('party', 'vs party avg', signedPercent(delta), values.party === null ? null : `party avg ${formatDps(values.party)}`, tone(delta), partial, 'party');
}
function chainView(chain: RecapChain | null): ChainView | null {
  if (!chain?.steps.length) return null;
  return { label: `${chain.steps.map(step => step.name).join(' → ')} · inferred`, steps: chain.steps,
    totals: { time: elapsed(chain.steps[0].started, chain.steps.at(-1)!.ended),
      bags: chain.steps.reduce((sum, step) => sum + step.bags, 0), whites: chain.steps.reduce((sum, step) => sum + step.whites, 0) } };
}
function bestDrop(drops: DropView[]): { drop: DropView } | null {
  const drop = rankedDrops(drops)[0];
  return drop ? { drop } : null;
}
function killText(fight: FightView | undefined, usual: number | null): string {
  if (!fight) return '—';
  const kill = fight.kill ? duration(fight.started, fight.kill.at) : null;
  if (kill === null) return usual === null ? '—' : `usual ${microsText(usual)}`;
  const time = formatElapsed('0', String(kill));
  if (usual === null) return time;
  const delta = Number(kill) - usual;
  return Math.abs(delta) < 1_000_000 ? `${time} · usual ${microsText(usual)}`
    : `${time} · ${Math.floor(Math.abs(delta) / 1_000_000)} s ${delta < 0 ? 'faster' : 'slower'} than usual`;
}

export function runRecapView(data: RunRecapData, timeZone?: string): RunRecapView {
  const { story, extras, profile, baselines } = data, { run, totals } = story;
  const self = totals?.local?.is_local ? totals.local : uniqueSelf(totals?.players ?? []);
  const bosses = story.fights.filter(bossTier).sort(chronological);
  const details = new Map((story.boss_fights ?? []).map(fight => [fight.id, fight]));
  const profiles = new Map(profile?.bosses.map(boss => [boss.target_type, boss]) ?? []);
  const usuals = new Map(baselines?.rows.map(boss => [boss.target_type, boss]) ?? []);
  const bestIds = new Set(extras?.best_dps.map(record => record.fight_id) ?? []);
  const fights = bosses.map<FightRow>(row => {
    const targetType = row.target_type ?? row.boss_type, detail = details.get(row.id);
    const learned = targetType === null ? null : profiles.get(targetType), baseline = targetType === null ? null : usuals.get(targetType);
    const usual = (learned && learned.kill_samples >= 3 ? knownNumber(learned.median_kill_micros) : null)
      ?? (baseline && baseline.kill_samples >= 3 ? knownNumber(baseline.median_kill_micros) : null);
    return { fightId: row.id, targetType, name: row.target_name, kill: killText(detail, usual),
      dps: knownNumber(row.local_dps), best: bestIds.has(row.id), vsParty: ratio(row.local_dps, row.party?.avg_dps),
      rank: knownNumber(row.local_rank), players: row.player_count,
      buckets: detail && partySeries(detail.buckets).partyRecorded ? detail.buckets : null };
  });
  let bestHit: { value: number; name: string | null } | null = null;
  if (self) for (const fight of story.boss_fights ?? []) {
    const hit = knownNumber(uniqueSelf(fight.players)?.biggest_hit);
    if (hit !== null && (bestHit === null || hit > bestHit.value)) {
      bestHit = { value: hit, name: story.fights.find(row => row.id === fight.id)?.target_name ?? null };
    }
  }
  const dpsDelta = ratio(self?.dps, profile?.usual_run_dps), dpsTone = tone(dpsDelta);
  const dpsDetail = profile === null ? 'vs your usual —' : profile.usual_run_dps === null
    ? `learning (${profile.run_dps_samples} runs)` : dpsDelta === null ? 'vs your usual —'
    : `${dpsTone === 'up' ? '▲ ' : dpsTone === 'down' ? '▼ ' : ''}${signedPercent(Math.abs(dpsDelta)).replace('+', '')} vs your usual`;
  const partial = totals?.partial ?? false;
  const fame = knownNumber(totals?.fame_delta);
  const deaths = totals?.players.reduce((sum, row) => sum + row.deaths, 0);
  const tiles = [tile('dps', 'Your DPS', formatDps(self?.dps), dpsDetail, dpsTone, partial, 'usual'),
    partyTile(self ? bosses : [], bosses.some(row => row.partial)),
    tile('rank', 'Your rank', self?.rank == null ? '—' : `#${self.rank} of ${totals!.players.length}`,
      self?.share != null ? `${formatShare(self.share)} of damage` : null, null, partial),
    tile('best-hit', 'Best hit', formatStat(bestHit?.value ?? null), bestHit?.name ?? null, null, story.boss_fights_limited === true),
    tile('fame', 'Fame', fame === null ? '—' : `+${fame}`,
      deaths === undefined ? null : `${deaths} ${deaths === 1 ? 'death' : 'deaths'}`)];
  const meta: string[] = [];
  if (extras?.clear_number != null) meta.push(`Clear #${extras.clear_number}`);
  if (extras && extras.in_a_row >= 2) meta.push(`${ordinal(extras.in_a_row)} ${run.dungeon} in a row tonight`);
  meta.push(endedText(run.ended, timeZone));
  const dry = extras?.dry_streak_end;
  const streakNote = !dry ? null : dry.had_previous_white ? `Ended ${indefiniteArticle(dry.runs)} ${dry.runs}-run dry streak`
    : `No ${run.dungeon} white in ${dry.runs} runs before this`;
  const chips: RecapChip[] = [];
  if (dry?.had_previous_white) chips.push({ kind: 'streak', text: streakNote! });
  for (const record of extras?.best_dps ?? []) {
    const name = story.fights.find(row => row.id === record.fight_id)?.target_name ?? '—';
    chips.push({ kind: 'best-dps', text: `Best DPS on ${name}` });
  }
  const pb = extras?.pb_clear;
  if (pb) {
    const gain = pb.previous_best_micros - pb.clear_micros;
    chips.push({ kind: 'pb', text: `PB clear ${microsText(pb.clear_micros)} · ${gain < 1_000_000 ? 'under 1 s' : microsText(gain)} faster` });
  }
  for (const kind of ['first_clear', 'clear_count', 'first_white', 'new_items'] as const) {
    for (const milestone of extras?.milestones ?? []) {
      if (milestone.kind !== kind) continue;
      switch (milestone.kind) {
        case 'first_clear': chips.push({ kind: 'milestone', text: `First ${run.dungeon} clear` }); break;
        case 'clear_count': chips.push({ kind: 'milestone', text: `${ordinal(milestone.count)} ${run.dungeon} clear` }); break;
        case 'first_white': chips.push({ kind: 'milestone', text: `First ${run.dungeon} white` }); break;
        case 'new_items': chips.push({ kind: 'new-items', text: `New in collection${milestone.more > 0 ? ` · +${milestone.more} more` : ''}`,
          itemTypes: milestone.item_types }); break;
      }
    }
  }
  const players = [...(totals?.players ?? [])].sort((a, b) =>
    (a.rank ?? Infinity) - (b.rank ?? Infinity) || b.damage - a.damage);
  const rows = players.map(row => ({ rank: row.rank ?? null, name: row.name ?? null, classId: row.class_id ?? null,
    share: row.share ?? null, isLocal: row.is_local }));
  return { kind: 'run', runId: run.id, title: run.dungeon, outcome: run.outcome,
    outcomeLabel: run.outcome === 'cleared' ? 'CLEARED' : run.outcome === 'died' ? 'DIED' : run.outcome === 'left' ? 'LEFT EARLY' : 'Outcome not established',
    clearTime: run.outcome === 'cleared' ? elapsed(run.started, run.completed_at) : null, pb: pb != null, meta,
    chain: chainView(extras?.chain ?? null), best: bestDrop(story.drops), streakNote, streak: data.streak, streakEnded: dry?.runs ?? null,
    tiles, fights, fightsPartial: story.boss_fights_limited === true || bosses.some(row => row.partial), chips,
    party: { rows, top: rows.slice(0, 3).map(row => ({ name: row.name ?? 'Unknown', share: formatShare(row.share) })), more: Math.max(0, players.length - 3) },
    drops: story.drops, death: run.death ?? null,
    character: story.character ? { name: story.character.name, class_id: story.character.class_id, maxed: characterMaxed(story.character) } : null,
    died: run.outcome === 'died' ? { at: run.died_at, elapsed: elapsed(run.started, run.died_at) } : null };
}

export function visitRecapView(data: VisitRecapData, timeZone?: string, liveTitle?: string | null): VisitRecapView {
  const { visit } = data;
  const periods = visit.periods.map(period => ({ start: timestamp(period.started), end: timestamp(period.ended), open: period.ended === null }));
  const periodOf = (at: string): number | null => {
    const time = timestamp(at);
    if (time === null) return null;
    const index = periods.findIndex(period => period.start !== null && time >= period.start
      && (period.open || (period.end !== null && time < period.end)));
    return index < 0 ? null : index;
  };
  const kills = realmKillRows({ fights: visit.boss_fights, events: visit.announcements,
    self: fight => 'self' in fight ? fight.self : uniqueSelf(fight.players), inScope: at => periodOf(at) !== null, periodOf });
  const details = new Map(visit.boss_fights.map(fight => [`fight:${fight.id}`, fight]));
  const summaries = new Map(visit.fights.map(fight => [`fight:${fight.id}`, fight]));
  const usuals = new Map(data.baselines?.rows.map(boss => [boss.target_type, boss]) ?? []);
  const bestIds = new Set(visit.best_dps.map(record => record.fight_id));
  const events = eventRows(kills, null, visit.drops).map<VisitEventRow>(row => {
    if (row.state !== 'killed') return { ...row, kill: null, vsParty: null, buckets: null, best: false };
    const detail = details.get(row.key), summary = summaries.get(row.key);
    const baseline = row.targetType === null ? null : usuals.get(row.targetType);
    const usual = baseline && baseline.kill_samples >= 3 ? knownNumber(baseline.median_kill_micros) : null;
    return { ...row, best: detail !== undefined && bestIds.has(detail.id),
      players: summary?.player_count ?? null, vsParty: ratio(summary?.local_dps, summary?.party?.avg_dps),
      kill: killText(detail, usual), buckets: detail && partySeries(detail.buckets).partyRecorded ? detail.buckets : null };
  });
  const announced = events.filter(row => row.state === 'announced').length;
  const bestRank = visit.fights.filter(fight => fight.target_type != null && fight.target_kind != null && fight.target_kind !== 'ordinary')
    .sort(chronological).reduce<RunFightSummary | null>((best, fight) =>
    fight.local_rank !== null && (best === null || fight.local_rank < best.local_rank!) ? fight : best, null);
  const partial = visit.fights.some(fight => fight.partial);
  const tiles = [tile('dps', 'Your DPS', formatDps(weighted(visit.fights, false).local), null, null, partial), partyTile(visit.fights, partial),
    tile('events', 'Events killed', String(events.filter(row => row.state === 'killed').length), announced ? `+${announced} announced` : null,
      null, visit.boss_fights_limited || visit.announcements_limited),
    tile('best-rank', 'Best rank', bestRank === null ? '—' : `#${bestRank.local_rank} of ${bestRank.player_count}`),
    tile('fame', 'Fame this visit', knownNumber(visit.fame_delta) === null ? '—' : `+${visit.fame_delta}`)];
  const spans = visit.periods.map(period => duration(period.started, period.ended));
  const timeHere = spans.length === 0 || spans.some(span => span === null) ? '—'
    : formatElapsed('0', String(spans.reduce<bigint>((sum, span) => sum + span!, 0n)));
  return { kind: 'visit', visitId: visit.visit_id, characterId: visit.character_id, title: liveTitle != null && liveTitle !== '—' ? liveTitle : visit.title,
    timeHere, meta: [endedText(visit.periods.at(-1)?.ended ?? null, timeZone)], chain: chainView(visit.chain),
    best: bestDrop(visit.drops), tiles, events, drops: visit.drops, dropsPartial: visit.drops_limited,
    chips: visit.best_dps.map(record => ({ kind: 'best-dps',
      text: `Best DPS on ${visit.fights.find(row => row.id === record.fight_id)?.target_name ?? '—'}` })) };
}

function addDropTypes(types: Set<number>, view: Pick<RunRecapView, 'drops' | 'best' | 'chain'>): void {
  for (const drop of [...view.drops, ...(view.best ? [view.best.drop] : [])]) {
    types.add(drop.bag_type);
    for (const item of drop.contents) types.add(item.item_type);
    if (drop.contents_known) for (const type of drop.items) types.add(type);
  }
  for (const step of view.chain?.steps ?? []) for (const type of step.item_types) types.add(type);
}

/** Saved evidence can contain types absent from the current live window. */
export function recapVisualTypes(view: RunRecapView): number[] {
  const types = new Set<number>();
  if (view.death?.killed_by_type != null) types.add(view.death.killed_by_type);
  if (view.character?.class_id != null) types.add(view.character.class_id);
  for (const fight of view.fights) if (fight.targetType !== null) types.add(fight.targetType);
  addDropTypes(types, view);
  for (const chip of view.chips) for (const type of chip.itemTypes ?? []) types.add(type);
  return [...types];
}

/** Realm recaps retain their own event, loot and chain visuals after leaving. */
export function visitRecapVisualTypes(view: VisitRecapView): number[] {
  const types = new Set<number>([1796]);
  for (const event of view.events) {
    if (event.targetType !== null) types.add(event.targetType);
    for (const bag of event.bags) types.add(bag.bagType);
  }
  addDropTypes(types, view);
  return [...types];
}

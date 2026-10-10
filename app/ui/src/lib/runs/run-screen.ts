import type { RunStory } from '../bindings/RunStory';
import type { DungeonBest } from '../bindings/DungeonBest';
import type { RunFightSummary } from '../bindings/RunFightSummary';
import type { KpiCell } from '../home/v3/types';
import { dungeonEvents, dungeonTimeline, type DungeonMarker } from '../home/dungeon';
import { formatClock, formatDps, formatElapsed, formatShare, formatStat, formatTimeDelta } from '../styles/format';
import { bossTier, estimatedClear } from './story';

export function runKpis(story: RunStory, best: DungeonBest | null): KpiCell[] {
  const { run, totals, comparison } = story, local = totals?.local;
  // Same rule as the store's clear metrics: a clear is known before the run exits.
  const clear = run.outcome === 'cleared' && run.completed_at !== null && BigInt(run.completed_at) > BigInt(run.started)
    ? formatElapsed(run.started, run.completed_at) : '—';
  const baseline = best?.dungeon === run.dungeon ? best.best_clear_micros : null;
  const delta = clear !== '—' && baseline !== null && Number.isSafeInteger(baseline) && baseline > 0
    ? BigInt(run.completed_at!) - BigInt(run.started) - BigInt(baseline) : null;
  let sub = delta !== null ? `PB ${formatElapsed('0', String(baseline))} · ${formatTimeDelta(delta)}` : 'PB —';
  if (clear !== '—' && comparison && comparison.previous_clears >= 3 && comparison.mean_micros > 0
    && Number.isFinite(comparison.mean_micros) && Number.isFinite(comparison.change_fraction)
    && !estimatedClear(story) && BigInt(run.completed_at!) > BigInt(run.started)) {
    sub += comparison.change_fraction === 0 ? ' · unchanged from avg' : ` · ${formatStat(Math.abs(comparison.change_fraction) * 100)}% ${comparison.change_fraction < 0 ? 'faster' : 'slower'} than avg`;
  }
  const events = dungeonEvents(story.events ?? [], run.completed_at);
  const partial = !run.complete_capture || run.issues.length > 0 || story.events_limited || totals?.partial;
  const count = (type: 'player_left' | 'player_death') => {
    const observed = events.filter(event => event.kind.type === type).length;
    return story.events == null || (partial && observed === 0) ? '—' : `${story.events_limited ? '~' : ''}${observed}`;
  };
  const fame = totals?.fame_delta;
  return [
    { key: 'clear', label: 'Clear time', value: clear, sub, tone: delta !== null && delta > 0n ? 'bad' : null,
      subTone: delta === null ? null : delta > 0n ? 'bad' : 'good' },
    { key: 'damage', label: 'Damage', value: formatStat(totals?.total_damage ?? null),
      sub: totals ? `${totals.fights} fights · ${totals.kills} kills` : '—' },
    { key: 'share', label: 'Your share', value: formatShare(local?.share ?? null),
      sub: local ? `rank ${formatStat(local.rank ?? null)} of ${totals?.players.length} · ${formatDps(local.dps)} DPS` : '—' },
    { key: 'party', label: 'Party', value: run.party_size === null ? '—' : `${run.party_size_estimated ? '~' : ''}${formatStat(run.party_size)}`,
      sub: `${count('player_left')} left · ${count('player_death')} deaths` },
    { key: 'fame', label: 'Run fame', value: fame == null || !Number.isFinite(fame) ? '—' : `${fame > 0 ? '+' : ''}${formatStat(fame)}`, tone: 'fame', sub: 'this run' },
  ];
}
export function runMarkers(story: RunStory): DungeonMarker[] {
  // Saved boss membership also includes legacy bosses, quests and minibosses.
  return dungeonTimeline((story.boss_fights ?? []).map(fight => ({ ...fight, target_type: fight.target_type ?? fight.boss_type, target_kind: 'boss' })), story.drops, dungeonEvents(story.events ?? [], story.run.completed_at));
}
export function otherTargets(story: RunStory): RunFightSummary[] {
  return story.fights.filter(fight => !bossTier(fight) && fight.target_type !== null);
}
function runDate(story: RunStory, weekday = false): string {
  const date = new Date(Number(BigInt(story.run.started) / 1000n));
  return Number.isFinite(date.getTime()) ? date.toLocaleDateString('en-US', { ...(weekday ? { weekday: 'short' as const } : {}), month: 'short', day: 'numeric' }) : '—';
}
export function runHeaderLine(story: RunStory, className: string | null): string {
  const character = [story.character?.name, className].filter(Boolean).join(', ');
  return [runDate(story, true), formatClock(story.run.started, 'en-US', { second: undefined }), character,
    story.run.context?.realm_name, story.run.party_size !== null ? `party of ${story.run.party_size_estimated ? '~' : ''}${story.run.party_size}` : null].filter(Boolean).join(' · ');
}
export function runCrumbs(story: RunStory): Array<{ label: string; href?: string | null }> {
  return [{ label: 'Runs', href: '#/runs' }, { label: `${story.run.dungeon} · ${runDate(story)}` }];
}

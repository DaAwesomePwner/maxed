<script lang="ts">
  import type { LiveSnapshot } from '../../bindings/LiveSnapshot';
  import type { DungeonBest } from '../../bindings/DungeonBest';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import { objectVisual } from '../../bridge/visual-index';
  import { spriteUrl } from '../../bridge/visuals';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { debugMode } from '../../debug/debug';
  import { routeHref } from '../../navigation/routes';
  import { fightPlayerHref, runPlayerHref } from '../../players/links';
  import { verifiedSelf } from '../../shell/rail';
  import { formatDps, formatElapsed, formatShare, formatStat } from '../../styles/format';
  import { dungeonObservations, dungeonRunSelf, dungeonTimeline, dungeonView } from '../dungeon';
  import DungeonEvidence from '../DungeonEvidence.svelte';
  import { knownNumber } from '../evidence';
  import { clockTime } from '../nexus';
  import type { HomeVisuals } from '../visuals';
  import type { HomeLiveWindow } from '../v2/contracts';
  import DungeonTimeline from '../v2/DungeonTimeline.svelte';
  import { lootView } from '../v2/loot';
  import BagList from '../v3/BagList.svelte';
  import BestBagCard from '../v3/BestBagCard.svelte';
  import BossHpBar from '../v3/BossHpBar.svelte';
  import DamageTable from '../v3/DamageTable.svelte';
  import LootTally from '../v3/LootTally.svelte';
  import { homeLoot } from '../v3/loot-rank';
  import { bossOnly, damageRows, fightDps, notificationTip, personalBest } from '../v3/screen-data';
  import { bossRows } from './bosses';
  import BossesCard from './BossesCard.svelte';
  import DpsHero from './DpsHero.svelte';
  import DryStreakRow from './DryStreakRow.svelte';
  import { partyAverage, usualFor, type Usual } from './hero';
  import { topWithSelf } from './meter';
  import type { HomeV4ReadState } from './reads';
  import { dungeonGeneration, type EndedStreaks } from './streaks';
  import WhiteBanner from './WhiteBanner.svelte';
  import type { WhiteBannerState } from './white-queue';
  import './screens.css';

  let { snapshot, stale, visuals, portalType, liveWindow, dungeonBests, reads, endedStreaks, white, onSelectFight }: {
    snapshot: LiveSnapshot | null; stale: boolean; visuals: HomeVisuals; portalType: number | null;
    liveWindow: HomeLiveWindow | null; dungeonBests: DungeonBest[] | null; reads: HomeV4ReadState;
    endedStreaks: EndedStreaks; white: WhiteBannerState; onSelectFight?: (id: string) => void;
  } = $props();
  const run = $derived(snapshot?.run ?? null);
  const key = $derived(`${snapshot?.stream_id}:${run?.id}`);
  const view = $derived(dungeonView(snapshot, null, liveWindow));
  const observations = $derived(dungeonObservations(snapshot, liveWindow));
  const bosses = $derived(observations.fights.filter(bossOnly));
  // A retained or manually focused ended fight must not keep the hero in fight mode.
  const current = $derived(run?.ended == null ? bosses.find(fight => fight.ended === null) ?? null : null);
  const heroSelf = $derived(verifiedSelf(snapshot?.character ?? null, current));
  const totals = $derived(run ? snapshot?.run_totals ?? null : null);
  const runSelf = $derived(dungeonRunSelf(snapshot));
  const heroPlayer = $derived(current ? heroSelf : runSelf);
  const profile = $derived(!reads.errors.profile && reads.profile?.dungeon === run?.dungeon ? reads.profile : null);
  const usual = $derived(new Map([...reads.usual].filter(([type]) => !reads.failedUsualTypes.has(type))));
  const history = $derived<Usual>(current ? usualFor(usual.get(current.target_type!))
    : profile && snapshot?.character?.identity_verified ? profile.run_dps_samples >= 3 && knownNumber(profile.usual_run_dps) !== null
      ? { kind: 'value', dps: profile.usual_run_dps!, fights: profile.run_dps_samples }
      : { kind: 'learning', fights: profile.run_dps_samples } : { kind: 'none' });
  const target = $derived(objectVisual(visuals.objects, current?.target_type ?? current?.boss_type));
  const targetName = $derived(target?.name ?? 'Unknown boss');
  const learnedIndex = $derived(profile?.bosses.findIndex(boss => boss.target_type === current?.target_type) ?? -1);
  const bossList = $derived(bossRows(bosses, profile, usual, snapshot ? Number(snapshot.at) : NaN, visuals));
  const portal = $derived(objectVisual(visuals.objects, portalType));
  const resolveSpriteUrl = (sprite: SpriteRef) => spriteUrl(visuals.client_fingerprint, sprite) ?? '';
  const clock = (at: string) => clockTime(at) ?? '—';
  const pb = $derived(personalBest(snapshot, dungeonBests));
  const usualClear = $derived(profile && profile.clear_samples >= 3 ? knownNumber(profile.usual_clear_micros) : null);
  const benchmarks = $derived([usualClear !== null ? `usual clear ${formatElapsed('0', String(usualClear))}` : null,
    pb ? `PB ${pb.best}` : null].filter(Boolean).join(' · '));
  const fame = $derived(totals?.fame_delta != null && Number.isFinite(totals.fame_delta) ? totals.fame_delta : null);

  let selection = $state<{ key: string; whole: boolean } | null>(null);
  const wholeRun = $derived(!current || (selection?.key === key && selection.whole));
  const fight = $derived(wholeRun ? null : current);
  const self = $derived(verifiedSelf(snapshot?.character ?? null, fight));
  // v3 runRows caps at nine, replacing the ninth row. Project the full supplied
  // set with the same damageRows helper so v4 can count the gap before any rank.
  const allRows = $derived(damageRows(wholeRun ? totals?.players ?? [] : fight?.players ?? [], wholeRun ? runSelf : self));
  const rows = $derived(topWithSelf(allRows));
  const hidden = $derived(Math.max(0, allRows.length - rows.filter(row => !('gap' in row)).length));
  const totalDamage = $derived(knownNumber(wholeRun ? totals?.total_damage : fight?.total_damage));
  const otherDamage = $derived(knownNumber(wholeRun ? totals?.unattributed_damage : fight?.unattributed_damage));
  const otherShare = $derived(totalDamage !== null && totalDamage > 0 && otherDamage !== null ? otherDamage / totalDamage : null);
  const detailHref = $derived(fight ? routeHref({ kind: 'fight', id: fight.id, runId: run?.id })
    : run ? routeHref({ kind: 'run', id: run.id }) : null);
  function choose(whole: boolean) {
    selection = { key, whole };
    if (!whole && current) onSelectFight?.(current.id);
  }

  const loot = $derived(homeLoot(observations.drops, visuals));
  const lootEvidence = $derived(lootView({ drops: observations.drops, visuals, partial: observations.partial,
    reasons: observations.reasons, title: 'LOOT THIS RUN' }));
  const stats = $derived(!reads.errors.streak && reads.streak?.generation === dungeonGeneration(snapshot)
    && reads.streak?.value.dungeon === run?.dungeon ? reads.streak.value : null);
  const ended = $derived(run ? endedStreaks.get(run.dungeon) : null);
  const markers = $derived(dungeonTimeline(bosses, observations.drops, observations.events));
</script>

<div class="v4-screen dungeon-screen" data-stale={stale}>
  <header class="v4-header">
    <SpriteImage sprite={portal?.sprite ?? null} sheetUrl={portal?.sprite ? resolveSpriteUrl(portal.sprite) : null}
      fit={40} alt={portal?.name ?? 'Dungeon'} />
    <div class="v4-heading">
      <h2>{run?.dungeon ?? snapshot?.map ?? 'Dungeon'}</h2>
      <p class="v4-meta"><span>Entered {clockTime(run?.started) ?? '—'}</span>
        <span>Players {formatStat(knownNumber(snapshot?.instance?.population))}</span>
        <span title={notificationTip(observations.events, 'player_left') ?? undefined}>Left {formatStat(observations.left.value)}</span>
        <span title={notificationTip(observations.events, 'player_death') ?? undefined}>Deaths {formatStat(observations.deaths.value)}</span>
        <span>Run fame {fame === null ? '—' : `${fame > 0 ? '+' : ''}${formatStat(fame)}`}</span></p>
    </div>
    <div class="v4-clock"><span class="sr-only">Run time</span>
      <strong>{run && snapshot ? formatElapsed(run.started, run.completed_at ?? run.ended ?? snapshot.at) : '—'}</strong>
      {#if benchmarks}<p>{benchmarks}</p>{/if}</div>
    <span class="v4-live" class:stale role="status" aria-live="polite">{stale ? 'Recovering' : run?.ended ? 'Ended' : run ? 'Live' : '—'}</span>
  </header>
  <div class="v4-dungeon-grid">
    <div class="v4-main">
      <div class="v4-hero">
        <div class="v4-hero-content" class:covered={white.current !== null} aria-hidden={white.current !== null} inert={white.current !== null}>
          <DpsHero label={`Your DPS · ${current ? targetName : 'Whole run'}`} live={!stale && current !== null}
            dps={knownNumber(heroPlayer?.dps)} rank={knownNumber(heroPlayer?.rank)} players={(current ? current.players.length : totals?.players.length) || null}
            share={heroPlayer?.share ?? null} bestHit={current ? knownNumber(heroSelf?.biggest_hit) : null}
            party={current ? partyAverage(current.players) : null} usual={history} buckets={current?.buckets ?? []} height={112} />
        </div>
        <WhiteBanner state={white} {visuals} {resolveSpriteUrl} itemHref={type => routeHref({ kind: 'item', type })} />
      </div>
      {#if current}
        <section class="v4-card v4-boss" aria-label="Live boss">
          <SpriteImage sprite={target?.sprite ?? null} sheetUrl={target?.sprite ? resolveSpriteUrl(target.sprite) : null} alt={targetName} fit={32} />
          <div class="v4-boss-detail"><div class="v4-boss-title"><strong>{targetName}</strong>
            {#if learnedIndex >= 0 && profile}<span>Boss {learnedIndex + 1} of {profile.bosses.length}</span>{/if}
            <time>{snapshot ? formatElapsed(current.started, snapshot.at) : '—'}</time></div>
            <BossHpBar hp={current.target_hp ?? null} max={current.max_hp} statusInside />
          </div>
        </section>
      {/if}
      <section class="v4-card v4-meter" aria-label="Party damage">
        <header class="v4-card-heading"><h3>Party damage</h3>
          <div class="v4-meter-tabs" role="group" aria-label="Damage scope">
            <button type="button" aria-pressed={!wholeRun} disabled={!current} onclick={() => choose(false)}>This fight</button>
            <button type="button" aria-pressed={wholeRun} onclick={() => choose(true)}>Whole run</button>
          </div>
        </header>
        <p class="v4-meter-total">{formatStat(totalDamage)} damage{#if fight} · {formatDps(fightDps(fight))} DPS{/if}</p>
        <DamageTable basis={wholeRun ? 'whole_run' : fight?.dps_basis} {rows} total={allRows.length} {visuals} {resolveSpriteUrl}
          rowHref={row => wholeRun ? run ? runPlayerHref(run.id, row) : null : fight ? fightPlayerHref(fight, row, run?.id) : null} />
        <footer class="v4-meter-footer"><p>{#if hidden > 0}+{hidden} more · {/if}{formatShare(otherShare)} of damage not attributed to a player</p>
          {#if detailHref}<a href={detailHref}>{wholeRun ? 'Open run ›' : 'Open fight ›'}</a>{/if}</footer>
      </section>
    </div>
    <div class="v4-side">
      <BossesCard rows={bossList} {visuals} {resolveSpriteUrl} />
      <section class="v4-card v4-loot" aria-label="Loot">
        <h3>Loot</h3>
        <DryStreakRow dungeon={run?.dungeon ?? 'Dungeon'} {stats} {ended} />
        <BestBagCard best={loot.best} {visuals} {resolveSpriteUrl} meta={loot.best ? clock(loot.best.drop.at) : ''} scope={key} />
        <div class="v4-bags">{#key key}<BagList drops={loot.bags} {visuals} {resolveSpriteUrl} {clock} scroll newestFirst />{/key}</div>
        <LootTally {...loot.counts} />
      </section>
    </div>
  </div>
  {#if run && snapshot}{#key key}
    <DungeonTimeline {run} at={snapshot.at} {markers} partial={observations.partial} {visuals} />
  {/key}{/if}
  {#if $debugMode}<DungeonEvidence {stale} {run} {view} {fame} {pb} {snapshot} {observations} {totals} {lootEvidence} {wholeRun} {self} {fight} />{/if}
</div>

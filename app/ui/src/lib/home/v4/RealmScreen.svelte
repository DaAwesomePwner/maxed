<script lang="ts">
  import type { LiveSnapshot } from '../../bindings/LiveSnapshot';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import type { TargetKind } from '../../bindings/TargetKind';
  import { objectVisual } from '../../bridge/visual-index';
  import { spriteUrl } from '../../bridge/visuals';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { debugMode } from '../../debug/debug';
  import { routeHref } from '../../navigation/routes';
  import { fightPlayerHref } from '../../players/links';
  import { scopedFights } from '../../shell/rail';
  import { formatDps, formatElapsed, formatShare, formatStat } from '../../styles/format';
  import { knownNumber } from '../evidence';
  import { clockTime } from '../nexus';
  import { realmView } from '../realm';
  import RealmEvidence from '../RealmEvidence.svelte';
  import type { HomeLiveWindow } from '../v2/contracts';
  import { lootView } from '../v2/loot';
  import BagList from '../v3/BagList.svelte';
  import BestBagCard from '../v3/BestBagCard.svelte';
  import BossHpBar from '../v3/BossHpBar.svelte';
  import DamageTable from '../v3/DamageTable.svelte';
  import { homeLoot } from '../v3/loot-rank';
  import LootTally from '../v3/LootTally.svelte';
  import { damageRows, fightDps } from '../v3/screen-data';
  import type { HomeVisuals } from '../visuals';
  import DpsHero from './DpsHero.svelte';
  import EventsLog from './EventsLog.svelte';
  import { partyAverage, usualFor } from './hero';
  import { topWithSelf } from './meter';
  import type { HomeV4ReadState } from './reads';
  import { eventRows } from './realm-events';
  import VisitTimeline from './VisitTimeline.svelte';
  import WhiteBanner from './WhiteBanner.svelte';
  import type { WhiteBannerState } from './white-queue';
  import './screens.css';

  let { snapshot, stale, window, visuals, reads, white }: {
    snapshot: LiveSnapshot | null; stale: boolean; window: HomeLiveWindow; visuals: HomeVisuals;
    reads: HomeV4ReadState; white: WhiteBannerState;
  } = $props();
  const kinds: Record<TargetKind, string> = { event_boss: 'Event boss', boss: 'Boss', miniboss: 'Miniboss', quest: 'Quest', ordinary: 'Target' };
  let standaloneFocus = $state<{ key: string; id: string | null } | null>(null);
  const key = $derived(`${snapshot?.stream_id}:${snapshot?.session?.id}:${window.visitKey}`);
  const previousId = $derived(window.streamId === snapshot?.stream_id ? window.focusedFightId ?? null
    : standaloneFocus?.key === key ? standaloneFocus.id : null);
  const view = $derived(realmView(snapshot, window, previousId));
  $effect(() => {
    const id = view.fight?.id ?? null;
    if (standaloneFocus?.key !== key || standaloneFocus.id !== id) standaloneFocus = { key, id };
  });
  const instance = $derived(view.realm?.instance);
  const fight = $derived(view.fight);
  const live = $derived(view.fightStatus === 'live');
  const now = $derived(Number(snapshot?.at));
  const fightEnd = $derived(fight?.ended ?? fight?.kill?.at ?? (live ? String(now) : fight?.last_hit) ?? fight?.started ?? '');
  const target = $derived(objectVisual(visuals.objects, fight?.target_type));
  const fallback = $derived(objectVisual(visuals.objects, fight?.boss_type));
  const targetName = $derived(target?.name ?? fallback?.name ?? 'Unknown target');
  const targetSprite = $derived(target?.sprite ?? fallback?.sprite ?? null);
  const quest = $derived(objectVisual(visuals.objects, instance?.quest_target?.object_type));
  const portal = $derived(objectVisual(visuals.objects, 1796));
  const fame = $derived(view.visitFame);
  const usualType = $derived(fight?.target_type ?? fight?.boss_type ?? -1);
  const history = $derived(usualFor(reads.failedUsualTypes.has(usualType) || !snapshot?.character?.identity_verified
    ? undefined : reads.usual.get(usualType)));
  const allRows = $derived(damageRows(fight?.players ?? [], view.self));
  const rows = $derived(topWithSelf(allRows));
  const hidden = $derived(Math.max(0, allRows.length - rows.filter(row => !('gap' in row)).length));
  const totalDamage = $derived(knownNumber(fight?.total_damage));
  const otherDamage = $derived(knownNumber(fight?.unattributed_damage));
  const otherShare = $derived(totalDamage !== null && totalDamage > 0 && otherDamage !== null ? otherDamage / totalDamage : null);
  const fights = $derived(new Map(scopedFights(snapshot, window).map(fight => [`fight:${fight.id}`, fight])));
  const events = $derived(eventRows(view.kills, live ? fight : null, view.drops).map(row => {
    const source = fights.get(row.key);
    const visual = objectVisual(visuals.objects, row.targetType), fallback = objectVisual(visuals.objects, source?.boss_type);
    const type = visual?.sprite ? row.targetType : fallback?.sprite ? source!.boss_type : row.targetType;
    // Retained visit summaries have no participant count; only a full snapshot
    // can establish it. Bounded window fights may contain just five players.
    const full = snapshot?.fights.find(fight => source?.id === fight.id);
    return { ...row, targetType: type, name: visual?.name ?? fallback?.name ?? row.name,
      players: row.state === 'announced' ? null : full?.players.length || row.players };
  }));
  const loot = $derived(homeLoot(view.drops, visuals));
  const timelineDrops = $derived.by(() => {
    const linked = new Set(events.flatMap(row => row.bags.map(bag => bag.dropId)));
    return view.drops.filter(drop => linked.has(drop.id));
  });
  const lootEvidence = $derived(lootView({ drops: view.drops, visuals, partial: view.partial, reasons: view.reasons, title: 'LOOT THIS VISIT' }));
  const resolveSpriteUrl = (sprite: SpriteRef) => spriteUrl(visuals.client_fingerprint, sprite) ?? '';
  const clock = (at: string) => clockTime(at) ?? '—';
</script>

<div class="v4-screen realm-screen" data-stale={stale}>
  <header class="v4-header">
    <SpriteImage sprite={portal?.sprite ?? null} sheetUrl={portal?.sprite ? resolveSpriteUrl(portal.sprite) : null} fit={40} alt={portal?.name ?? 'Realm Portal'} />
    <div class="v4-heading">
      <h2>{instance?.context.realm_name ?? view.realm?.map ?? 'Realm'}</h2>
      <p class="v4-meta"><span>Entered {clockTime(view.visitStarted) ?? '—'}</span>
        <span title="Estimated population">Players {formatStat(knownNumber(instance?.population))} / {formatStat(knownNumber(instance?.context.instance_cap))}</span>
        <span>Fame this visit {fame === null ? '—' : `${fame > 0 ? '+' : ''}${formatStat(fame)}`}</span></p>
    </div>
    <div class="v4-clock"><p>Time here</p><strong aria-label="Time here">{view.visitElapsed !== null ? formatElapsed('0', view.visitElapsed, true) : '—'}</strong>
      {#if view.hasAwayTime}<p>excludes time away</p>{/if}</div>
    <span class="v4-live" class:stale role="status" aria-live="polite">{stale ? 'Recovering' : view.realm ? 'Live' : '—'}</span>
    <div class="realm-progress">
      <div><span>Realm closing progress</span><span>Realm score {formatStat(view.score?.current ?? null)} / {formatStat(view.score?.max ?? null)} · {view.percent === null ? '—' : `${formatStat(view.percent)}%`}</span></div>
      <div class="progress-track" role="progressbar" aria-label="Realm closing progress" title="Realm score, not a closure prediction"
        aria-valuemin="0" aria-valuemax="100" aria-valuenow={view.scoreFill ?? undefined}
        aria-valuetext={view.score ? `Realm score ${view.score.current} / ${view.score.max}` : 'Realm score unavailable'}>
        {#if view.scoreFill !== null}<i style:width={`${view.scoreFill}%`}></i>{/if}
      </div>
    </div>
  </header>
  <div class="v4-realm-grid">
    <div class="v4-main">
      <div class="v4-hero">
        <div class="v4-hero-content" class:covered={white.current !== null} class:ended={fight !== null && !live}
          aria-hidden={white.current !== null} inert={white.current !== null}>
          {#if fight}
            {#if !live}<span class="ended-tag">Ended</span>{/if}
            <DpsHero label={`Your DPS · ${targetName}`} live={!stale && live} dps={knownNumber(view.self?.dps)}
              rank={knownNumber(view.self?.rank)} players={fight.players.length || null} share={view.self?.share ?? null}
              bestHit={knownNumber(view.self?.biggest_hit)} party={partyAverage(fight.players)} usual={history}
              buckets={fight.buckets} height={112} target={{ name: targetName, kind: fight.target_kind ? kinds[fight.target_kind] : '—',
                elapsed: formatElapsed(fight.started, fightEnd, true), sprite: targetSprite }} {resolveSpriteUrl} />
            <BossHpBar hp={fight.target_hp ?? null} max={fight.max_hp} ended={!live} statusInside
              killedAt={fight.kill && !fight.kill.provisional ? fight.kill.at : null} />
          {:else}
            <section class="v4-card quest-target" aria-label="Current quest">
              <SpriteImage sprite={quest?.sprite ?? null} sheetUrl={quest?.sprite ? resolveSpriteUrl(quest.sprite) : null} fit={40} alt={quest?.name ?? 'Unknown target'} />
              <h3>Quest: {quest?.name ?? 'Unknown target'}</h3>
            </section>
          {/if}
        </div>
        <WhiteBanner state={white} {visuals} {resolveSpriteUrl} itemHref={type => routeHref({ kind: 'item', type })} />
      </div>
      <section class="v4-card v4-meter" aria-label="Party damage">
        <h3>Damage on {fight ? targetName : '—'}</h3>
        <p class="v4-meter-total">{formatStat(totalDamage)} damage · {formatDps(fight ? fightDps(fight) : null)} DPS</p>
        <DamageTable basis={fight?.dps_basis} {rows} total={allRows.length} {visuals} {resolveSpriteUrl} rowHref={row => fight ? fightPlayerHref(fight, row) : null} />
        <footer class="v4-meter-footer"><p>{hidden > 0 ? `+${hidden} more · ` : ''}{formatShare(otherShare)} of damage not attributed to a player</p>
          {#if fight}<a href={routeHref({ kind: 'fight', id: fight.id })}>Open fight ›</a>{/if}</footer>
      </section>
    </div>
    <div class="v4-side">
      <EventsLog rows={events} {visuals} {resolveSpriteUrl} />
      <section class="v4-card v4-loot" aria-label="Loot this visit">
        <h3>Loot this visit</h3>
        <BestBagCard best={loot.best} {visuals} {resolveSpriteUrl} meta={loot.best ? clock(loot.best.drop.at) : ''} scope={key} />
        <div class="v4-bags">{#key key}<BagList drops={loot.bags} {visuals} {resolveSpriteUrl} {clock} scroll newestFirst />{/key}</div>
        <LootTally {...loot.counts} />
      </section>
    </div>
  </div>
  <VisitTimeline rows={events} kills={view.kills} liveFight={live ? fight : null} drops={timelineDrops}
    started={view.visitStarted} at={String(now)} {visuals} {resolveSpriteUrl} partial={view.partial} />
  {#if $debugMode}<RealmEvidence {stale} {instance} {view} {fame} {fight} {lootEvidence} />{/if}
</div>

<style>
  .realm-progress { flex-basis: 100%; min-width: 0; }
  .realm-progress > div:first-child { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 4px 12px; font-size: 11px; color: var(--text-secondary); }
  .progress-track { height: 6px; margin-top: 6px; overflow: hidden; border-radius: var(--radius-pill); background: var(--surface-raised); }
  .progress-track i { display: block; height: 100%; background: var(--violet); }
  .v4-hero-content { display: flex; flex-direction: column; gap: 8px; }
  .ended { opacity: .6; }
  .ended-tag { align-self: start; font-size: 11px; padding: 2px 8px; border: 1px solid var(--border); border-radius: var(--radius-pill); }
  .quest-target { display: flex; align-items: center; gap: 12px; min-height: 112px; overflow-wrap: anywhere; }
</style>

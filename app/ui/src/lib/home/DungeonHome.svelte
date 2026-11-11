<script lang="ts">
  import { tick } from 'svelte';
  import DungeonEvidence from './DungeonEvidence.svelte';
  import { objectVisual } from '../bridge/visual-index';
  import type { LiveSnapshot } from '../bindings/LiveSnapshot';
  import type { DungeonBest } from '../bindings/DungeonBest';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { KpiCell } from './v3/types';
  import type { HomeLiveWindow } from './v2/contracts';
  import SpriteImage from '../components/SpriteImage.svelte';
  import { spriteUrl } from '../bridge/visuals';
  import { verifiedSelf } from '../shell/rail';
  import { routeHref } from '../navigation/routes';
  import { fightPlayerHref, runPlayerHref } from '../players/links';
  import { debugMode } from '../debug/debug';
  import { formatDps, formatElapsed, formatStat, formatShare } from '../styles/format';
  import { knownNumber } from './evidence';
  import { dungeonObservations, dungeonRunSelf, dungeonTimeline, dungeonView } from './dungeon';
  import { emptyHomeVisuals, type HomeVisuals } from './visuals';
  import { lootView } from './v2/loot';
  import DungeonTimeline from './v2/DungeonTimeline.svelte';
  import KpiStrip from './v3/KpiStrip.svelte';
  import BestBagCard from './v3/BestBagCard.svelte';
  import BagList from './v3/BagList.svelte';
  import LootTally from './v3/LootTally.svelte';
  import DamageTable from './v3/DamageTable.svelte';
  import BossHpBar from './v3/BossHpBar.svelte';
  import FightTabs from './v3/FightTabs.svelte';
  import FightChart from './v3/FightChart.svelte';
  import { rankWindow } from './v3/rank-window';
  import { homeLoot } from './v3/loot-rank';
  import { bossOnly, damageRows, runRows, personalBest, notificationTip, fightDps } from './v3/screen-data';
  import { clockTime } from './nexus';
  import { arrivalFeedback, type FeedbackEvent } from './v3/feedback';
  import { bestItem } from './v3/loot-rank';
  import './v3/area-home.css';

  let { snapshot, stale, visuals = emptyHomeVisuals(), portalType = null, liveWindow = null,
    onSelectFight, dungeonBests = null }: {
    snapshot: LiveSnapshot | null; stale: boolean; visuals?: HomeVisuals; portalType?: number | null;
    liveWindow?: HomeLiveWindow | null; onSelectFight?: (id: string | null) => void; dungeonBests?: DungeonBest[] | null;
  } = $props();
  let run = $derived(snapshot?.run ?? null);
  let key = $derived(`${snapshot?.stream_id}:${run?.id}`);
  let standaloneFocus = $state<{ key: string; id: string | null } | null>(null);
  let previousId = $derived(liveWindow?.streamId === snapshot?.stream_id ? liveWindow?.focusedFightId ?? null
    : standaloneFocus?.key === key ? standaloneFocus.id : null);
  let view = $derived(dungeonView(snapshot, previousId, liveWindow));
  let observations = $derived(dungeonObservations(snapshot, liveWindow));
  let bosses = $derived(observations.fights.filter(bossOnly));
  let current = $derived(view.fight && bossOnly(view.fight) ? view.fight
    : run?.ended == null ? bosses.find(row => row.ended === null) ?? null : null);
  $effect(() => { const id = current?.id ?? null;
    if (standaloneFocus?.key !== key || standaloneFocus.id !== id) standaloneFocus = { key, id };
  });
  let selection = $state<{ key: string; id: string } | null>(null);
  let chosen = $derived(selection?.key === key && (selection.id === 'whole'
    || bosses.some(row => row.id === selection?.id)) ? selection.id : null);
  $effect(() => {
    if (selection !== null && chosen === null) { selection = null; onSelectFight?.(null); }
  });
  let selected = $derived(chosen ?? current?.id ?? 'whole');
  let wholeRun = $derived(selected === 'whole');
  let fight = $derived(wholeRun ? null : bosses.find(row => row.id === selected) ?? current);
  let self = $derived(verifiedSelf(snapshot?.character ?? null, fight));
  let totals = $derived(run ? snapshot?.run_totals ?? null : null);
  let runSelf = $derived(dungeonRunSelf(snapshot));
  let rows = $derived(wholeRun ? runRows(totals?.players ?? [], runSelf) : rankWindow(damageRows(fight?.players ?? [], self)));
  let portal = $derived(objectVisual(visuals.objects, portalType));
  const resolveSpriteUrl = (s: SpriteRef) => spriteUrl(visuals.client_fingerprint, s);
  const clock = (at: string) => clockTime(at) ?? '—';
  let loot = $derived(homeLoot(observations.drops, visuals));
  let best = $derived(loot.best);
  let bags = $derived(loot.bags);
  let tally = $derived(loot.counts);
  let lootEvidence = $derived(lootView({ drops: observations.drops, visuals, partial: observations.partial,
    reasons: observations.reasons, title: 'LOOT THIS RUN' }));
  let pb = $derived(personalBest(snapshot, dungeonBests));
  let fame = $derived(totals?.fame_delta != null && Number.isFinite(totals.fame_delta) ? totals.fame_delta : null);
  let cells: KpiCell[] = $derived([
    { key: 'time', label: 'Time', value: run && snapshot ? formatElapsed(run.started, run.completed_at ?? run.ended ?? snapshot.at) : '—',
      tone: pb?.behind ? 'bad' : null, sub: pb ? `PB ${pb.best} · ${pb.delta}` : 'PB —',
      subTone: pb ? (pb.behind ? 'bad' : 'good') : null },
    { key: 'players', label: 'Players', value: formatStat(knownNumber(snapshot?.instance?.population)),
      of: formatStat(knownNumber(snapshot?.instance?.context.instance_cap)), sub: `peak ${formatStat(knownNumber(snapshot?.instance?.population_peak))}` },
    { key: 'left', label: 'Left', value: formatStat(observations.left.value), tooltip: notificationTip(observations.events, 'player_left') },
    { key: 'deaths', label: 'Deaths', value: formatStat(observations.deaths.value), tooltip: notificationTip(observations.events, 'player_death') },
    { key: 'fame', label: 'Run fame', value: fame === null ? '—' : `${fame > 0 ? '+' : ''}${formatStat(fame)}`, tone: 'fame', sub: 'this run' },
  ]);
  let markers = $derived(dungeonTimeline(bosses, observations.drops, observations.events));
  let lootRoot: HTMLDivElement;
  const arrivalColours = $derived(new Map(observations.drops.map(drop => {
    const item = bestItem(drop, visuals);
    const white = objectVisual(visuals.objects, drop.bag_type)?.bag?.is_white;
    return [drop.id, item?.tier === 'UT' ? 'var(--tier-ut)' : item?.tier === 'ST'
      ? 'var(--bag-orange)' : white ? 'var(--bag-white, #fff)' : 'var(--text-secondary)'];
  })));
  $effect(() => {
    const colours = arrivalColours;
    void tick().then(() => {
      for (const node of lootRoot?.querySelectorAll<HTMLElement>('.area-loot [data-drop-id]') ?? []) {
        node.style.setProperty('--loot-arrival-colour', colours.get(node.dataset.dropId!) ?? 'var(--text-secondary)');
      }
    });
  });
  let feedback = $derived<FeedbackEvent[]>([
    ...observations.drops.map(drop => ({ id: `bag:${drop.id}`, targets: (root: HTMLElement) => {
      const node = [...root.querySelectorAll<HTMLElement>('.area-loot [data-drop-id]')]
        .find(row => row.dataset.dropId === drop.id) ?? null;
      return [{ node, effect: 'glow' as const, color: 'var(--loot-arrival-colour)' }];
    } })),
    ...markers.map(marker => ({ id: `marker:${marker.id}`, targets: (root: HTMLElement) => {
      const node = [...root.querySelectorAll<HTMLElement>('[data-marker-members]')]
        .find(row => (JSON.parse(row.dataset.markerMembers ?? '[]') as string[]).includes(marker.id)) ?? null;
      return [{ node, effect: 'pulse' as const }];
    } })),
  ]);

  function choose(id: string) {
    selection = { key, id };
    onSelectFight?.(id === 'whole' ? null : id);
  }
</script>

<div bind:this={lootRoot} class="area-home dungeon-home" data-stale={stale} use:arrivalFeedback={{ scope: key, events: feedback }}>
  <header class="area-header run-title">
    <span class="run-icon"><SpriteImage sprite={portal?.sprite ?? null} fit={40}
      sheetUrl={portal?.sprite ? resolveSpriteUrl(portal.sprite) : null} alt={portal?.name ?? 'Dungeon'} /></span>
    <div><h2>{run?.dungeon ?? snapshot?.map ?? 'Dungeon'}</h2><p>Entered {clockTime(run?.started) ?? '—'}</p></div>
    <span class="live-chip" class:stale role={stale ? 'status' : undefined}>{stale ? 'Recovering' : run?.ended ? 'Ended' : run ? 'Live' : '—'}</span>
  </header>
  <div class="area-kpis dungeon-kpis area-pb"><KpiStrip {cells} /></div>
  <section class="area-card fight-panel" aria-label="Fight Damage">
    <h3 class="sr-only">Fight Damage</h3>
    {#key key}<FightTabs fights={bosses} {selected} {visuals} {resolveSpriteUrl} onselect={choose} />{/key}
    <div class="fight-detail" data-whole-run={wholeRun}>
      {#if wholeRun}
        {#if totals}<p class="fight-meta scope"><span>{formatStat(totals.total_damage)} damage</span>
          <span>{totals.fights} fights · {totals.kills} kills</span>
          <span class="gold">You: {formatShare(runSelf?.share ?? null)} · rank {formatStat(knownNumber(runSelf?.rank))}</span></p>
        {:else}<p>No Totals</p>{/if}
      {:else if fight}
        <BossHpBar hp={fight.target_hp ?? null} max={fight.max_hp} ended={fight.ended !== null}
          killedAt={fight.kill && !fight.kill.provisional ? fight.kill.at : null} />
        <p class="fight-meta"><span>{formatStat(knownNumber(fight.total_damage))} damage · {formatDps(fightDps(fight))} DPS</span>
          <span class="gold">You: best hit {formatStat(knownNumber(self?.biggest_hit))} · {formatStat(knownNumber(self?.hits))} hits</span></p>
      {/if}
      {#if wholeRun ? totals !== null : fight !== null}
        <DamageTable basis={wholeRun ? 'whole_run' : fight?.dps_basis} {rows} total={wholeRun ? totals?.players.length ?? 0 : fight?.players.length ?? 0}
          rowHref={row => wholeRun ? run ? runPlayerHref(run.id, row) : null : fight ? fightPlayerHref(fight, row, run?.id) : null}
          otherDamage={wholeRun ? totals?.unattributed_damage : fight?.unattributed_damage} {visuals} {resolveSpriteUrl}
          moreHref={wholeRun && run ? routeHref({ kind: 'run', id: run.id })
            : fight ? routeHref({ kind: 'fight', id: fight.id, runId: run?.id }) : null} />
      {/if}
      {#if fight && !wholeRun}<FightChart buckets={fight.buckets} />{/if}
    </div>
  </section>
  <section class="area-card area-loot" aria-label="LOOT THIS RUN">
    <h3>LOOT THIS RUN</h3>
    <BestBagCard {best} {visuals} {resolveSpriteUrl} meta={best ? clock(best.drop.at) : ''} scope={key} />
    <div class="bag-space">{#key key}<BagList drops={bags} {visuals} {resolveSpriteUrl} {clock} />{/key}</div>
    <LootTally {...tally} />
  </section>
  {#if run && snapshot}<div class="area-timeline">{#key key}
    <DungeonTimeline {run} at={snapshot.at} {markers} partial={observations.partial} {visuals} />
  {/key}</div>{/if}
  {#if $debugMode}<DungeonEvidence {stale} {run} {view} {fame} {pb} {snapshot} {observations} {totals} {lootEvidence} {wholeRun} {self} {fight} />{/if}
</div>

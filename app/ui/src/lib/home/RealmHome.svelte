<script lang="ts">
  import { objectVisual } from '../bridge/visual-index';
  import { arrivalFeedback, type FeedbackEvent } from './v3/feedback';
  import type { LiveSnapshot } from '../bindings/LiveSnapshot';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { TargetKind } from '../bindings/TargetKind';
  import type { HomeLiveWindow } from './v2/contracts';
  import type { KpiCell } from './v3/types';
  import { emptyHomeLiveWindow } from './v2/live';
  import { emptyHomeVisuals, type HomeVisuals } from './visuals';
  import { realmView, realmKillLabels } from './realm';
  import { clockTime } from './nexus';
  import { coverageLabels, knownNumber, reasonNotes } from './evidence';
  import { formatDps, formatElapsed, formatStat } from '../styles/format';
  import { spriteUrl } from '../bridge/visuals';
  import { routeHref } from '../navigation/routes';
  import { debugMode } from '../debug/debug';
  import SpriteImage from '../components/SpriteImage.svelte';
  import ObjectLabel from './ObjectLabel.svelte';
  import KpiStrip from './v3/KpiStrip.svelte';
  import BestBagCard from './v3/BestBagCard.svelte';
  import BagList from './v3/BagList.svelte';
  import LootTally from './v3/LootTally.svelte';
  import BossHpBar from './v3/BossHpBar.svelte';
  import DamageTable from './v3/DamageTable.svelte';
  import RealmKills from './v3/RealmKills.svelte';
  import { rankWindow } from './v3/rank-window';
  import { homeLoot } from './v3/loot-rank';
  import { damageRows, fightDps } from './v3/screen-data';
  import { lootView } from './v2/loot';
  import './v3/area-home.css';

  let { snapshot = null, stale = false, window = emptyHomeLiveWindow(), visuals = emptyHomeVisuals() }: {
    snapshot?: LiveSnapshot | null; stale?: boolean; window?: HomeLiveWindow; visuals?: HomeVisuals;
  } = $props();
  const kinds: Record<TargetKind, string> = { event_boss: 'Event boss', boss: 'Boss', miniboss: 'Miniboss', quest: 'Quest', ordinary: 'Target' };
  let standaloneFocus = $state<{ key: string; id: string | null } | null>(null);
  let focusKey = $derived(`${snapshot?.stream_id}:${snapshot?.session?.id}:${window.visitKey}`);
  let previousId = $derived(window.streamId === snapshot?.stream_id ? window.focusedFightId ?? null
    : standaloneFocus?.key === focusKey ? standaloneFocus.id : null);
  let view = $derived(realmView(snapshot, window, previousId));
  $effect(() => {
    const id = view.fight?.id ?? null;
    if (standaloneFocus?.key !== focusKey || standaloneFocus.id !== id) standaloneFocus = { key: focusKey, id };
  });
  let instance = $derived(view.realm?.instance);
  let fight = $derived(view.fight);
  let visual = $derived(objectVisual(visuals.objects, (fight?.target_type ?? fight?.boss_type)));
  let portal = $derived(objectVisual(visuals.objects, 1796));
  let live = $derived(view.fightStatus === 'live');
  let fightEnd = $derived(fight?.ended ?? fight?.kill?.at ?? (live ? view.realm?.at : fight?.last_hit) ?? fight?.started ?? '');
  let fame = $derived(view.visitFame);
  let rows = $derived(rankWindow(damageRows(fight?.players ?? [], view.self)));
  let loot = $derived(homeLoot(view.drops, visuals));
  let best = $derived(loot.best);
  let bags = $derived(loot.bags);
  let tally = $derived(loot.counts);
  let lootEvidence = $derived(lootView({ drops: view.drops, visuals, partial: view.partial, reasons: view.reasons, title: 'LOOT THIS VISIT' }));
  const resolveSpriteUrl = (s: SpriteRef) => spriteUrl(visuals.client_fingerprint, s);
  const clock = (at: string) => clockTime(at) ?? '—';
  let cells: KpiCell[] = $derived([
    { key: 'score', label: 'Realm score', value: formatStat(view.score?.current ?? null),
      of: formatStat(view.score?.max ?? null), sub: view.percent === null ? '—' : `${formatStat(view.percent)}%`, progress: view.scoreFill },
    { key: 'players', label: 'Players', value: formatStat(knownNumber(instance?.population)),
      of: formatStat(knownNumber(instance?.context.instance_cap)), sub: `peak ${formatStat(knownNumber(instance?.population_peak))}` },
    { key: 'time', label: 'Time here', value: view.visitElapsed !== null ? formatElapsed('0', view.visitElapsed, true) : '—',
      sub: view.hasAwayTime ? 'excludes time away' : `since ${view.visitStarted ? clock(view.visitStarted) : '—'}` },
    { key: 'fame', label: 'Fame', value: fame === null ? '—' : `+${formatStat(fame)}`, sub: 'this visit', tone: 'fame' },
  ]);
  let killFeedback = $derived<FeedbackEvent[]>(view.kills.map(row => ({
    // Task 7 retains exact fight links when an announcement is merged.
    id: row.fightId ? `fight:${row.fightId}` : row.id,
    targets: (root: HTMLElement) => {
      if (!row.fightId || row.provisional || row.evidence === 'realm_announcement' || !(row.self && row.self.damage > 0)) return [];
      const panel = [...root.querySelectorAll<HTMLElement>('.realm-fight[data-fight-id]')]
        .find(node => node.dataset.fightId === row.fightId) ?? null;
      const kill = [...root.querySelectorAll<HTMLElement>('[data-kill-id]')]
        .find(node => node.dataset.killId === row.id) ?? null;
      return [{ node: panel, effect: 'glow' as const, color: '#f2c14e', duration: 500 },
        { node: kill, effect: 'slide' as const, duration: 500 }];
    },
  })));
</script>

<div class="area-home realm-home" data-stale={stale}
  use:arrivalFeedback={{ scope: focusKey, events: killFeedback }}>
  <header class="area-header">
    <span class="run-icon"><SpriteImage sprite={portal?.sprite ?? null} fit={40}
      sheetUrl={portal?.sprite ? resolveSpriteUrl(portal.sprite) : null} alt={portal?.name ?? 'Realm Portal'} /></span>
    <div><h2>{instance?.context.realm_name ?? view.realm?.map ?? 'Realm'}</h2>
      <p>Realm · entered {view.visitStarted ? clock(view.visitStarted) : '—'}</p></div>
    <span class="live-chip" class:stale role={stale ? 'status' : undefined}>{stale ? 'Reconnecting' : view.realm ? 'Live' : '—'}</span>
  </header>
  <div class="area-kpis"><KpiStrip {cells} /></div>
  <section class="area-card fight-panel realm-fight" aria-label="Realm Fight" data-fight-id={fight?.id}>
    <div class="fight-heading"><h3>{live || !fight ? 'LIVE FIGHT' : 'LAST FIGHT'}</h3>
      {#if view.fightStatus === 'ended'}<span class="ended-tag">Ended</span>{/if}
    </div>
    {#if fight}
      <div class="fight-target">
        <SpriteImage sprite={visual?.sprite ?? null} fit={40} sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null}
          alt={visual?.name ?? 'Boss'} />
        <div class="boss-name"><b title={visual?.name ?? 'Unknown Target'}>{visual?.name ?? 'Unknown Target'}</b>
          <span class="kind">{fight.target_kind ? kinds[fight.target_kind] : '—'}</span></div>
        <span class="fight-time">{formatElapsed(fight.started, fightEnd, true)} fight</span>
      </div>
      <BossHpBar hp={fight.target_hp ?? null} max={fight.max_hp} ended={fight.ended !== null} statusInside
        killedAt={fight.kill && !fight.kill.provisional ? fight.kill.at : null} />
      <p class="fight-meta"><span>{formatStat(knownNumber(fight.total_damage))} damage · {formatDps(fightDps(fight))} DPS</span>
        <span class="gold">You: best hit {formatStat(knownNumber(view.self?.biggest_hit))} · {formatStat(knownNumber(view.self?.hits))} hits</span></p>
      <DamageTable {rows} total={fight.players.length} otherDamage={fight.unattributed_damage} {visuals} {resolveSpriteUrl}
        moreHref={routeHref({ kind: 'fight', id: fight.id })} />
    {:else}
      <BossHpBar hp={null} max={null} statusInside /><p>No Boss</p>
      {#if instance?.quest_target}<p>Quest: <ObjectLabel type={instance.quest_target.object_type ?? null} kind="enemy" {visuals} /></p>{/if}
    {/if}
  </section>
  <section class="area-card realm-kill-card" aria-label="BOSSES KILLED">
    <h3>BOSSES KILLED <span class="kill-count">{view.kills.length}</span></h3>
    {#key view.visitKey}<RealmKills rows={view.kills} {window} {visuals} {resolveSpriteUrl} />{/key}
  </section>
  <section class="area-card area-loot" aria-label="LOOT THIS VISIT">
    <h3>LOOT THIS VISIT</h3>
    {#if view.drops.length}<BestBagCard {best} {visuals} {resolveSpriteUrl} meta={best ? clock(best.drop.at) : ''} scope={view.visitKey} />{:else}<p>No Loot</p>{/if}
    <div class="bag-space">{#if view.drops.length}{#key view.visitKey}<BagList drops={bags} {visuals} {resolveSpriteUrl} {clock} />{/key}{/if}</div>
    <LootTally {...tally} />
  </section>
  {#if $debugMode}<details class="area-debug"><summary>Visit evidence</summary>
    {#if stale}<p>Capture is recovering; values are frozen.</p>{/if}
    <p>Estimated population. Peak is the highest received population in this instance.</p>
    {#if knownNumber(instance?.population) === null}<p>No population observation.</p>{/if}
    {#if !view.score}<p>A finite nonnegative score and positive maximum are required.</p>{/if}
    {#if !view.visitAvailable}<p>Visit observations unavailable until the next observed entry</p>{/if}
    {#if view.partial}<p>Partial visit observations - gaps or omitted rows remain.</p>{/if}
    {#if fame === null}<p>Fame this visit unavailable: a verified entry baseline and current fame were not both established.</p>{/if}
    {#each view.reasons as reason}<p>{reason}</p>{/each}
    {#each [...new Set([...(view.realm?.issues ?? []), ...(fight?.issues ?? [])])] as issue}<p>{coverageLabels[issue]}</p>{/each}
    {#each lootEvidence.reasons as reason}<p>{reason}</p>{/each}
    <p>Loot counts are appearances in bags, not stack sizes or items picked up.</p>
    {#if view.fightPartial}<p>Partial fight.</p>{/if}
    {#if fight && !view.self}<p>Your contribution is unavailable without a unique verified local player.</p>{/if}
    {#if fight?.players.some(row => row.dps === null || row.rank === null || row.share === null)}<p>Unavailable rank, DPS or share was not established by captured evidence.</p>{/if}
    {#if fight && (knownNumber(fight.target_hp) === null || !(fight.max_hp && fight.max_hp > 0))}<p>Current HP or positive maximum was not observed.</p>{/if}
    <p>Retained ended fights may contain only five players. More-player counts cover supplied rows only.</p>
    {#if !fight && instance?.quest_target}<p>Quest targeting alone does not establish a kill or event boss.</p>{/if}
    {#each view.kills as kill}<p>{realmKillLabels[kill.evidence]}{kill.provisional ? ' · Provisional' : ''}{kill.partial ? ' · Partial Fight' : ''}</p>
      {#if kill.self && (kill.self.rank === null || kill.self.share === null)}<p>Missing metrics were not established by captured evidence.</p>{/if}
      {#if !kill.self}<p>Contribution unavailable - {kill.evidence === 'realm_announcement' ? 'announcement has no exact fight link.' : 'local identity not verified for this fight.'}</p>{/if}
    {/each}
    {#each fight?.players ?? [] as player}{#each player.sources.filter(source => source.estimated) as source}
      {#each reasonNotes(source.reasons).length ? reasonNotes(source.reasons) : ['Damage source estimated.'] as reason}<p>{reason}</p>{/each}
    {/each}{/each}
  </details>{/if}
</div>

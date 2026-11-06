<script lang="ts">
  import type { LiveSnapshot } from '../bindings/LiveSnapshot';
  import type { FightView } from '../bindings/FightView';
  import type { PlayerDamage } from '../bindings/PlayerDamage';
  import type { DungeonView, dungeonObservations } from './dungeon';
  import type { personalBest } from './v3/screen-data';
  import type { lootView } from './v2/loot';
  import { formatStat } from '../styles/format';
  import { coverageLabels, evidenceLabel, knownNumber, reasonNotes } from './evidence';

  let { stale, run, view, fame, pb, snapshot, observations, totals, lootEvidence, wholeRun, self, fight }: {
    stale: boolean;
    run: LiveSnapshot['run'];
    view: DungeonView;
    fame: number | null;
    pb: ReturnType<typeof personalBest>;
    snapshot: LiveSnapshot | null;
    observations: ReturnType<typeof dungeonObservations>;
    totals: LiveSnapshot['run_totals'];
    lootEvidence: ReturnType<typeof lootView>;
    wholeRun: boolean;
    self: PlayerDamage | null;
    fight: FightView | null;
  } = $props();
</script>

<details class="area-debug"><summary>Run evidence</summary>
  {#if stale}<p>Capture is recovering. Numbers and elapsed time are frozen at the last accepted observation.</p>{/if}
  <p>Run outcome: {run?.outcome ?? 'Not established'}. Your deaths this run: {formatStat(view.personalDeaths)} (run evidence, not added to notification counts).</p>
  {#each run?.evidence ?? [] as item}<p>{evidenceLabel(item.evidence)}</p>{/each}
  {#if fame === null}<p>Run Fame unavailable: two fresh fame samples were not established.</p>{/if}
  {#if !pb}<p>All-time best clear unavailable: no saved baseline or the dungeon-bests query did not return a ready result.</p>{/if}
  {#if knownNumber(snapshot?.instance?.population) === null}<p>Players unavailable until an instance population is observed.</p>{/if}
  <p>Players are estimated. Departure causes are unconfirmed.</p>
  <p>Death and departure counts describe received notifications only. Unavailable counts do not mean zero.</p>
  {#if observations.left.provisional || observations.deaths.provisional}<p>Provisional notifications.</p>{/if}
  {#if view.partial || observations.partial || totals?.partial}<p>Partial observations.</p>{/if}
  {#each observations.reasons as reason}<p>{reason}</p>{/each}
  {#each lootEvidence.reasons as reason}<p>{reason}</p>{/each}
  <p>Loot counts are appearances in bags, not stack sizes or items picked up.</p>
  <p>Ranks and shares are supplied across the observed set; these rows are not a complete party census. Unavailable values lack captured evidence.</p>
  <p>Retained ended fights may contain only five players. More-player counts cover supplied rows only.</p>
  {#if wholeRun}<p>Dungeon DPS uses the encounter span from first to last hit.</p>{/if}
  {#if !self && !wholeRun}<p>Your DPS and rank are unavailable: your identity or a unique local player is unavailable.</p>{/if}
  {#if fight && (knownNumber(fight.target_hp) === null || !(fight.max_hp && fight.max_hp > 0))}<p>Current HP or positive maximum was not observed.</p>{/if}
  {#each fight?.issues ?? [] as issue}<p>{coverageLabels[issue]}</p>{/each}
  {#each fight?.players ?? [] as player}
    <p>{player.name ?? 'Player'} · Hits {formatStat(player.hits)} · Deaths {formatStat(player.deaths)}</p>
    {#each player.sources.filter(source => source.estimated) as source}
      {#each reasonNotes(source.reasons).length ? reasonNotes(source.reasons) : ['Damage source estimated.'] as reason}<p>{reason}</p>{/each}
    {/each}
  {/each}
</details>

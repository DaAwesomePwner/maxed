<script lang="ts">
  import type { LiveSnapshot } from '../bindings/LiveSnapshot';
  import type { FightView } from '../bindings/FightView';
  import type { realmView } from './realm';
  import type { lootView } from './v2/loot';
  import { realmKillLabels } from './realm';
  import { coverageLabels, knownNumber, reasonNotes } from './evidence';

  let { stale, instance, view, fame, fight, lootEvidence }: {
    stale: boolean;
    instance: LiveSnapshot['instance'] | undefined;
    view: ReturnType<typeof realmView>;
    fame: number | null;
    fight: FightView | null;
    lootEvidence: ReturnType<typeof lootView>;
  } = $props();
</script>

<details class="area-debug"><summary>Visit evidence</summary>
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
</details>

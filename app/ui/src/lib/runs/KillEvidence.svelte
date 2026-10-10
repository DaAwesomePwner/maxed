<script lang="ts">
  import { debugMode } from '../debug/debug';
  import type { KillView } from '../bindings/KillView';
  import type { KillEvidence } from '../bindings/KillEvidence';
  import { timestamp } from './story';
  let { kill = null }: { kill?: KillView | null } = $props();
  const labels: Record<KillEvidence, string> = {
    victory_quest_target: 'Victory associated with the quest target',
    victory_sole_boss: 'Victory associated with the sole boss',
    announced_death_removed: 'Death announcement paired with target removal',
    hp_zero_removed: 'Zero HP followed by target removal',
    realm_announcement: 'Realm kill announcement',
  };
</script>

{#if $debugMode}
<section aria-label="Kill Evidence"><p class:provisional={kill?.provisional}>
  {#if kill}<strong>{kill.provisional ? 'Provisional kill evidence' : 'Saved kill evidence'}</strong>
    &middot; {timestamp(kill.at)} &middot; {labels[kill.evidence]}
  {:else}Kill evidence unavailable: this fight has no saved kill evidence.{/if}
</p></section>
{/if}

<style>
  p { margin: 0; color: var(--text-secondary); font-size: var(--text-label); }
  strong { color: var(--text-primary); } .provisional, .provisional strong { color: var(--estimated); }
</style>

<script lang="ts">
  import type { EnchantView } from '../bindings/EnchantView';
  import { enchantLabel } from './presentation';
  let { enchant }: { enchant: EnchantView } = $props();
</script>
<span class="enchant" title={enchantLabel(enchant)}>
  {#if enchant.state === 'recorded' && enchant.applied !== null && enchant.applied > 0}
    <span class="pips" aria-hidden="true">{#each Array.from({ length: Math.min(4, enchant.applied) }) as _}<i></i>{/each}</span>
  {/if}
  <span>{enchantLabel(enchant)}</span>
</span>
<style>
  .enchant { display: inline-flex; gap: 5px; align-items: center; min-width: 0; max-width: 100%; }
  .enchant > span:last-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pips { display: inline-flex; gap: 2px; flex: none; }
  i { width: 3px; height: 3px; background: var(--gold); }
</style>

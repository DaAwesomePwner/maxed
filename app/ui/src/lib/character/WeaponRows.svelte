<script lang="ts">
  import type { CharacterWeaponCalculation } from '../bindings/CharacterWeaponCalculation';
  import type { CharacterValue } from '../bindings/CharacterValue';
  import type { CharacterRange } from '../bindings/CharacterRange';
  import FormulaDetails, { amount, valueReason, reasonText } from './FormulaDetails.svelte';
  const observedInput = (value: number | null) => value === null ? '— · Not observed' : amount(value);
  let { weapon, version, defense }: { weapon: CharacterWeaponCalculation; version: string; defense: number } = $props();
</script>
{#snippet scalar(value: CharacterValue)}
  {amount(value.value, 2)}{#if valueReason(value)}<small>{valueReason(value)}</small>{/if}
{/snippet}
{#snippet bounds(value: CharacterRange)}
  {#if value.min === null || value.max === null}—{:else}{amount(value.min)}–{amount(value.max)}{/if}
  {#if valueReason(value)}<small>{valueReason(value)}</small>{/if}
{/snippet}
<table>
  <thead><tr><th>Attack group</th><th>Catalog damage range</th><th>Damage per projectile after defense</th>
    <th>Normal firing rate<br />attacks/sec</th><th>Projectiles/sec</th><th>Theoretical DPS</th></tr></thead>
  <tbody>
    {#each weapon.groups as group}
      <tr><th scope="row">{group.group_id}<small>Projectile {group.projectile_index}</small></th>
        <td>{@render bounds(group.catalog_range)}</td><td>{@render bounds(group.hit_range)}</td>
        <td>{@render scalar(group.attacks_per_sec)}</td><td>{@render scalar(group.projectiles_per_sec)}</td><td>{@render scalar(group.dps)}</td></tr>
      <tr><td colspan="6"><FormulaDetails {version} inputs={[
        { label: 'ATT', value: observedInput(weapon.applied_att) }, { label: 'DEX', value: observedInput(weapon.applied_dex) },
        { label: 'Applied condition bits (Weak, Damaging)', value: observedInput(weapon.applied_primary_mask) },
        { label: 'Applied defense', value: String(defense) }, { label: 'Projectile count', value: String(group.count) },
        { label: 'Rate factor', value: amount(group.rate_factor.value, 2) },
        { label: 'Armor piercing', value: group.armor_piercing === null ? '— · Metadata unavailable' : group.armor_piercing ? 'Yes' : 'No' },
      ]} values={[
        { label: 'Catalog range', value: group.catalog_range }, { label: 'Hit range', value: group.hit_range },
        { label: 'Mean hit', value: group.hit_mean }, { label: 'Rate factor', value: group.rate_factor },
        { label: 'Normal firing rate', value: group.attacks_per_sec }, { label: 'Projectiles/sec', value: group.projectiles_per_sec },
        { label: 'Theoretical DPS', value: group.dps },
      ]} /></td></tr>
    {:else}
      <tr><td colspan="6">— No supported attack groups. {valueReason(weapon.dps_total) || 'Firing metadata unavailable'}</td></tr>
    {/each}
    <tr><th colspan="5" scope="row">Supported subtotal</th><td>{@render scalar(weapon.dps_subtotal)}</td></tr>
    <tr><th colspan="5" scope="row">Full total — Theoretical DPS</th><td>{@render scalar(weapon.dps_total)}</td></tr>
  </tbody>
</table>
<FormulaDetails {version} inputs={[
  { label: 'ATT', value: observedInput(weapon.applied_att) },
  { label: 'DEX', value: observedInput(weapon.applied_dex) },
  { label: 'Applied condition bits (Weak, Damaging)', value: observedInput(weapon.applied_primary_mask) },
  { label: 'Applied defense', value: String(defense) },
]} values={[
  { label: 'Supported subtotal', value: weapon.dps_subtotal },
  { label: 'Full total', value: weapon.dps_total },
]} />
{#if weapon.dps_total.exclusions.length}<p>Excluded: {reasonText(weapon.dps_total.exclusions)}</p>{/if}
<style>
  table { width: 100%; table-layout: fixed; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { text-align: left; padding: 8px 4px; border-bottom: 1px solid var(--border); vertical-align: top; overflow-wrap: anywhere; }
  thead th { font-size: 12px; color: var(--text-secondary); }
  small { display: block; font-size: 11px; color: var(--text-secondary); }
  p { overflow-wrap: anywhere; }
</style>

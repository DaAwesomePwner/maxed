<script lang="ts">
  import type { CharacterRegeneration } from '../bindings/CharacterRegeneration';
  import type { CharacterValue } from '../bindings/CharacterValue';
  import { amount, reasonText, valueReason } from './FormulaDetails.svelte';
  let { regeneration = null, version }: { regeneration?: CharacterRegeneration | null; version?: string } = $props();
  const resources = [
    { key: 'hp', label: 'HP', caption: 'Excludes base VIT, pets, healing effects and combat-state modifiers.' },
    { key: 'mp', label: 'MP', caption: 'Excludes base MP regeneration, pets, consumables, ability costs and combat-state modifiers.' },
  ] as const;
</script>
{#snippet contribution(value: CharacterValue | undefined)}
  {amount(value?.value, 2)}{#if value?.value != null} /sec{/if}
  {#if valueReason(value)}<small>{valueReason(value)}</small>{/if}
{/snippet}
<section aria-labelledby="character-regeneration">
  <h2 id="character-regeneration">Regeneration</h2>
  <p>Baseline contributions only. Quiet, Bleeding, Sick and other suppression or healing effects are excluded.
    No inferred combat state or automatic out-of-combat doubling.</p>
  {#each resources as resource}
    <div class="resource">
      <h3>{resource.label} contributions</h3>
      <h4>Included</h4>
      <dl>
        {#if resource.key === 'mp'}<div><dt>WIS contribution</dt><dd>{@render contribution(regeneration?.wis)}</dd></div>{/if}
        {#each regeneration?.effects.filter(effect => effect.resource === resource.key) ?? [] as effect}
          <div><dt>Slot {effect.source_slot + 1} · enchant {effect.enchant_id}</dt>
            <dd>{@render contribution(effect.total)}<br />Supported subtotal: {@render contribution(effect.subtotal)}
              <details><summary>Contribution details</summary>
                <p>Formula version: {version ?? '— · Calculation unavailable'}</p>
                <p>Flat: {@render contribution(effect.flat_per_sec)}</p>
                <p>Fraction of maximum per second: {amount(effect.fraction_of_max_per_sec.value, 4)}
                  {valueReason(effect.fraction_of_max_per_sec)}</p>
                <p>Applied maximum: {amount(effect.applied_maximum)}{effect.applied_maximum === null ? ' · Not observed' : ''}</p>
                <p>Source units: {effect.source_units.join(', ') || '— · Metadata unavailable'}</p>
                <p>Evidence: {effect.total.evidence}; {effect.total.completeness}</p>
                {#if effect.total.exclusions.length}<p>Excluded: {reasonText(effect.total.exclusions)}</p>{/if}
              </details>
            </dd></div>
        {:else}<div><dt>Equipped enchant effects</dt><dd>No individual effects reported; contribution availability is shown below.</dd></div>{/each}
        <div><dt>Supported subtotal</dt><dd>{@render contribution(regeneration?.[`${resource.key}_subtotal`])}</dd></div>
        <div><dt>Full modeled contribution</dt><dd>{@render contribution(regeneration?.[`${resource.key}_total`])}</dd></div>
      </dl>
      <h4>Excluded</h4>
      <p>{resource.caption}</p>
      <dl><div><dt>{resource.key === 'hp' ? 'Base VIT' : 'Base MP regeneration'}</dt>
        <dd>— unavailable/excluded<small>{valueReason(resource.key === 'hp' ? regeneration?.base_vit : regeneration?.base_mp) || 'Pending verification'}</small></dd></div></dl>
      {#if regeneration?.[`${resource.key}_total`].exclusions.length}
        <p>{reasonText(regeneration[`${resource.key}_total`].exclusions)}</p>
      {/if}
    </div>
  {/each}
  <details><summary>Regeneration formula details</summary>
    <p>Formula version: {version ?? '— · Calculation unavailable'}</p>
    <p>Applied WIS: {amount(regeneration?.applied_wis)}{regeneration?.applied_wis == null ? ' · Not observed' : ''}</p>
    <p>WIS contribution evidence: {regeneration?.wis.evidence ?? '— · Calculation unavailable'}.
      WIS alone is not the complete natural MP regeneration rate.</p>
    <p>A recorded empty enchant list with complete metadata can contribute zero. Missing enchant metadata
      remains unavailable. Missing maxima block percentage contributions independently of known flat contributions.</p>
  </details>
</section>
<style>
  section, .resource { min-width: 0; overflow-wrap: anywhere; }
  .resource { border-top: 1px solid var(--hairline); padding-block: var(--space-3); }
  h2 { margin: 0 0 var(--space-3); font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); }
  h3 { margin: 0 0 var(--space-2); font-size: var(--text-body); line-height: var(--leading-body); font-weight: var(--weight-medium); }
  h4 { margin: var(--space-3) 0 var(--space-1); font-size: var(--text-caption); line-height: var(--leading-caption); font-weight: var(--weight-medium); color: var(--text-muted); }
  dl { margin: 0; }
  dl > div { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); gap: var(--space-3); padding-block: var(--space-2); border-bottom: 1px solid var(--hairline); font-size: var(--text-label); line-height: var(--leading-label); }
  dt { color: var(--text-secondary); }
  dd { margin: 0; font-variant-numeric: tabular-nums; }
  small { display: block; color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  p { margin-block: 0 var(--space-2); color: var(--text-secondary); font-size: var(--text-label); line-height: var(--leading-label); }
  summary { cursor: pointer; min-height: var(--target-min); align-content: center; font-size: var(--text-label); color: var(--text-secondary); }
  summary:hover { color: var(--text-primary); }
</style>

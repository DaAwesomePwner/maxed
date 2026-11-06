<script module lang="ts">
  import type { CharacterValue } from '../bindings/CharacterValue';
  import type { CharacterRange } from '../bindings/CharacterRange';
  export const reasonText = (reasons: string[]) => reasons.map(reason => reason.replaceAll('_', ' ')).join('; ');
  export function amount(value: number | null | undefined, decimals = 0): string {
    return value != null && Number.isFinite(value) ? value.toFixed(decimals) : '—';
  }
  export function valueReason(value: CharacterValue | CharacterRange | null | undefined): string {
    if (!value) return 'Calculation unavailable';
    if (value.reasons.length) return reasonText(value.reasons);
    const missing = 'value' in value ? value.value === null : value.min === null || value.max === null;
    return missing ? 'Required inputs unavailable' : '';
  }
</script>

<script lang="ts">
  let { version, inputs, values }: {
    version: string; inputs: { label: string; value: string }[];
    values: { label: string; value: CharacterValue | CharacterRange }[];
  } = $props();
</script>

<details>
  <summary onkeydown={event => {
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    const details = event.currentTarget.parentElement as HTMLDetailsElement;
    details.open = !details.open;
  }}>Formula details</summary>
  <p>Formula version: {version}</p>
  <ul aria-label="Applied inputs">{#each inputs as input}<li>{input.label}: {input.value}</li>{/each}</ul>
  <p>Catalog bounds use integer rolls [min, max), or the fixed value when equal. Adjusted damage is truncated
    before per-projectile defense. Display rounding does not change calculation precision.</p>
  <ul aria-label="Calculation evidence">
    {#each values as entry}
      <li>{entry.label} — Evidence: {entry.value.evidence}; {entry.value.completeness}
        {#if valueReason(entry.value)}; {valueReason(entry.value)}{/if}
        {#if entry.value.exclusions.length}; Excluded: {reasonText(entry.value.exclusions)}{/if}
      </li>
    {/each}
  </ul>
</details>

<style>
  details { margin-block: 12px; overflow-wrap: anywhere; }
  summary { cursor: pointer; min-height: var(--target-min, 32px); align-content: center; color: var(--violet-text); }
  p, ul { margin-block: 8px; }
  ul { padding-left: 20px; }
</style>

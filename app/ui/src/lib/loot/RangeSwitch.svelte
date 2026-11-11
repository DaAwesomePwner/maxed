<script lang="ts">
  import { tick } from 'svelte';
  import type { TimeRange } from '../bindings/TimeRange';
  import { presetRange, presetFor, type RangePreset } from '../runs/runs-list';
  import { dateRange, filterDays } from '../runs/filters';
  import { rangeLabel } from './loot-format';
  import '../styles/controls.css';
  let { range, label = 'Loot date range', now = () => new Date(), onChange }: {
    range: TimeRange | null; label?: string; now?: () => Date; onChange: (range: TimeRange | null) => void;
  } = $props();
  const id = $props.id();
  const presets: Array<[RangePreset, string]> = [['all', 'All time'], ['30d', '30 days'], ['7d', '7 days'], ['today', 'Today']];
  let selected = $derived(presetFor(range, now()));
  let open = $state(false), from = $state(''), through = $state(''), validation = $state('');
  let button = $state<HTMLButtonElement>(), first = $state<HTMLInputElement>();
  async function show() {
    [from, through] = filterDays(range); validation = ''; open = true;
    await tick(); first?.focus();
  }
  function close() { open = false; button?.focus(); }
  function apply(event: SubmitEvent) {
    event.preventDefault();
    try { const next = dateRange(from, through); onChange(next); close(); }
    catch (error) { validation = error instanceof Error ? error.message : 'Invalid Dates'; }
  }
</script>
<svelte:window onkeydown={event => { if (open && event.key === 'Escape') { event.preventDefault(); close(); } }} />
<div class="range">
  <div class="seg" role="group" aria-label={label}>
    {#each presets as [preset, label]}
      <button type="button" aria-pressed={selected === preset} onclick={() => { open = false; onChange(presetRange(preset, now())); }}>{label}</button>
    {/each}
    <button type="button" bind:this={button} aria-pressed={selected === null} aria-expanded={open} aria-controls={`${id}-dates`} onclick={() => open ? close() : show()}>{selected === null && range ? rangeLabel(range) : 'Custom…'}</button>
  </div>
  {#if open}
    <form id={`${id}-dates`} class="popover" aria-label="Custom date range" onsubmit={apply}>
      <label for={`${id}-from`}>From</label><input class="ctl" id={`${id}-from`} bind:this={first} type="date" bind:value={from} />
      <label for={`${id}-through`}>Through</label><input class="ctl" id={`${id}-through`} type="date" bind:value={through} />
      {#if validation}<p role="alert">{validation}</p>{/if}
      <div class="actions"><button class="ctl ghost" type="button" onclick={close}>Cancel</button><button class="ctl" type="submit">Use dates</button></div>
    </form>
  {/if}
</div>
<style>
  .range { position: relative; }
  .popover { position: absolute; right: 0; top: calc(100% + var(--space-2)); z-index: 10; display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: var(--space-2); width: 240px; padding: var(--space-3); border: 1px solid var(--hairline-strong); border-radius: var(--radius-control); background: var(--surface-raised); box-shadow: var(--elevation-popover); font-size: var(--text-caption); }
  .actions { grid-column: 1 / -1; display: flex; justify-content: flex-end; gap: var(--space-2); }
  p { grid-column: 1 / -1; margin: 0; color: var(--danger); }
</style>

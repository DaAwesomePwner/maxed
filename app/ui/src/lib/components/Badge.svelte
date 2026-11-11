<script lang="ts">
  import { debugMode } from '../debug/debug';
  import Tooltip from './Tooltip.svelte';
  let { label, tone = 'neutral', estimated }: {
    label: string; tone?: 'neutral' | 'gold' | 'success' | 'danger' | 'warning' | 'estimated' | 'information'; estimated?: { reason: string };
  } = $props();
  const componentId = $props.id();
  const tooltipId = `${componentId}-evidence`;
  let reason = $derived.by(() => {
    if ((estimated || tone === 'estimated') && !estimated?.reason.trim()) throw new Error('Estimated badges require a nonempty evidence reason.');
    return estimated?.reason.trim();
  });
</script>

{#if reason && $debugMode}
  <Tooltip id={tooltipId} text={reason}>
    {#snippet trigger(describedby)}
      <button type="button" class="badge" data-tone={tone} aria-describedby={describedby}>{label} <span class="estimate">· Estimated</span></button>
    {/snippet}
  </Tooltip>
{:else}<span class="badge" data-tone={tone}>{label}</span>{/if}

<style>
  /* The one badge: a quiet hairline tag. Tone colours the text only where it carries meaning. */
  .badge { display: inline-flex; align-items: center; flex-wrap: wrap; gap: var(--space-1); box-sizing: border-box; min-height: 18px; padding: 0 var(--space-1); border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); background: transparent; color: var(--text-secondary); font-family: var(--font-ui); font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-medium); overflow-wrap: anywhere; }
  button.badge { min-height: var(--target-min); cursor: help; text-align: start; }
  button.badge:hover { background: var(--surface-hover); }
  [data-tone="gold"] { color: var(--gold); }
  [data-tone="success"] { color: var(--success); }
  [data-tone="danger"] { color: var(--danger); }
  [data-tone="estimated"], [data-tone="warning"], .estimate { color: var(--estimated); }
  [data-tone="information"] { color: var(--information); }
</style>

<script lang="ts">
  import { debugMode } from '../debug/debug';
  import Tooltip from './Tooltip.svelte';
  let { label, tone = 'neutral', estimated }: {
    label: string; tone?: 'neutral' | 'gold' | 'violet' | 'success' | 'danger' | 'warning' | 'estimated' | 'information'; estimated?: { reason: string };
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
  .badge { display: inline-flex; align-items: center; flex-wrap: wrap; gap: var(--space-1); padding: var(--space-1) var(--space-2); border: 1px solid var(--border-strong); border-radius: var(--radius-pill); background: var(--surface-raised); color: var(--text-secondary); font-family: var(--font-ui); font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-semibold); overflow-wrap: anywhere; }
  button.badge { min-height: var(--target-min); cursor: help; text-align: start; }
  button.badge:hover { background: var(--surface-hover); border-color: var(--text-muted); }
  [data-tone="gold"] { color: var(--gold); }
  [data-tone="violet"] { color: var(--violet-text); }
  [data-tone="success"] { color: var(--success); }
  [data-tone="danger"] { color: var(--danger); }
  [data-tone="estimated"], [data-tone="warning"], .estimate { color: var(--estimated); }
  [data-tone="information"] { color: var(--information); }
</style>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import Card from '../components/Card.svelte';
  let { number, heading, current, complete, children }: { number: number; heading: string; current: boolean; complete: boolean; children: Snippet } = $props();
  let phase = $derived(complete ? 'verified' : current ? 'current' : 'pending');
</script>
<li aria-current={current ? 'step' : undefined}>
  <Card {heading}>
    <p class="step-state" data-state={phase}>Step {number} · {complete ? 'Verified' : current ? 'Current' : 'Pending'}</p>
    {@render children()}
  </Card>
</li>
<style>
  /* The step number lives in quiet state text; the current step is a neutral surface step, never gold. */
  li { min-width: 0; list-style: none; }
  li[aria-current] :global(section) { background: var(--surface-selected); border-color: var(--hairline-strong); box-shadow: var(--highlight-top); }
  .step-state { margin: calc(-1 * var(--space-3)) 0 var(--space-3); color: var(--text-muted); font-size: var(--text-caption); line-height: var(--leading-caption); }
  .step-state[data-state="current"] { color: var(--text-primary); }
  .step-state[data-state="verified"] { color: var(--success); }
</style>

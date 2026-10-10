<script lang="ts">
  import type { Snippet } from 'svelte';
  import Card from '../components/Card.svelte';
  let { number, heading, current, complete, children }: { number: number; heading: string; current: boolean; complete: boolean; children: Snippet } = $props();
</script>
<li aria-current={current ? 'step' : undefined}>
  <Card heading={`${number}. ${heading}`}>
    <p class="step-state">{complete ? 'Verified' : current ? 'Current' : 'Pending'}</p>
    {@render children()}
  </Card>
</li>
<style>
  li { min-width: 0; list-style: none; border-inline-start: 3px solid transparent; }
  li[aria-current] { border-inline-start-color: var(--gold); }
  .step-state { color: var(--gold); font-size: var(--text-label); margin-bottom: var(--space-3); }
</style>

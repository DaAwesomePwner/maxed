<script lang="ts">
  // Harness-only component; production uses the shared Card component.
  let { title }: { title: string } = $props();
  let expanded = $state(false);
</script>

<section aria-label={title}>
  <h2>{title}</h2>
  <button type="button" aria-expanded={expanded} onclick={() => { expanded = !expanded; }}>
    {expanded ? 'Hide details' : 'Show details'}
  </button>
  <p role="status">{expanded ? 'Synthetic details' : ''}</p>
</section>

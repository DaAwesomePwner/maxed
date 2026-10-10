<script lang="ts">
  import { getContext } from 'svelte';
  import { STATUS_REGISTRY, type StatusRegistry } from './registry';
  let { message }: { message: string | null } = $props();
  const registry = getContext<StatusRegistry | undefined>(STATUS_REGISTRY);
  $effect(() => {
    if (message) return registry?.add(message);
  });
</script>

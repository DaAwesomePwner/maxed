<script lang="ts">
  import { onMount } from 'svelte';
  import { createClient, type Client } from './lib/bridge/client';
  import { nativeTransport } from './lib/bridge/native';
  import Shell from './lib/shell/Shell.svelte';
  import { productionFeatures, type Features } from './lib/navigation/routes';
  let { client = createClient(nativeTransport), preview = false, features = productionFeatures }: { client?: Client; preview?: boolean; features?: Features } = $props();
  let startup = $state<boolean | null>(null);
  onMount(() => {
    let mounted = true;
    if (preview) startup = false;
    else void client.queryStartupSetupRequired().then(
      required => { if (mounted) startup = required; },
      () => { if (mounted) startup = true; },
    );
    return () => { mounted = false; };
  });
</script>

<Shell {client} {preview} {features} initialSetupRequired={startup === true} />

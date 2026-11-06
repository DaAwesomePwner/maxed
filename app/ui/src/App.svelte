<script lang="ts">
  import { createClient, type Client } from './lib/bridge/client';
  import { nativeTransport } from './lib/bridge/native';
  import Shell from './lib/shell/Shell.svelte';
  import { productionFeatures, type Features } from './lib/navigation/routes';
  let { client = createClient(nativeTransport), preview = false, features = productionFeatures }: { client?: Client; preview?: boolean; features?: Features } = $props();
  let startup = $state<boolean | null>(null);
  let upgradeRequired = $state(false);
  $effect(() => {
    const owner = client, synthetic = preview;
    let mounted = true;
    startup = null; upgradeRequired = false;
    if (synthetic) startup = false;
    else {
      void owner.queryStartupSetupRequired().then(
        required => { if (mounted) startup = required; },
        () => { if (mounted) startup = true; },
      );
      void owner.queryHistoryUpgradeStatus().then(
        status => { if (mounted) upgradeRequired = status.phase !== 'ready'; },
        () => { if (mounted) upgradeRequired = true; },
      );
    }
    return () => { mounted = false; };
  });
</script>

<Shell {client} {preview} {features} initialSetupRequired={startup === true || upgradeRequired} />

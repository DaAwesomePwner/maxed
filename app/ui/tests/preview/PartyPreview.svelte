<script lang="ts">
  import { untrack } from 'svelte';
  import Shell from '../../src/lib/shell/Shell.svelte';
  import { productionFeatures } from '../../src/lib/navigation/routes';
  import { createPartyPreview } from './party-scenario';

  let { variant = new URLSearchParams(location.search).get('variant') === 'stress' ? 'stress' : 'default' }:
    { variant?: 'default' | 'stress' } = $props();
  const preview = untrack(() => {
    if (!location.hash) window.history.replaceState(null, '', '#/party');
    return createPartyPreview(variant);
  });
</script>

<Shell client={preview.client} preview features={{ ...productionFeatures, party: true }} />

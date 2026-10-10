<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import Shell from '../../src/lib/shell/Shell.svelte';
  import { createLogsPreview } from './logs-scenario';

  let { variant = new URLSearchParams(location.search).get('variant') === 'stress' ? 'stress' : 'default' }:
    { variant?: 'default' | 'stress' } = $props();
  const preview = untrack(() => {
    if (!location.hash) window.history.replaceState(null, '', '#/logs');
    return createLogsPreview(variant);
  });
  onDestroy(preview.dispose);
</script>

<Shell client={preview.client} preview resolveSpriteUrl={() => '/tests/preview/character.svg'} />

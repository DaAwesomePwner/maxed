<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import Shell from '../../src/lib/shell/Shell.svelte';
  import { createRunsPreview } from './runs-scenario';

  let { variant = new URLSearchParams(location.search).get('variant') === 'stress' ? 'stress' : 'default' }:
    { variant?: 'default' | 'stress' } = $props();
  const preview = untrack(() => {
    // Choose a useful default without replacing a supplied deep link.
    if (!location.hash) window.history.replaceState(null, '', '#/runs');
    return createRunsPreview(variant);
  });
  onDestroy(preview.dispose);
</script>

<Shell client={preview.client} preview resolveSpriteUrl={() => '/tests/preview/character.svg'} />

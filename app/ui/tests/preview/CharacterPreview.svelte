<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import Shell from '../../src/lib/shell/Shell.svelte';
  import { productionFeatures } from '../../src/lib/navigation/routes';
  import { characterVariant, createCharacterPreview, type CharacterVariant } from './character-scenario';

  let { variant = characterVariant(new URLSearchParams(location.search).get('variant')), preview: supplied }:
    { variant?: CharacterVariant; preview?: ReturnType<typeof createCharacterPreview> } = $props();
  const preview = untrack(() => {
    if (!location.hash) window.history.replaceState(null, '', '#/character');
    return supplied ?? createCharacterPreview(variant);
  });
  onMount(() => {
    // First retain the authored calculation, then exercise the real freeze transition.
    if (variant !== 'stale') return;
    const timer = setTimeout(() => preview.setStale(true), 100);
    return () => clearTimeout(timer);
  });
</script>

<Shell client={preview.client} preview features={{ ...productionFeatures, character: true }} />

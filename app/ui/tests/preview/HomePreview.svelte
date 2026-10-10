<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import Shell from '../../src/lib/shell/Shell.svelte';
  import { createHomePreview, previewMode, type PreviewMode } from './scenario';

  let { mode = previewMode(new URLSearchParams(location.search).get('preview')) }: { mode?: PreviewMode } = $props();
  // Each mount owns one frozen scenario, just as each browser URL owns one mode.
  const preview = untrack(() => createHomePreview(mode));
  onDestroy(preview.dispose);
</script>

<Shell client={preview.client} preview resolveSpriteUrl={() => '/tests/preview/character.svg'} />

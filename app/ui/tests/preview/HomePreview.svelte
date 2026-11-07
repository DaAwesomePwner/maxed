<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import Shell from '../../src/lib/shell/Shell.svelte';
  import { productionFeatures } from '../../src/lib/navigation/routes';
  import { createHomePreview, previewMode, type PreviewMode } from './scenario';

  let { mode = previewMode(new URLSearchParams(location.search).get('preview')) }: { mode?: PreviewMode } = $props();
  // Each mount owns one frozen scenario, including saved recap replies. Copying stays an explicit click.
  const preview = untrack(() => createHomePreview(mode));
  onDestroy(preview.dispose);
</script>

{#if new URLSearchParams(location.search).get('variant') === 'accounts'}
  <button type="button" onclick={preview.switchAccount}>Switch synthetic account</button>
{/if}
<Shell client={preview.client} preview features={{ ...productionFeatures, homeRedesign: mode.startsWith('home-v4-') }}
  resolveSpriteUrl={() => '/tests/preview/character.svg'} />

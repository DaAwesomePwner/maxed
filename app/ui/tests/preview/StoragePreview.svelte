<script lang="ts">
  import { untrack } from 'svelte';
  import Shell from '../../src/lib/shell/Shell.svelte';
  import { createStoragePreview, storagePreviewMarker } from './storage-scenario';
  let { surface = new URLSearchParams(location.search).get('preview') === 'upgrade' ? 'upgrade' : 'storage',
    variant = new URLSearchParams(location.search).get('variant') ?? 'default' }:
    { surface?: 'storage' | 'upgrade'; variant?: string } = $props();
  const scenario = untrack(() => {
    if (!location.hash) window.history.replaceState(null, '', surface === 'storage' ? '#/settings' : '#/setup');
    return createStoragePreview(surface, variant);
  });
</script>

<!-- Fixture transport owns every command. Real controls stay interactive for acceptance. -->
<div data-preview={storagePreviewMarker}>
  <div class="preview-controls" aria-label="Synthetic storage controls">
    <span>Synthetic {surface} preview</span>
    <button onclick={() => scenario.transport.advanceStorageMaintenance()}>Advance receipt job</button>
    <button onclick={() => scenario.transport.advanceHistoryUpgrade()}>Advance history upgrade</button>
    <button onclick={() => scenario.transport.advanceHistoryUpgrade('verification_failed')}>Fail upgrade backup</button>
  </div>
  <Shell client={scenario.client} resolveSpriteUrl={() => '/tests/preview/character.svg'} />
</div>
<style>
  .preview-controls { position: fixed; z-index: 100; bottom: 0; right: 0; display: flex; flex-wrap: wrap; gap: 8px; padding: 4px; background: var(--surface-panel); font-size: 11px; }
</style>

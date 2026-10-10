<script lang="ts">
  import BannerHost from '../../../src/lib/components/BannerHost.svelte';
  import type { AlertKind } from '../../../src/lib/bindings/AlertKind';
  let { id = 'event-1', kind = 'white_bag', title = 'Synthetic drop', context = 'Synthetic dungeon', visible = true, onDismiss = () => {} }: {
    id?: string; kind?: AlertKind; title?: string; context?: string; visible?: boolean; onDismiss?: () => void;
  } = $props();
</script>
<BannerHost moment={visible ? { id, kind, title, context } : null} {onDismiss} />

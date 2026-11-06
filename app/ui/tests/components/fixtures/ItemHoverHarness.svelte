<script lang="ts">
  import { itemHover, type ItemHoverParams } from '../../../src/lib/components/item-hover';
  let { suffix = '', first = { type: 10, name: 'Fallback A' } }: { suffix?: string; first?: ItemHoverParams } = $props();
</script>

<a href="/items/10" aria-describedby="existing" use:itemHover={{ ...first, suffix }}>Item A</a>
<a href="/items/11" use:itemHover={{ type: 11, name: 'Fallback B' }}>Item B</a>

<script lang="ts">
  import '../../../src/lib/styles/tokens.css';
  import '../../../src/lib/styles/motion.css';
  import Badge from '../../../src/lib/components/Badge.svelte';
  import Banner from '../../../src/lib/components/Banner.svelte';
  import Card from '../../../src/lib/components/Card.svelte';
  import EmptyState from '../../../src/lib/components/EmptyState.svelte';
  import ListRow from '../../../src/lib/components/ListRow.svelte';
  import SpriteImage from '../../../src/lib/components/SpriteImage.svelte';
  import Tooltip from '../../../src/lib/components/Tooltip.svelte';
  import { bagTones } from '../../../src/lib/styles/rarity';
  import type { SpriteRef } from '../../../src/lib/bindings/SpriteRef';
  import type { AlertKind } from '../../../src/lib/bindings/AlertKind';
  const sprite: SpriteRef = { key: 'preview', sheet: 'atlas.svg', x: 0, y: 0, width: 8, height: 8 };
  const sheetUrl = new URL('./atlas.svg', import.meta.url).href;
  const kinds: AlertKind[] = ['white_bag', 'personal_best', 'flawless_run', 'death'];
  let dismissed = $state<AlertKind[]>([]);
  let retried = $state(false);
</script>

<div class="preview">
  <h1>Arcane Modern · component preview</h1>
  <p>Synthetic fixtures only. Check at 100% and 150%, with keyboard focus and reduced motion.</p>
  <Card heading="Your session" tone="hero">
    <div class="flow">
      <SpriteImage {sprite} {sheetUrl} alt="Synthetic pixel tile" />
      <SpriteImage sprite={null} sheetUrl={null} alt="Unknown class" />
      <Badge label="Observed" tone="success" />
      <Badge label="Party size" tone="estimated" estimated={{ reason: 'Maximum players observed during this partial capture.' }} />
      <Badge label="Coverage" tone="estimated" estimated={{ reason: 'A separate explanation to check independent focus and hover.' }} />
    </div>
  </Card>
  <Card heading="Bag presentation tones">
    <div class="flow">{#each bagTones as tone}<span class="bag bag-glow" data-bag-tone={tone}>{tone}</span>{/each}</div>
    <p>These are visual categories, not tier rankings.</p>
  </Card>
  <div class="flow">
    {#each ['neutral', 'gold', 'violet', 'success', 'danger', 'warning', 'information'] as tone}
      <Badge label={tone} tone={tone as 'neutral' | 'gold' | 'violet' | 'success' | 'danger' | 'warning' | 'information'} />
    {/each}
    <button type="button" disabled>Capture unavailable</button>
    <Tooltip id="preview-tooltip" text="Evidence remains visible when moving the pointer here. Escape dismisses it.">
      {#snippet trigger(describedby)}<button type="button" aria-describedby={describedby}>Inspect evidence</button>{/snippet}
    </Tooltip>
  </div>
  <Card heading="Recent runs">
    <ListRow href="#story" label="Synthetic dungeon" selected><span>11:04 · cleared</span></ListRow>
    <ListRow href="#unknown" label="Long synthetic run name with incomplete observations"><Badge label="Unknown outcome" tone="warning" /></ListRow>
  </Card>
  <div class="grid">
    <EmptyState heading="No runs yet" description="Enter a dungeon while capture is running." />
    <EmptyState heading="Loading runs" description="Reading your saved runs." state="loading" />
    <EmptyState heading="Runs unavailable" description="Capture has not produced a session yet." state="unavailable" />
    <EmptyState heading={retried ? 'Retry requested' : 'Could not load runs'} description="Retry reading saved runs." state="error">
      {#snippet action()}<button type="button" onclick={() => { retried = true; }}>Retry</button>{/snippet}
    </EmptyState>
  </div>
  {#each kinds as kind}{#if !dismissed.includes(kind)}
    <Banner id={`preview-${kind}`} {kind} title="Synthetic moment" context="A long synthetic dungeon and encounter name to check wrapping without covering the rail or breadcrumbs." onDismiss={() => { dismissed = [...dismissed, kind]; }} />
  {/if}{/each}
</div>

<style>
  .preview { display: grid; gap: var(--space-6); padding: var(--space-8); background: var(--surface-canvas); color: var(--text-primary); font: var(--text-body)/var(--leading-body) var(--font-ui); }
  h1 { margin: 0; font-size: var(--text-hero); line-height: var(--leading-hero); }
  p { margin: 0; color: var(--text-secondary); }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 16rem), 1fr)); gap: var(--space-4); }
  .flow { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-4); }
  .bag { padding: var(--space-2); background: var(--surface-raised); border-radius: var(--radius-sprite); }
  button { min-height: var(--target-min); border: 1px solid var(--border-strong); border-radius: var(--radius-control); background: var(--surface-raised); color: var(--text-primary); padding: var(--space-2); font: inherit; }
  button:disabled { color: var(--text-disabled); cursor: not-allowed; opacity: 1; }
</style>

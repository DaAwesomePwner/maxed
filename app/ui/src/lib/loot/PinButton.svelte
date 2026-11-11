<script lang="ts">
  import '../styles/controls.css';
  import { onDestroy } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { AppSettings } from '../bindings/AppSettings';
  import type { ChaseItem } from '../bindings/ChaseItem';
  let { client, itemType, dungeon = null, settings, onChange, disabled = false, onSaving, onSaved, variant = 'label', name }: {
    client: Client; itemType: number; dungeon?: string | null; settings: AppSettings | null;
    onChange?: (chase: ChaseItem[]) => void; disabled?: boolean; onSaving?: (saving: boolean) => void;
    onSaved?: (chase: ChaseItem[]) => void; variant?: 'label' | 'icon'; name?: string;
  } = $props();
  let saving = $state(false), error = $state('');
  let disposed = false;
  let pinned = $derived(settings?.loot.chase.some(item => item.item_type === itemType && item.dungeon === (dungeon ?? null)) ?? false);
  onDestroy(() => { disposed = true; });
  async function toggle() {
    if (saving || disabled || !settings) return;
    const owner = client, type = itemType, place = dungeon ?? null, notifySaving = onSaving, notifySaved = onSaved;
    const chase = $state.snapshot(settings.loot.chase);
    const next = pinned ? chase.filter(item => item.item_type !== type || item.dungeon !== place)
      : [...chase, { item_type: type, dungeon: place, pinned_at: (BigInt(Date.now()) * 1000n).toString() }];
    saving = true; error = '';
    notifySaving?.(true);
    try {
      await owner.setChaseList(next);
      notifySaved?.(next);
      if (!disposed && client === owner && itemType === type && (dungeon ?? null) === place) onChange?.(next);
    } catch {
      if (!disposed) error = "Couldn't save the chase list";
    } finally {
      if (!disposed) saving = false;
      notifySaving?.(false);
    }
  }
</script>

<div class="pin">
  <button type="button" class="ctl" class:sq={variant === 'icon'} class:ghost={variant === 'icon'} aria-label={variant === 'icon' ? `${pinned ? 'Stop chasing' : 'Chase'} ${name ?? `#${itemType}`}` : undefined} aria-pressed={pinned} disabled={saving || disabled || settings === null} onclick={toggle}><span aria-hidden="true">★</span>{#if variant === 'label'}{pinned ? 'Chasing' : 'Chase'}{/if}</button>
  {#if error}<p role="alert">{error}</p>{/if}
</div>

<style>
  .pin { display: grid; gap: var(--space-1); }
  .ctl { color: var(--text-secondary); justify-self: start; }
  /* A chased item is yours, so its star is gold; the button frame stays neutral. */
  .ctl[aria-pressed="true"] { color: var(--gold); border-color: var(--text-secondary); }
  .ctl.ghost[aria-pressed="true"] { border-color: transparent; }
  .ctl.sq { width: 24px; height: 24px; }
  p { margin: 0; color: var(--warning); font-size: var(--text-meta); }
</style>

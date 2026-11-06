<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { Client } from '../../bridge/client';
  import { objectVisual } from '../../bridge/visual-index';
  import { spriteUrl } from '../../bridge/visuals';
  import type { HomeVisuals } from '../visuals';
  import type { RunRecapView, VisitRecapView } from './recap-view';
  import { defaultRecapImageSettings, loadSprite, recapImageModel, renderRecapImage, type RecapImageContext } from './recap-image';
  import { copyPng } from './clipboard';

  let { view, settings, visuals, context }: {
    view: RunRecapView | VisitRecapView; settings: Pick<Client, 'querySettings' | 'updateSettings'>; visuals: HomeVisuals;
    context: Pick<RecapImageContext, 'character' | 'className' | 'date'>;
  } = $props();
  let options = $state({ ...defaultRecapImageSettings });
  let loading = $state(true), saving = $state(false), copying = $state(false), open = $state(false), status = $state('');
  let disposed = false, timer: ReturnType<typeof setTimeout> | undefined;
  let optionsButton: HTMLButtonElement;
  const menuId = $props.id();
  function announce(message: string) {
    if (disposed) return;
    clearTimeout(timer); status = message;
    timer = setTimeout(() => { status = ''; }, 4000);
  }
  onMount(() => {
    void settings.querySettings().then(current => { if (!disposed) options = { ...current.recap_image }; })
      .catch(() => { if (!disposed) options = { ...defaultRecapImageSettings }; })
      .finally(() => { if (!disposed) loading = false; });
  });
  onDestroy(() => { disposed = true; clearTimeout(timer); });
  function closeOnEscape(event: KeyboardEvent) {
    if (event.key === 'Escape') { open = false; optionsButton.focus(); }
  }
  function changed(key: keyof typeof options, input: HTMLInputElement) {
    const checked = input.checked;
    // Keep the accepted preference visible until the save succeeds, including
    // when a rejected write leaves the reactive value unchanged.
    input.checked = options[key];
    void toggle(key, checked);
  }
  async function toggle(key: keyof typeof options, checked: boolean) {
    if (loading || saving) return;
    saving = true;
    try {
      const current = await settings.querySettings();
      const next = { ...current.recap_image, [key]: checked };
      await settings.updateSettings({ ...current, recap_image: next });
      if (!disposed) options = next;
    } catch {
      options = { ...options };
      announce("Couldn't save image options");
    } finally { if (!disposed) saving = false; }
  }
  async function copy() {
    if (loading || saving || copying) return;
    copying = true; clearTimeout(timer); status = '';
    try {
      const imageView = view.kind === 'visit' ? { ...view, events: view.events.map(row => ({ ...row,
        name: objectVisual(visuals.objects, row.targetType)?.name ?? row.name })) } : view;
      const model = recapImageModel(imageView, options, { ...context,
        spriteUrl: type => {
          const sprite = objectVisual(visuals.objects, type)?.sprite;
          return sprite ? spriteUrl(visuals.client_fingerprint, sprite) : null;
        },
        playerClassName: type => objectVisual(visuals.objects, type)?.name ?? null,
        objectName: type => objectVisual(visuals.objects, type)?.name ?? null,
      });
      const blob = await renderRecapImage(model, loadSprite);
      if (!disposed) announce(await copyPng(blob) === 'copied' ? 'Copied' : "Couldn't copy the image");
    } catch { announce("Couldn't copy the image"); }
    finally { if (!disposed) copying = false; }
  }
</script>

<div class="recap-copy">
  <div class="copy-controls">
    <button type="button" onclick={copy} aria-disabled={loading || saving || copying}>Copy recap image</button>
    <button type="button" bind:this={optionsButton} aria-label="Recap image options" aria-expanded={open} aria-controls={menuId}
      onclick={() => { open = !open; }} onkeydown={closeOnEscape}>Options</button>
  </div>
  {#if open}
    <fieldset id={menuId}>
      <legend>Include in image</legend>
      {#each [['names', 'Names'], ['loot', 'Loot'], ['fights', 'Fights']] as [key, label]}
        <label><input type="checkbox" checked={options[key as keyof typeof options]} aria-disabled={loading || saving}
          onchange={event => changed(key as keyof typeof options, event.currentTarget)} onkeydown={closeOnEscape} />{label}</label>
      {/each}
    </fieldset>
  {/if}
  <span class="copy-status" role="status">{status}</span>
</div>

<style>
  .recap-copy { display: grid; gap: 6px; position: relative; }
  .copy-controls { display: flex; flex-wrap: wrap; gap: 6px; }
  button { min-height: var(--target-min); padding: 6px 12px; font: inherit; color: var(--text-primary); background: var(--surface-panel); border: 1px solid var(--border-strong); border-radius: var(--radius-control); }
  [aria-disabled="true"] { opacity: .55; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  fieldset { margin: 0; padding: 8px 12px; display: flex; flex-wrap: wrap; gap: 12px; border: 1px solid var(--border-strong); border-radius: var(--radius-control); }
  legend, .copy-status { font-size: 12px; color: var(--text-secondary); }
  label { display: flex; align-items: center; gap: 6px; min-height: var(--target-min); }
  .copy-status { min-height: 1em; }
</style>

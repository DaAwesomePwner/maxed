<script lang="ts">
  import { tick } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { PartySettings } from '../bindings/PartySettings';
  import type { RequirementPreset } from '../bindings/RequirementPreset';
  import { presetSummary } from './party-screen';
  import PresetEditor from './PresetEditor.svelte';
  import '../styles/controls.css';
  let { client }: { client: Client } = $props();
  let settings = $state.raw<PartySettings | null>(null), selected = $state.raw<RequirementPreset | null>(null);
  let busy = $state(false), error = $state(''), retry = $state(0), deleting = $state(false);
  let dialog = $state<HTMLDialogElement>();
  let dirty = $state(false), discarding = $state(false);
  let pendingSelection = $state.raw<RequirementPreset | null>(null);
  let returnFocus: HTMLElement | null = null;
  let newButton = $state<HTMLButtonElement>();
  const editButtons = $state<Record<string, HTMLButtonElement | undefined>>({});
  let focusAfterWrite = false;
  let lifetime = 0;
  $effect(() => {
    const owner = client, generation = ++lifetime; retry;
    settings = null; selected = null; error = ''; busy = false; deleting = false;
    void owner.querySettings().then(value => { if (lifetime === generation) settings = value.party; })
      .catch(() => { if (lifetime === generation) error = 'Could not load presets. Retry to refresh them.'; });
    return () => { lifetime++; };
  });
  $effect(() => {
    const element = dialog;
    if (element) {
      returnFocus = document.activeElement as HTMLElement | null;
      if (element.showModal) element.showModal(); else element.open = true;
      return () => { if (element.open) element.close?.(); };
    }
  });
  function closeDialog() {
    dialog?.close?.(); deleting = false; discarding = false; pendingSelection = null;
    if (busy) focusAfterWrite = true;
    else returnFocus?.focus();
  }
  function applySelection(preset: RequirementPreset | null) {
    const previousId = selected?.id;
    selected = preset; error = '';
    if (!preset) void tick().then(() => {
      if (selected !== null) return;
      const button = previousId ? editButtons[previousId] : undefined;
      if (button?.isConnected) button.focus(); else newButton?.focus();
    });
  }
  function select(preset: RequirementPreset | null) {
    if (preset?.id === selected?.id) return;
    if (dirty) { pendingSelection = preset; discarding = true; }
    else applySelection(preset);
  }
  function discard() {
    const next = pendingSelection;
    closeDialog(); applySelection(next); dirty = false;
  }
  function create() {
    if (!settings || settings.presets.length >= 32 || busy) return;
    select({ id: crypto.randomUUID(), name: '', min_maxed: null, min_tier: {}, ut_st_meets: true, auto_apply: [] });
  }
  async function persist(next: PartySettings, after: () => void) {
    if (busy) return;
    const generation = lifetime;
    busy = true; error = '';
    try {
      await client.updatePartySettings(next);
      if (generation === lifetime) { settings = next; after(); }
    } catch (cause) {
      if (generation === lifetime) error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      if (generation === lifetime) {
        busy = false;
        if (focusAfterWrite) {
          focusAfterWrite = false;
          void tick().then(() => { if (returnFocus?.isConnected) returnFocus.focus(); else newButton?.focus(); });
        }
      }
    }
  }
  function save(preset: RequirementPreset, makeDefault: boolean) {
    if (!settings) return;
    const exists = settings.presets.some(row => row.id === preset.id);
    const presets = settings.presets.map(row => row.id === preset.id ? preset
      : { ...row, auto_apply: row.auto_apply.filter(name => !preset.auto_apply.includes(name)) });
    if (!exists) presets.push(preset);
    const defaultId = makeDefault ? preset.id : settings.default_preset_id === preset.id ? null : settings.default_preset_id ?? null;
    void persist({ presets, default_preset_id: defaultId }, () => { selected = preset; });
  }
  function remove() {
    if (!settings || !selected) return;
    const id = selected.id;
    void persist({ presets: settings.presets.filter(row => row.id !== id), default_preset_id: settings.default_preset_id === id ? null : settings.default_preset_id ?? null },
      () => { closeDialog(); applySelection(null); dirty = false; });
  }
</script>
<section aria-label="Party presets">
  {#if error && !deleting && !discarding}<p role="alert">{error}</p>{/if}
  {#if !settings}
    {#if error}<button class="ctl" onclick={() => { retry++; }}>Retry</button>{:else}<p role="status">Loading presets</p>{/if}
  {:else}
    <div class="layout" class:with-editor={selected !== null}>
      <section class="panel list" aria-label="Saved presets">
        <header><h2>Presets</h2><button class="ctl" bind:this={newButton} disabled={busy || settings.presets.length >= 32} onclick={create}>New preset</button></header>
        {#if settings.presets.length >= 32}<p class="hint">32 presets saved. Delete a preset to add another.</p>{/if}
        <div class="table-scroll"><table aria-label="Requirement presets"><thead><tr><th>Name</th><th>Rules</th><th>Auto-apply in</th><th>Default</th></tr></thead>
          <tbody>{#each settings.presets as preset (preset.id)}<tr class:selected={selected?.id === preset.id}>
            <td><button class="edit" bind:this={editButtons[preset.id]} aria-label={`Edit ${preset.name}`} disabled={busy} onclick={() => select(preset)}>{preset.name}</button></td>
            <td>{presetSummary(preset)}</td><td>{preset.auto_apply.join(', ') || 'Manual'}</td><td>{settings.default_preset_id === preset.id ? 'Default' : ''}</td>
          </tr>{/each}</tbody>
        </table></div>
        {#if !settings.presets.length}<p class="hint">No presets yet. Create a preset to check party requirements.</p>{/if}
      </section>
      {#if selected}<div class="panel">{#key selected.id}<PresetEditor {client} preset={selected} {settings} {busy} bind:dirty onSave={save}
        onDelete={() => { error = ''; deleting = true; }} onCancel={() => select(null)} />{/key}</div>{/if}
    </div>
  {/if}
  {#if (deleting && selected) || discarding}<dialog bind:this={dialog} aria-label={discarding ? 'Discard unsaved changes?' : 'Delete preset'}
    oncancel={event => { event.preventDefault(); if (!busy) closeDialog(); }} onclose={closeDialog}>
    <h3>{discarding ? 'Discard unsaved changes?' : 'Delete preset'}</h3>
    {#if !discarding}<p>Delete {selected?.name}?</p>{/if}
    {#if error}<p role="alert">{error}</p>{/if}
    <div class="actions"><button class="ctl" disabled={busy} onclick={closeDialog}>Cancel</button><button class="ctl" disabled={busy} onclick={() => discarding ? discard() : remove()}>{busy ? 'Deleting…' : 'Confirm'}</button></div>
  </dialog>{/if}
</section>
<style>
  .layout { display: grid; align-items: start; gap: var(--space-3); } .with-editor { grid-template-columns: minmax(0, 1fr) 340px; }
  .panel { min-width: 0; padding: var(--space-3); background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-card); }
  header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); margin-bottom: var(--space-3); } h2 { font-size: 14px; margin: 0; }
  .table-scroll { overflow-x: auto; } table { width: 100%; border-collapse: collapse; font-size: 12px; } th { text-align: left; color: var(--text-secondary); } th, td { padding: 6px; } td { border-top: 1px solid var(--border); } .selected { background: #f2c14e14; }
  .edit { font: inherit; text-align: left; color: var(--text-primary); background: none; border: 0; min-height: var(--target-min); cursor: pointer; } .edit:hover { color: var(--gold); } .hint { color: var(--text-secondary); font-size: 12px; } [role=alert] { color: var(--danger); white-space: pre-wrap; }
  dialog { max-width: 400px; background: var(--surface-panel); color: var(--text-primary); border: 1px solid var(--border-strong); border-radius: var(--radius-card); padding: var(--space-4); } dialog::backdrop { background: #0009; } .actions { display: flex; justify-content: flex-end; gap: var(--space-2); }
  @media (max-width: 900px) { .with-editor { grid-template-columns: minmax(0, 1fr); } }
</style>
